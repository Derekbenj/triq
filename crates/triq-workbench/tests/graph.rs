//! Turning what the runtimes report into the drawing: one card per node, one wire per connection.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use std::collections::BTreeSet;
use std::time::Duration;
use triq::prelude::*;
use triq_workbench::egui::{Rect, pos2};
use triq_workbench::graph::{self, Card, Place, Places, Reading};
use triq_workbench::snarl::Snarl;

#[derive(Ports)]
#[allow(dead_code)]
struct WidePorts {
    a_port_with_quite_a_long_name_in: Input<Pose>,
    a_port_with_quite_a_long_name_out: Output<Pose>,
}

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
    let viewer = snarl.nodes().find(|card| card.name() == "viewer").unwrap();
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
    let (id, _) = snarl.nodes_ids_data().find(|(_, node)| node.value.name() == "imu").unwrap();
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
    assert_eq!(snarl[id].node().expect("a node").outputs[0].sent, 1, "counters are fresh");
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
    let position = |name: &str| snarl.nodes_pos().find(|(_, card)| card.name() == name).unwrap().0;
    assert!(position("imu").x < position("smoother").x, "a source is left of what it feeds");
    assert!(position("smoother").x < position("viewer").x);

    // Three more sources share the first column.
    let _more: Vec<ImuPorts> = (0..3).map(|i| scene.robot.attach(&format!("extra{i}"))).collect::<triq::Result<_>>()?;
    graph::sync(&mut snarl, &scene.inventories());
    let cards: Vec<_> =
        snarl.nodes_pos().map(|(pos, card)| (card.key(), Rect::from_min_size(pos, graph::size(card)))).collect();
    assert_eq!(cards.len(), 6);
    for (index, (name, card)) in cards.iter().enumerate() {
        for (other_name, other) in &cards[index + 1..] {
            assert!(!card.intersects(*other), "{name} and {other_name} overlap");
        }
    }

    // A card with long port names is wider, and what it feeds keeps clear of it.
    let wide: WidePorts = scene.robot.attach("wide")?;
    let _fed: WidePorts = scene.robot.attach("fed")?;
    scene.robot.connect("wide/a_port_with_quite_a_long_name_out", "fed/a_port_with_quite_a_long_name_in")?;
    graph::sync(&mut snarl, &scene.inventories());
    let card = |name: &str| {
        let (pos, card) = snarl.nodes_pos().find(|(_, card)| card.name() == name).unwrap();
        Rect::from_min_size(pos, graph::size(card))
    };
    assert!(card("wide").width() > card("imu").width());
    assert!(card("wide").right() < card("fed").left(), "with room for the wire between them");
    drop(wide);
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
    // The same data with a header beside it is another message type.
    let headed = triq::PortSpec {
        message: triq::MessageType { header: "Frame".into(), ..image.message.clone() },
        ..imu_pose.clone()
    };
    assert!(!graph::compatible(&headed, image));
    // And an input is wired to an output, not to another input.
    assert!(!graph::compatible(pose, pose));
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
    let position = |snarl: &Snarl<Card>, name: &str| snarl.nodes_pos().find(|(_, card)| card.name() == name).unwrap().0;
    assert!(position(&snarl, "late").x < position(&snarl, "smoother").x);

    graph::tidy(&mut snarl);
    assert!(position(&snarl, "imu").x < position(&snarl, "smoother").x);
    assert!(position(&snarl, "smoother").x < position(&snarl, "late").x);
    assert_eq!(position(&snarl, "late").x, position(&snarl, "viewer").x, "both are fed by the smoother");
    assert_ne!(position(&snarl, "late").y, position(&snarl, "viewer").y);
    Ok(())
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
    // A node with nothing connected to it, and nothing to send.
    let _idle: FilterPorts = scene.robot.attach("idle")?;
    let before = scene.inventories();
    assert!(graph::rates(&before, &before).is_empty(), "one reading gives no rate");

    for _ in 0..30 {
        scene.imu.pose.send(Pose::default())?;
    }
    // How many samples the viewer has taken so far.
    let count = |_: &str| scene.desk.inventory().nodes[0].inputs[0].sources[0].received;
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
    // Every port has a rate, whether or not anything passes through it.
    assert_eq!((rates[&format!("{robot}/idle/raw")], rates[&format!("{robot}/idle/smooth")]), (0.0, 0.0));

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

#[test]
fn a_rate_is_timed_by_when_the_samples_were_sent() -> triq::Result {
    // 29 samples between two readings half a second apart. Their send times
    // say sixty a second; the time between the readings would say 58.
    let before = Reading { count: 100, sent_ns: 5_000_000_000, taken_ns: 9_000_000_000 };
    let sent_ns = before.sent_ns + 29 * 1_000_000_000 / 60;
    let now = Reading { count: 129, sent_ns, taken_ns: before.taken_ns + 500_000_000 };
    assert!((29.0 / graph::seconds_between(&before, &now) - 60.0).abs() < 1e-6);
    // A runtime too old to say when samples were sent is timed by its readings.
    let old = |reading: Reading| Reading { sent_ns: 0, ..reading };
    assert_eq!(29.0 / graph::seconds_between(&old(before), &old(now)), 58.0);
    // With nothing new, the time that passed is the time between the readings.
    let idle = Reading { count: 100, taken_ns: now.taken_ns, ..before };
    assert_eq!(graph::seconds_between(&before, &idle), 0.5);

    // The rates beside the pins are worked out this way: for an output, and for each output an input hears.
    let scene = Scene::new()?;
    scene.imu.pose.send(Pose::default())?;
    assert!(wait_for(|| scene.viewer.pose.recv().expect("recv").is_some()));
    let first = scene.inventories();
    let mut second = first.clone();
    for inventory in &mut second {
        inventory.taken_ns += 500_000_000;
        for node in &mut inventory.nodes {
            for source in
                node.inputs.iter_mut().flat_map(|input| &mut input.sources).filter(|source| source.sent_ns > 0)
            {
                (source.received, source.sent_ns) = (source.received + 29, source.sent_ns + 29 * 1_000_000_000 / 60);
            }
            for output in node.outputs.iter_mut().filter(|output| output.sent_ns > 0) {
                (output.sent, output.sent_ns) = (output.sent + 29, output.sent_ns + 29 * 1_000_000_000 / 60);
            }
        }
    }
    let rates = graph::rates(&first, &second);
    let (robot, desk) = (scene.robot.name(), scene.desk.name());
    for port in [format!("{robot}/imu/pose"), format!("{robot}/smoother/raw"), format!("{desk}/viewer/pose")] {
        assert!((rates[&port] - 60.0).abs() < 1e-6, "{port}: {}", rates[&port]);
    }
    assert_eq!(rates[&format!("{desk}/viewer/image")], 0.0, "an input still waiting for its output reads zero");

    // And so are the points a chart draws.
    let mut history = triq_workbench::history::History::default();
    history.record(&first);
    history.record(&second);
    let filtered =
        triq_workbench::history::Connection { to: format!("{robot}/smoother/raw"), from: format!("{robot}/imu/pose") };
    assert!((history.latest(&filtered).expect("a point").rate - 60.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn a_card_that_comes_back_is_put_where_it_was() -> triq::Result {
    let scene = Scene::new()?;
    let (mut snarl, mut places) = (Snarl::new(), Places::new());
    graph::sync_with(&mut snarl, &scene.inventories(), &mut places, 100);
    let position =
        |snarl: &Snarl<Card>, name: &str| snarl.nodes_pos().find(|(_, card)| card.name() == name).map(|found| found.0);
    let laid_out = position(&snarl, "imu").unwrap();

    // The user moves the smoother's card.
    let put = pos2(700.0, 420.0);
    for (_, drawn) in snarl.nodes_ids_data_mut().filter(|(_, drawn)| drawn.value.name() == "smoother") {
        drawn.pos = put;
    }
    // The robot's program stops: its cards go, and where they were is remembered.
    graph::sync_with(&mut snarl, &[scene.desk.inventory()], &mut places, 200);
    assert_eq!(position(&snarl, "smoother"), None);
    let smoother = format!("{}/smoother", scene.robot.name());
    assert_eq!(places[&smoother], Place { at: [700.0, 420.0], seen: 200 });

    // It runs again. The card is where the user put it, and a card that was never moved is where it was.
    graph::sync_with(&mut snarl, &scene.inventories(), &mut places, 300);
    assert_eq!((position(&snarl, "smoother"), position(&snarl, "imu")), (Some(put), Some(laid_out)));
    assert_eq!(places[&smoother].seen, 300, "and it was seen again just now");

    // A node that was never there before is laid out as always.
    scene.robot.spawn_kind("filter", "late", triq::Placement::Dedicated, serde_json::json!(null))?;
    graph::sync_with(&mut snarl, &scene.inventories(), &mut places, 400);
    assert!(position(&snarl, "late").is_some_and(|place| place != put));
    Ok(())
}

#[test]
fn only_the_cards_seen_last_are_remembered() {
    // Programs come and go under names that are never used again; their cards are not kept for ever.
    let place = |seen| Place { at: [seen as f32, 0.0], seen };
    let places: Places =
        (0..graph::REMEMBERED as u64 + 10).map(|seen| (format!("run{seen}/imu"), place(seen))).collect();
    let kept = graph::remembered(&places);
    assert_eq!(kept.len(), graph::REMEMBERED);
    assert!(!kept.contains_key("run0/imu") && !kept.contains_key("run9/imu"), "the oldest ten are let go");
    assert!(kept.contains_key("run10/imu") && kept.contains_key(&format!("run{}/imu", graph::REMEMBERED + 9)));
    assert_eq!(graph::remembered(&kept), kept, "what fits is kept as it is");
}

#[test]
fn the_cards_that_feed_a_card_and_those_it_feeds_are_found_also_through_others() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name(), scene.desk.name());
    // A node that is wired to nothing, and a second filter that hears the sensor as the first does.
    let _spare: ImuPorts = scene.desk.attach("spare")?;
    scene.robot.spawn_kind("filter", "beside", triq::Placement::Dedicated, serde_json::json!(null))?;
    scene.robot.connect("imu/pose", "beside/raw")?;
    // The viewer and the node without wires are at the desk, the others in the robot.
    let key = |name: &str| format!("{}/{name}", if ["viewer", "spare"].contains(&name) { desk } else { robot });
    let cards = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|name| key(name)).collect() };
    let flow = |name: &str| {
        let mut snarl = Snarl::new();
        graph::sync(&mut snarl, &scene.inventories());
        graph::flow(&snarl, &key(name))
    };

    // The sensor feeds the filter and the filter the viewer: data passes through all three.
    // The second filter is not on that way: it only hears the same sensor.
    assert_eq!(flow("smoother"), cards(&["imu", "smoother", "viewer"]));
    assert_eq!(flow("viewer"), cards(&["imu", "smoother", "viewer"]));
    assert_eq!(flow("beside"), cards(&["imu", "beside"]));
    // What the sensor sends goes on to all of them.
    assert_eq!(flow("imu"), cards(&["imu", "smoother", "beside", "viewer"]));
    // A card with no wire is alone, and what is not on the drawing is nothing.
    assert_eq!(flow("spare"), cards(&["spare"]));
    assert_eq!(flow("nobody"), cards(&[]));

    // The second filter now feeds the first: it is on the way to the viewer.
    scene.robot.connect("beside/smooth", "smoother/raw")?;
    assert_eq!(flow("viewer"), cards(&["imu", "beside", "smoother", "viewer"]));
    // And the first feeds the second: wires that go round in a ring are followed once.
    scene.robot.connect("smoother/smooth", "beside/raw")?;
    assert_eq!(flow("beside"), cards(&["imu", "beside", "smoother", "viewer"]));
    Ok(())
}
