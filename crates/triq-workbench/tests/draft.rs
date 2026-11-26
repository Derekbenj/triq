//! A draft: changes that were made on the drawing and are held back, to be
//! asked for together. It holds what it takes to get from the machine to the
//! drawing and no more, in the order it was written down.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use serde_json::json;
use triq::{Direction, Group, Inventory, NodeInfo, NodeState, Placement};
use triq_workbench::draft::{Draft, Mark};
use triq_workbench::feed::Request;
use triq_workbench::graph::{Groups, groups};

fn connect(from: &str, to: &str) -> Request {
    Request::Connect { from: from.to_owned(), to: to.to_owned() }
}

fn disconnect(from: &str, to: &str) -> Request {
    Request::Disconnect { from: from.to_owned(), to: to.to_owned() }
}

fn group(runtime: &str, node: &str, group: &str) -> Request {
    Request::SetGroup { runtime: runtime.to_owned(), node: node.to_owned(), group: group.to_owned() }
}

fn node<'a>(machine: &'a [Inventory], runtime: &str, name: &str) -> Option<&'a NodeInfo> {
    let inventory = machine.iter().find(|inventory| inventory.runtime == runtime)?;
    inventory.nodes.iter().find(|node| node.name == name)
}

/// The outputs that the first input of a node hears.
fn heard(machine: &[Inventory], runtime: &str, name: &str) -> Vec<String> {
    let node = node(machine, runtime, name).expect("the node");
    node.inputs[0].sources.iter().map(|source| source.output.clone()).collect()
}

#[test]
fn a_draft_holds_what_it_takes_to_get_from_the_machine_to_the_drawing_and_no_more() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let real = scene.inventories();
    let (pose, smooth) = (format!("{robot}/imu/pose"), format!("{robot}/smoother/smooth"));
    let viewer = format!("{desk}/viewer/pose");
    let mut draft = Draft::default();
    assert!(draft.changes(&real).is_empty());

    // A wire that is not on the machine is drawn and taken away, twenty times over: there is nothing to do.
    for _ in 0..20 {
        draft.record(connect(&pose, &viewer), &real);
        draft.record(disconnect(&pose, &viewer), &real);
    }
    assert_eq!(draft.changes(&real), []);
    // Drawn once more, it is one change.
    draft.record(connect(&pose, &viewer), &real);
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer)]);

    // A wire that is on the machine is cut, and drawn again: for that one there is nothing to do either.
    draft.record(disconnect(&smooth, &viewer), &real);
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer), disconnect(&smooth, &viewer)]);
    draft.record(connect(&smooth, &viewer), &real);
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer)]);

    // Changes are kept in the order they were written down. One that is written again is the last.
    draft.record(disconnect(&smooth, &viewer), &real);
    draft.record(disconnect(&pose, &viewer), &real);
    draft.record(connect(&pose, &viewer), &real);
    assert_eq!(draft.changes(&real), [disconnect(&smooth, &viewer), connect(&pose, &viewer)]);

    // The same goes for whether a node runs, and for its settings: a change back is no change.
    let smoother = || (robot.clone(), "smoother".to_owned());
    let (runtime, node) = smoother();
    draft.record(Request::Pause { runtime, node }, &real);
    assert_eq!(draft.changes(&real).len(), 3);
    let (runtime, node) = smoother();
    draft.record(Request::Resume { runtime, node }, &real);
    let (runtime, node) = smoother();
    draft.record(Request::SetParams { runtime, node, params: json!({ "alpha": 0.9 }) }, &real);
    let (runtime, node) = smoother();
    draft.record(Request::SetParams { runtime, node, params: json!({ "alpha": 0.5 }) }, &real);
    assert_eq!(draft.changes(&real), [disconnect(&smooth, &viewer), connect(&pose, &viewer)]);

    // Some of a node's settings may be changed and the rest left. The node is shown with the
    // change made; a later change to its settings takes the place of an earlier one, whichever
    // sort each is; and a change to what it already has is none.
    let change = |changes: serde_json::Value| {
        let (runtime, node) = smoother();
        Request::ChangeParams { runtime, node, changes }
    };
    let shown = |draft: &Draft| self::node(&draft.over(&real), &robot, "smoother").unwrap().params.clone();
    draft.record(change(json!({ "alpha": 0.9 })), &real);
    assert_eq!(draft.changes(&real).last(), Some(&change(json!({ "alpha": 0.9 }))));
    assert_eq!(shown(&draft), json!({ "alpha": 0.9 }));
    assert_eq!(draft.marks(&real).card(&format!("{robot}/smoother")), Some(Mark::Changed));
    draft.record(change(json!({ "alpha": 0.7 })), &real);
    assert_eq!((draft.changes(&real).len(), shown(&draft)), (3, json!({ "alpha": 0.7 })));
    let (runtime, node) = smoother();
    draft.record(Request::SetParams { runtime, node, params: json!({ "alpha": 0.8 }) }, &real);
    assert_eq!((draft.changes(&real).len(), shown(&draft)), (3, json!({ "alpha": 0.8 })));
    draft.record(change(json!({ "alpha": 0.5 })), &real);
    assert_eq!(draft.changes(&real), [disconnect(&smooth, &viewer), connect(&pose, &viewer)]);
    draft.record(change(json!({})), &real);
    assert_eq!(draft.changes(&real).len(), 2, "no setting named, nothing to do");
    assert_eq!(
        change(json!({ "alpha": 0.9 })).to_string(),
        format!("change {{\"alpha\":0.9}} in the settings of {robot}/smoother")
    );
    let (runtime, node) = smoother();
    let all = Request::SetParams { runtime, node, params: json!({ "alpha": 0.9 }) };
    assert_eq!(all.to_string(), format!("replace the settings of {robot}/smoother"));

    // One of the changes is taken back.
    draft.take_back(&disconnect(&smooth, &viewer), &real);
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer)]);

    // When the machine comes to be as it was drafted by other means, there is nothing left to do.
    scene.desk.connect(&pose, "viewer/pose")?;
    assert_eq!(draft.changes(&scene.inventories()), []);
    Ok(())
}

#[test]
fn a_change_of_some_settings_is_drawn_with_the_others_as_they_are() -> triq::Result {
    // A node with two settings, of which a draft changes one: it is drawn with the other as
    // the machine has it, and the change is no longer needed once the machine has that one.
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    let mut real = scene.inventories();
    let has = |real: &mut [Inventory], settings: serde_json::Value| {
        let home = real.iter_mut().find(|inventory| inventory.runtime == robot).unwrap();
        home.nodes.iter_mut().find(|node| node.name == "smoother").unwrap().params = settings;
    };
    has(&mut real, json!({ "alpha": 0.5, "beta": 2 }));
    let mut draft = Draft::default();
    let change =
        Request::ChangeParams { runtime: robot.clone(), node: "smoother".to_owned(), changes: json!({ "alpha": 0.9 }) };
    draft.record(change.clone(), &real);
    let drawn = draft.over(&real);
    assert_eq!(node(&drawn, &robot, "smoother").unwrap().params, json!({ "alpha": 0.9, "beta": 2 }));
    // Something else changes the other setting: the change is still to make, and leaves it be.
    has(&mut real, json!({ "alpha": 0.5, "beta": 7 }));
    assert_eq!(draft.changes(&real), [change]);
    assert_eq!(node(&draft.over(&real), &robot, "smoother").unwrap().params, json!({ "alpha": 0.9, "beta": 7 }));
    has(&mut real, json!({ "alpha": 0.9, "beta": 7 }));
    assert_eq!(draft.changes(&real), []);
    Ok(())
}

#[test]
fn the_machine_is_shown_as_it_would_be_with_the_changes_made() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let real = scene.inventories();
    let (pose, smooth) = (format!("{robot}/imu/pose"), format!("{robot}/smoother/smooth"));
    let (viewer, second) = (format!("{desk}/viewer/pose"), format!("{robot}/second/raw"));
    let mut draft = Draft::default();
    assert_eq!(draft.over(&real), real, "with nothing drafted, as it is");

    // A second filter beside the first, fed by the sensor; the viewer hears the second instead of the first.
    let on = |node: &str| (robot.clone(), node.to_owned());
    let (runtime, name) = on("second");
    let (kind, placement) = ("filter".to_owned(), Placement::Dedicated);
    draft.record(Request::Spawn { runtime, kind, name, placement, params: json!({ "alpha": 0.1 }) }, &real);
    draft.record(connect(&pose, &second), &real);
    draft.record(disconnect(&smooth, &viewer), &real);
    draft.record(connect(&format!("{robot}/second/smooth"), &viewer), &real);
    // The first filter is paused, and given other settings. The sensor is to go.
    let (runtime, node) = on("smoother");
    draft.record(Request::Pause { runtime, node }, &real);
    let (runtime, node) = on("smoother");
    draft.record(Request::SetParams { runtime, node, params: json!({ "alpha": 0.9 }) }, &real);
    let (runtime, node) = on("imu");
    draft.record(Request::Remove { runtime, node }, &real);
    assert_eq!(draft.changes(&real).len(), 7);

    let drafted = draft.over(&real);
    // The node that is to be created is there, with the ports its factory says such a node has.
    let ghost = self::node(&drafted, &robot, "second").expect("the node that was drafted");
    assert_eq!((ghost.kind.as_str(), &ghost.params), ("filter", &json!({ "alpha": 0.1 })));
    let ports = |node: &NodeInfo| {
        let inputs = node.inputs.iter().map(|input| &input.port);
        let outputs = node.outputs.iter().map(|output| &output.port);
        inputs.chain(outputs).map(|port| (port.name.clone(), port.direction)).collect::<Vec<_>>()
    };
    assert_eq!(ports(ghost), [("raw".to_owned(), Direction::Input), ("smooth".to_owned(), Direction::Output)]);
    assert_eq!(ghost.outputs[0].service, format!("{robot}/second/smooth"));
    // Wires that were drawn are there. A wire that was cut is too, until it really is: it is shown as going.
    assert_eq!(heard(&drafted, &robot, "second"), std::slice::from_ref(&pose));
    assert_eq!(heard(&drafted, &desk, "viewer"), [smooth.clone(), format!("{robot}/second/smooth")]);
    // The first filter is as it would be.
    let first = self::node(&drafted, &robot, "smoother").expect("the first filter");
    assert_eq!((&first.state, &first.params), (&NodeState::Paused, &json!({ "alpha": 0.9 })));
    assert!(self::node(&drafted, &robot, "imu").is_some(), "a node that is to go is shown until it has gone");

    // What the draft does to each card and wire, for the drawing to tell them apart by.
    let marks = draft.marks(&real);
    let card = |node: &str| marks.card(&format!("{robot}/{node}"));
    assert_eq!(
        [card("second"), card("smoother"), card("imu")],
        [Some(Mark::New), Some(Mark::Changed), Some(Mark::Going)]
    );
    assert_eq!(marks.card(&format!("{desk}/viewer")), None);
    assert_eq!(marks.wire(&pose, &second), Some(Mark::New));
    assert_eq!(marks.wire(&smooth, &viewer), Some(Mark::Going));
    // A node that is to go takes its wires with it: so the one from the sensor to the first filter is going.
    let raw = format!("{robot}/smoother/raw");
    assert_eq!(marks.wire(&pose, &raw), Some(Mark::Going));
    assert_eq!(marks.wire(&format!("{robot}/camera/image"), &format!("{desk}/viewer/image")), None);
    // The ports at the ends of such wires are marked too.
    assert!(marks.port(&pose) && marks.port(&second) && marks.port(&smooth) && marks.port(&viewer) && marks.port(&raw));
    assert!(!marks.port(&format!("{desk}/viewer/image")));

    // The machine itself is as it was.
    assert_eq!(heard(&scene.inventories(), &desk, "viewer"), [smooth]);
    Ok(())
}

#[test]
fn a_node_that_was_only_drafted_is_taken_back_with_all_that_was_drafted_for_it() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    let real = scene.inventories();
    let (pose, second) = (format!("{robot}/imu/pose"), format!("{robot}/second/raw"));
    let viewer = format!("{}/viewer/pose", scene.desk.name());
    let mut draft = Draft::default();

    let spawn = |name: &str| Request::Spawn {
        runtime: robot.clone(),
        kind: "filter".to_owned(),
        name: name.to_owned(),
        placement: Placement::Dedicated,
        params: json!(null),
    };
    draft.record(spawn("second"), &real);
    draft.record(connect(&pose, &second), &real);
    draft.record(connect(&format!("{robot}/second/smooth"), &viewer), &real);
    draft.record(connect(&pose, &viewer), &real);
    assert_eq!(draft.changes(&real).len(), 4);

    // Removing it is not a request to any runtime: there is no such node. Its wires go with it.
    draft.record(Request::Remove { runtime: robot.clone(), node: "second".to_owned() }, &real);
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer)]);
    assert!(node(&draft.over(&real), &robot, "second").is_none());

    // A kind of node that no factory offers cannot be drawn, and stays a change all the same:
    // the runtime that is asked for it will say what it thinks of it.
    let Request::Spawn { runtime, name, placement, params, .. } = spawn("third") else { unreachable!() };
    draft.record(Request::Spawn { runtime, kind: "nope".to_owned(), name, placement, params }, &real);
    assert_eq!(draft.changes(&real).len(), 2);
    assert!(node(&draft.over(&real), &robot, "third").is_none());
    Ok(())
}

#[test]
fn which_group_a_node_is_in_is_drafted_with_the_rest() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    scene.robot.set_group("imu", "sensing")?;
    scene.robot.set_group("smoother", "sensing")?;
    let real = scene.inventories();
    let key = |runtime: &str, node: &str| format!("{runtime}/{node}");
    let grouped = |pairs: &[(String, &str)]| -> Groups {
        pairs.iter().map(|(node, group)| (node.clone(), (*group).to_owned())).collect()
    };
    let mut draft = Draft::default();
    assert_eq!(
        groups(&draft.over(&real)),
        grouped(&[(key(&robot, "imu"), "sensing"), (key(&robot, "smoother"), "sensing")])
    );

    // Two nodes are put in a group that is new, and one is taken out of the group it is in.
    draft.record(group(&desk, "viewer", "looking"), &real);
    draft.record(group(&robot, "smoother", "looking"), &real);
    draft.record(group(&robot, "imu", ""), &real);
    assert_eq!(draft.changes(&real).len(), 3);
    let drafted = groups(&draft.over(&real));
    assert_eq!(drafted, grouped(&[(key(&desk, "viewer"), "looking"), (key(&robot, "smoother"), "looking")]));
    // The machine itself is as it was.
    assert_eq!(groups(&scene.inventories()).len(), 2);

    // The drawing tells them apart: the group that is new, the one that loses its nodes, and the nodes that move.
    let marks = draft.marks(&real);
    assert_eq!([marks.card("/looking"), marks.card("/sensing")], [Some(Mark::New), Some(Mark::Changed)]);
    let card = |runtime: &str, node: &str| marks.card(&key(runtime, node));
    assert_eq!([card(&robot, "imu"), card(&desk, "viewer")], [Some(Mark::Changed), Some(Mark::Changed)]);

    // A node that is put back where it is leaves nothing to do, and a change can be taken back.
    draft.record(group(&robot, "imu", "sensing"), &real);
    draft.take_back(&group(&desk, "viewer", "looking"), &real);
    assert_eq!(draft.changes(&real), [group(&robot, "smoother", "looking")]);
    // When the machine comes to be as it was drafted by other means, there is nothing left to do.
    scene.robot.set_group("smoother", "looking")?;
    assert_eq!(draft.changes(&scene.inventories()), []);
    Ok(())
}

#[test]
fn a_group_that_a_runtime_offers_as_a_kind_is_drafted_whole() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let pose = format!("{robot}/imu/pose");
    // Two filters, one after the other, fed by the sensor, which is not of the group.
    let pair = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .spawn("filter", "fine", Placement::pool("dsp"), json!({ "alpha": 0.1 }))
        .connect(&pose, "coarse/raw")
        .connect("coarse/smooth", "fine/raw");
    scene.robot.register_group("pair", pair)?;
    let real = scene.inventories();
    let spawn = Request::Spawn {
        runtime: robot.clone(),
        kind: "pair".to_owned(),
        name: "p".to_owned(),
        placement: Placement::Dedicated,
        params: json!(null),
    };
    let mut draft = Draft::default();
    draft.record(spawn.clone(), &real);
    assert_eq!(draft.changes(&real), std::slice::from_ref(&spawn), "the whole group is one change");

    // Its nodes are drawn as they will be: named after the group, in it, with their settings, and wired.
    let drafted = draft.over(&real);
    let coarse = node(&drafted, &robot, "p.coarse").expect("the first node of the group");
    let fine = node(&drafted, &robot, "p.fine").expect("the second");
    assert_eq!(
        (coarse.kind.as_str(), coarse.group.as_str(), &coarse.params),
        ("filter", "p", &json!({ "alpha": 0.9 }))
    );
    assert_eq!((fine.group.as_str(), &fine.placement), ("p", &Some(Placement::pool("dsp"))));
    assert_eq!(heard(&drafted, &robot, "p.coarse"), std::slice::from_ref(&pose));
    assert_eq!(heard(&drafted, &robot, "p.fine"), [format!("{robot}/p.coarse/smooth")]);
    // The group, its nodes and its wires are all new.
    let marks = draft.marks(&real);
    let cards = ["/p".to_owned(), format!("{robot}/p.coarse"), format!("{robot}/p.fine")];
    assert_eq!(cards.map(|card| marks.card(&card)), [Some(Mark::New); 3]);
    assert_eq!(marks.wire(&format!("{robot}/p.coarse/smooth"), &format!("{robot}/p.fine/raw")), Some(Mark::New));
    assert_eq!(marks.wire(&pose, &format!("{robot}/p.coarse/raw")), Some(Mark::New));

    // More is drafted for its nodes: other settings for one, a wire from it, and both inside another group.
    let fine = || (robot.clone(), "p.fine".to_owned());
    let (runtime, node) = fine();
    draft.record(Request::SetParams { runtime, node, params: json!({ "alpha": 0.2 }) }, &real);
    draft.record(connect(&format!("{robot}/p.fine/smooth"), &format!("{desk}/viewer/pose")), &real);
    draft.record(group(&robot, "p.coarse", "outer/p"), &real);
    draft.record(group(&robot, "p.fine", "outer/p"), &real);
    assert_eq!(draft.changes(&real).len(), 5);
    let drafted = draft.over(&real);
    let fine = self::node(&drafted, &robot, "p.fine").expect("the second");
    assert_eq!((fine.group.as_str(), &fine.params), ("outer/p", &json!({ "alpha": 0.2 })));
    assert_eq!(draft.marks(&real).card("/outer"), Some(Mark::New));

    // Taken back, the group goes with all that was drafted for its nodes.
    draft.take_back(&spawn, &real);
    assert_eq!(draft.changes(&real), []);

    // And when such a group is there, creating it is nothing left to do.
    draft.record(spawn.clone(), &real);
    scene.robot.spawn_kind("pair", "p", Placement::Dedicated, json!(null))?;
    assert_eq!(draft.changes(&scene.inventories()), []);
    Ok(())
}

#[test]
fn a_kind_with_a_group_inside_it_is_drafted_with_that_group_inside() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    // A filter, and after it another that is in a group inside the group.
    let nest = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .group("deep", Group::new().spawn("filter", "fine", Placement::Dedicated, json!({ "alpha": 0.1 })))
        .connect("coarse/smooth", "deep.fine/raw");
    scene.robot.register_group("nest", nest)?;
    let real = scene.inventories();
    let spawn = Request::Spawn {
        runtime: robot.clone(),
        kind: "nest".to_owned(),
        name: "n".to_owned(),
        placement: Placement::Dedicated,
        params: json!(null),
    };
    let mut draft = Draft::default();
    draft.record(spawn, &real);

    // Its nodes are drawn where they will be: one in the group, the other in the group inside it.
    let drafted = draft.over(&real);
    let group = |name: &str| node(&drafted, &robot, name).map(|node| node.group.as_str());
    assert_eq!([group("n.coarse"), group("n.deep.fine")], [Some("n"), Some("n/deep")]);
    assert_eq!(heard(&drafted, &robot, "n.deep.fine"), [format!("{robot}/n.coarse/smooth")]);
    // Both groups are new.
    let marks = draft.marks(&real);
    assert_eq!(["/n", "/n/deep"].map(|card| marks.card(card)), [Some(Mark::New); 2]);
    Ok(())
}
