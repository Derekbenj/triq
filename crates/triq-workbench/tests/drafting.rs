//! Drafting in the workbench: what the user changes is held back and shown as
//! it would be, and the runtimes are asked for it when the draft is applied.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use serde_json::json;
use triq::{NodeState, Placement, Remote};
use triq_workbench::egui::{Pos2, Shape};
use triq_workbench::feed::Request;
use triq_workbench::graph::Card;
use triq_workbench::snarl::{InPinId, OutPinId, Snarl};
use window::*;

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Press a button above the graph.
fn press(harness: &mut Bench<'_>, label: &str) {
    harness.get_by_label(label).click();
    harness.run_steps(3);
}

fn output(snarl: &Snarl<Card>, name: &str) -> OutPinId {
    let found = snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let output = (0..drawn.value.right.len()).find(|output| drawn.value.output(*output) == name)?;
        Some(OutPinId { node, output })
    });
    found.unwrap_or_else(|| panic!("no output {name}"))
}

fn input(snarl: &Snarl<Card>, name: &str) -> InPinId {
    let found = snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let input = (0..drawn.value.left.len()).find(|input| drawn.value.input(*input) == name)?;
        Some(InPinId { node, input })
    });
    found.unwrap_or_else(|| panic!("no input {name}"))
}

/// The wires drawn into the viewer's input for poses, by the output they come from.
fn drawn(harness: &Bench<'_>, viewer: &str) -> Vec<String> {
    let graph = harness.state().graph();
    let into = graph.wires().filter(|(_, to)| graph[to.node].input(to.input) == viewer);
    let mut from: Vec<String> = into.map(|(from, _)| graph[from.node].output(from.output).to_owned()).collect();
    from.sort();
    from
}

/// How many wires have a glow along them. Such a wire is drawn several times
/// over: under the wire itself are wider and fainter lines along the same way.
fn glowing(harness: &Bench<'_>) -> usize {
    fn lines(shape: &Shape, found: &mut Vec<Vec<Pos2>>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| lines(shape, found)),
            Shape::Path(path) if !path.closed => found.push(path.points.clone()),
            _ => {}
        }
    }
    let mut found = Vec::new();
    harness.output().shapes.iter().for_each(|clipped| lines(&clipped.shape, &mut found));
    let mut repeated: Vec<&Vec<Pos2>> =
        found.iter().filter(|line| found.iter().filter(|other| other == line).count() > 1).collect();
    repeated.dedup();
    repeated.len()
}

/// The outputs the viewer's input for poses really hears.
fn heard(desk: &triq::Runtime) -> Vec<String> {
    let viewer = desk.inventory().nodes.into_iter().find(|node| node.name == "viewer").expect("the viewer");
    viewer.inputs[0].sources.iter().map(|source| source.output.clone()).collect()
}

#[test]
fn what_is_changed_while_drafting_waits_until_the_draft_is_applied() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // A second filter runs beside the first, fed by the same sensor. Nothing hears it yet.
    scene.robot.spawn_kind("filter", "second", Placement::Dedicated, json!(null))?;
    scene.robot.connect("imu/pose", "second/raw")?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| shows(harness, "second")), "the cards appear");
    let (first, second) = (format!("{robot}/smoother/smooth"), format!("{robot}/second/smooth"));
    let viewer = format!("{desk}/viewer/pose");
    let cut = Request::Disconnect { from: first.clone(), to: viewer.clone() };
    let join = Request::Connect { from: second.clone(), to: viewer.clone() };

    // Begin a draft, and in it have the viewer hear the second filter instead of the first.
    assert!(harness.state().drafted().is_empty() && !shows(&harness, "Apply draft"));
    assert_eq!(glowing(&harness), 0);
    press(&mut harness, "Draft");
    assert!(shows(&harness, "Draft: no changes") && shows(&harness, "Apply draft"));
    let graph = harness.state().graph();
    let (old, new, to) = (output(graph, &first), output(graph, &second), input(graph, &viewer));
    harness.state_mut().unwire(old, to);
    harness.state_mut().wire(new, to);
    harness.run_steps(3);
    // The machine is as it was. The drawing shows both wires: the one that is to go, and the one that is to come.
    assert_eq!(heard(&scene.desk), std::slice::from_ref(&first));
    assert_eq!(drawn(&harness, &viewer), [second.clone(), first.clone()]);
    // Each of the two has a glow along it, and no other wire has.
    assert_eq!(glowing(&harness), 2);
    // What is held back is listed in the order it was done.
    assert_eq!(harness.state().drafted(), [cut.clone(), join.clone()]);
    assert!(shows(&harness, "Draft: 2 changes"));
    for line in [format!("1. disconnect {first} from {viewer}"), format!("2. connect {second} to {viewer}")] {
        assert!(shows(&harness, &line), "the details list {line:?}");
    }
    // A picture of it, for a human to look at. Drawing it needs a GPU.
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("draft.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // A wire that is drawn and taken away again leaves nothing to do.
    let sensor = output(harness.state().graph(), &format!("{robot}/imu/pose"));
    for _ in 0..3 {
        harness.state_mut().wire(sensor, to);
        harness.state_mut().unwire(sensor, to);
    }
    assert_eq!(harness.state().drafted().len(), 2);

    // Apply the draft: the runtime that owns the viewer is asked for both, and the drafting is over.
    press(&mut harness, "Apply draft");
    assert!(wait_for(|| heard(&scene.desk) == [second.clone()]), "{:?}", heard(&scene.desk));
    assert!(until(&mut harness, |harness| drawn(harness, &viewer) == [second.clone()]));
    assert!(shows(&harness, "Draft") && !shows(&harness, "Apply draft"));
    assert_eq!((harness.state().drafted(), glowing(&harness)), (Vec::new(), 0));
    Ok(())
}

#[test]
fn a_draft_can_be_discarded_and_what_is_refused_of_one_stays_a_draft() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    scene.robot.spawn_kind("filter", "second", Placement::Dedicated, json!(null))?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| shows(harness, "second")), "the cards appear");
    let state = |node: &str| {
        let nodes = scene.robot.inventory().nodes;
        nodes.into_iter().find(|info| info.name == node).map(|info| info.state)
    };
    let (first, second) = (format!("{robot}/smoother/smooth"), format!("{robot}/second/smooth"));
    let viewer = format!("{desk}/viewer/pose");

    // In a draft, a node is created, wired, and another paused and cut off. Nothing of it happens.
    press(&mut harness, "Draft");
    // The list of runtimes may be long, with other programs running, and the kind out of sight.
    harness.get_all_by_label("New filter").next().expect("the factory").scroll_to_me();
    harness.run_steps(3);
    harness.get_all_by_label("New filter").next().expect("the factory").click();
    assert!(until(&mut harness, |harness| shows(harness, "Create")));
    harness.get_by_label("Create").click();
    assert!(until(&mut harness, |harness| shows(harness, "filter1")), "the node that is to be created is drawn");
    assert!(shows(&harness, "to be created"));
    let graph = harness.state().graph();
    let (sensor, new) = (output(graph, &format!("{robot}/imu/pose")), input(graph, &format!("{robot}/filter1/raw")));
    let (old, to) = (output(graph, &first), input(graph, &viewer));
    harness.state_mut().wire(sensor, new);
    harness.state_mut().unwire(old, to);
    harness.run_steps(3);
    assert_eq!(harness.state().drafted().len(), 3);
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("draft-node.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    harness.run_steps(3);
    assert!(scene.robot.inventory().nodes.iter().all(|node| node.name != "filter1"), "nothing was created");
    assert_eq!(heard(&scene.desk), std::slice::from_ref(&first));

    // Discard it: the drawing shows the machine as it is again.
    press(&mut harness, "Discard draft");
    assert!(!shows(&harness, "filter1") && !shows(&harness, "to be created"));
    assert_eq!((harness.state().drafted(), drawn(&harness, &viewer)), (Vec::new(), vec![first.clone()]));

    // Another draft: the viewer is to hear the second filter as well, and the first is cut off.
    press(&mut harness, "Draft");
    let graph = harness.state().graph();
    let (old, new, to) = (output(graph, &first), output(graph, &second), input(graph, &viewer));
    let extra = input(graph, &format!("{robot}/second/raw"));
    harness.state_mut().wire(new, to);
    harness.state_mut().wire(sensor, extra);
    harness.state_mut().unwire(old, to);
    harness.run_steps(3);
    assert_eq!(harness.state().drafted().len(), 3);
    // Before it is applied, the second filter is removed behind the workbench's back.
    Remote::open(&robot)?.remove("second")?;
    assert_eq!(state("second"), None);
    press(&mut harness, "Apply draft");
    // The first change was made. The second was refused, there being no such node, and the third
    // was not asked for: the two are a draft again, and the workbench says what went wrong.
    assert!(wait_for(|| heard(&scene.desk) == [first.clone(), second.clone()]), "{:?}", heard(&scene.desk));
    assert!(until(&mut harness, |harness| shows(harness, "Apply draft")), "there is a draft again");
    assert_eq!(harness.state().drafted().len(), 2);
    assert!(harness.query_by_label_contains("1 of the 3 changes were made").is_some());
    assert_eq!(state("smoother"), Some(NodeState::Running));
    Ok(())
}
