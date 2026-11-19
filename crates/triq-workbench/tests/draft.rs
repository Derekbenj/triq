//! A draft: changes that were made on the drawing and are held back, to be
//! asked for together. It holds what it takes to get from the machine to the
//! drawing and no more, in the order it was written down.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use serde_json::json;
use triq::{Direction, Inventory, NodeInfo, NodeState, Placement};
use triq_workbench::draft::{Draft, Mark};
use triq_workbench::feed::Request;

fn connect(from: &str, to: &str) -> Request {
    Request::Connect { from: from.to_owned(), to: to.to_owned() }
}

fn disconnect(from: &str, to: &str) -> Request {
    Request::Disconnect { from: from.to_owned(), to: to.to_owned() }
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

    // One of the changes is taken back.
    draft.take_back(&disconnect(&smooth, &viewer));
    assert_eq!(draft.changes(&real), [connect(&pose, &viewer)]);

    // When the machine comes to be as it was drafted by other means, there is nothing left to do.
    scene.desk.connect(&pose, "viewer/pose")?;
    assert_eq!(draft.changes(&scene.inventories()), []);
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
    assert_eq!(marks.wire(&pose, &format!("{robot}/smoother/raw")), None);
    // The ports at the ends of such wires are marked too.
    assert!(marks.port(&pose) && marks.port(&second) && marks.port(&smooth) && marks.port(&viewer));
    assert!(!marks.port(&format!("{robot}/smoother/raw")));

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
