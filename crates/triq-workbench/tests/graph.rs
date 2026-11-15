//! Turning what the runtimes report into the drawing: one card per node, one wire per connection.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use std::time::Duration;
use triq_workbench::graph::{self, Card};
use triq_workbench::snarl::Snarl;

fn keys(snarl: &Snarl<Card>) -> Vec<String> {
    let mut keys: Vec<String> = snarl.nodes().map(Card::key).collect();
    keys.sort();
    keys
}

fn sorted<const N: usize>(mut names: [String; N]) -> Vec<String> {
    names.sort();
    names.to_vec()
}

/// Wires as `output -> input` names.
fn wires(snarl: &Snarl<Card>) -> Vec<String> {
    let mut wires: Vec<String> = snarl
        .wires()
        .map(|(from, to)| format!("{} -> {}", snarl[from.node].output(from.output), snarl[to.node].input(to.input)))
        .collect();
    wires.sort();
    wires
}

#[test]
fn every_node_becomes_a_card_and_every_live_connection_a_wire() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name(), scene.desk.name());
    let mut snarl = Snarl::new();
    graph::sync(&mut snarl, &scene.inventories());

    assert_eq!(keys(&snarl), sorted([format!("{desk}/viewer"), format!("{robot}/imu"), format!("{robot}/smoother")]));
    assert_eq!(
        wires(&snarl),
        sorted([
            format!("{robot}/imu/pose -> {robot}/smoother/raw"),
            format!("{robot}/smoother/smooth -> {desk}/viewer/pose")
        ])
    );
    // The connection to an output that does not exist has no wire; the card says what it waits for.
    let viewer = snarl.nodes().find(|card| card.node.name == "viewer").unwrap();
    assert_eq!(viewer.waiting(0), Vec::<String>::new());
    assert_eq!(viewer.waiting(1), [format!("{robot}/camera/image")]);
    Ok(())
}

#[test]
fn syncing_again_follows_the_machine_and_keeps_what_the_user_arranged() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let mut snarl = Snarl::new();
    graph::sync(&mut snarl, &scene.inventories());

    // The user drags a card somewhere.
    let (id, _) = snarl.nodes_ids_data().find(|(_, node)| node.value.node.name == "imu").unwrap();
    let chosen = triq_workbench::egui::pos2(777.0, 555.0);
    snarl.get_node_info_mut(id).unwrap().pos = chosen;

    // Meanwhile the machine changes: data flows, a connection goes, a node goes, a node comes.
    scene.imu.pose.send(Pose::default())?;
    assert!(wait_for(|| scene.viewer.pose.recv().unwrap().is_some()));
    scene.desk.disconnect(&format!("{robot}/smoother/smooth"), "viewer/pose")?;
    scene.robot.spawn_kind("filter", "second", triq::Placement::Dedicated, serde_json::json!(null))?;
    scene.robot.connect("imu/pose", "second/raw")?;
    graph::sync(&mut snarl, &scene.inventories());

    assert_eq!(snarl.get_node_info(id).unwrap().pos, chosen, "a card stays where it was put");
    assert_eq!(snarl[id].node.outputs[0].sent, 1, "counters are fresh");
    assert_eq!(
        wires(&snarl),
        sorted([
            format!("{robot}/imu/pose -> {robot}/second/raw"),
            format!("{robot}/imu/pose -> {robot}/smoother/raw")
        ])
    );

    scene.robot.remove("smoother")?;
    graph::sync(&mut snarl, &scene.inventories());
    assert_eq!(keys(&snarl), sorted([format!("{desk}/viewer"), format!("{robot}/imu"), format!("{robot}/second")]));
    assert_eq!(wires(&snarl), [format!("{robot}/imu/pose -> {robot}/second/raw")]);

    // A whole program going away takes its cards with it.
    graph::sync(&mut snarl, &[scene.desk.inventory()]);
    assert_eq!(keys(&snarl), [format!("{desk}/viewer")]);
    assert!(wires(&snarl).is_empty());
    Ok(())
}

#[test]
fn new_cards_are_laid_out_left_to_right_without_overlapping() -> triq::Result {
    let scene = Scene::new()?;
    let mut snarl = Snarl::new();
    graph::sync(&mut snarl, &scene.inventories());
    let position = |name: &str| snarl.nodes_pos().find(|(_, card)| card.node.name == name).unwrap().0;
    assert!(position("imu").x < position("smoother").x, "a source is left of what it feeds");
    assert!(position("smoother").x < position("viewer").x);

    // Three more sources share the first column.
    let _more: Vec<ImuPorts> = (0..3).map(|i| scene.robot.attach(&format!("extra{i}"))).collect::<triq::Result<_>>()?;
    graph::sync(&mut snarl, &scene.inventories());
    let mut places: Vec<(i32, i32)> = snarl.nodes_pos().map(|(pos, _)| (pos.x as i32, pos.y as i32)).collect();
    places.sort();
    places.dedup();
    assert_eq!(places.len(), 6, "no two cards share a place");
    Ok(())
}

#[test]
fn only_ports_of_the_same_message_type_can_be_wired() -> triq::Result {
    let scene = Scene::new()?;
    let inventory = scene.desk.inventory();
    let viewer = &inventory.nodes[0];
    let (pose, image) = (&viewer.inputs[0].port, &viewer.inputs[1].port);
    let robot = scene.robot.inventory();
    let imu_pose = &robot.nodes.iter().find(|node| node.name == "imu").unwrap().outputs[0].port;
    assert!(graph::compatible(imu_pose, pose));
    assert!(!graph::compatible(imu_pose, image));
    assert_eq!(graph::color(&imu_pose.message), graph::color(&pose.message), "one colour per message type");
    assert_ne!(graph::color(&imu_pose.message), graph::color(&image.message));
    // The same data with a header beside it is another message type.
    let headed = triq::MessageType { header: "Frame".into(), ..image.message.clone() };
    assert_ne!(graph::color(&headed), graph::color(&image.message));
    Ok(())
}

#[test]
fn tidying_puts_every_card_right_of_what_feeds_it() -> triq::Result {
    let scene = Scene::new()?;
    let mut snarl = Snarl::new();
    graph::sync(&mut snarl, &scene.inventories());
    // A node that is wired only after it appeared was placed as if it stood alone.
    scene.robot.spawn_kind("filter", "late", triq::Placement::Dedicated, serde_json::json!(null))?;
    graph::sync(&mut snarl, &scene.inventories());
    scene.robot.connect("smoother/smooth", "late/raw")?;
    graph::sync(&mut snarl, &scene.inventories());
    let position =
        |snarl: &Snarl<Card>, name: &str| snarl.nodes_pos().find(|(_, card)| card.node.name == name).unwrap().0;
    assert!(position(&snarl, "late").x < position(&snarl, "smoother").x);

    graph::tidy(&mut snarl);
    assert!(position(&snarl, "imu").x < position(&snarl, "smoother").x);
    assert!(position(&snarl, "smoother").x < position(&snarl, "late").x);
    assert_eq!(position(&snarl, "late").x, position(&snarl, "viewer").x, "both are fed by the smoother");
    assert_ne!(position(&snarl, "late").y, position(&snarl, "viewer").y);
    Ok(())
}

/// Pretend each runtime took its second reading this long after its first.
fn later(before: &[triq::Inventory], mut after: Vec<triq::Inventory>, elapsed: [Duration; 2]) -> Vec<triq::Inventory> {
    for ((after, before), elapsed) in after.iter_mut().zip(before).zip(elapsed) {
        after.taken_ns = before.taken_ns + elapsed.as_nanos() as u64;
    }
    after
}

#[test]
fn rates_come_from_two_readings_of_the_counters() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name(), scene.desk.name());
    let (sent, received, passed_on, shown) = (
        format!("{robot}/imu/pose"),
        format!("{robot}/smoother/raw"),
        format!("{robot}/smoother/smooth"),
        format!("{desk}/viewer/pose"),
    );
    let before = scene.inventories();
    assert!(graph::rates(&before, &before).is_empty(), "one reading gives no rate");

    for _ in 0..30 {
        scene.imu.pose.send(Pose::default())?;
    }
    let count = |port: &str| graph::counts(&scene.inventories())[port];
    assert!(wait_for(|| {
        while scene.viewer.pose.recv().expect("recv").is_some() {}
        count(&shown) == 30
    }));

    // Every runtime says when it read its own counters, and no two read them
    // at the same moment. Here the desk (listed first) took a quarter of a
    // second between its readings and the robot half a second.
    let quarter = Duration::from_millis(250);
    let after = later(&before, scene.inventories(), [quarter, 2 * quarter]);
    let rates = graph::rates(&before, &after);
    assert_eq!((rates[&sent], rates[&received], rates[&passed_on]), (60.0, 60.0, 60.0));
    assert_eq!(rates[&shown], 120.0);

    // A port that was not there the first time has no rate yet.
    let late: ImuPorts = scene.robot.attach("late")?;
    late.pose.send(Pose::default())?;
    let newer = later(&after, scene.inventories(), [quarter; 2]);
    let rates = graph::rates(&after, &newer);
    assert!(!rates.contains_key(&format!("{robot}/late/pose")));
    assert_eq!(rates[&sent], 0.0, "and one that has gone quiet reads zero");

    // A program that started again counts from nothing: its old readings say nothing about its new ones.
    let mut restarted = newer.clone();
    restarted.iter_mut().for_each(|inventory| inventory.pid += 1);
    assert!(graph::rates(&after, &later(&after, restarted, [quarter; 2])).is_empty());
    Ok(())
}
