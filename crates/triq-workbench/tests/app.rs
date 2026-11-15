//! The whole workbench, drawn without a window and driven like a user would:
//! look at the machine, create a node from a factory, wire it, pause it, remove it.
#[path = "support/scene.rs"]
mod scene;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq::{NodeInfo, NodeState};
use triq_workbench::feed::Feed;
use triq_workbench::graph::Card;
use triq_workbench::snarl::{InPinId, OutPinId, Snarl};
use triq_workbench::{Workbench, egui};

/// Draw frames until `done` holds.
fn until(harness: &mut Harness<'_, Workbench>, mut done: impl FnMut(&Harness<'_, Workbench>) -> bool) -> bool {
    wait_for(|| {
        harness.step();
        done(harness)
    })
}

fn shows(harness: &Harness<'_, Workbench>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

fn click(harness: &mut Harness<'_, Workbench>, label: &str) {
    harness.get_all_by_label(label).next().unwrap_or_else(|| panic!("nothing labelled {label:?}")).click();
    harness.step();
}

/// The wires that end in one of `runtimes`. The workbench draws the whole
/// machine, and other triq programs may be running beside this test.
fn wires(harness: &Harness<'_, Workbench>, runtimes: [&str; 2]) -> usize {
    let graph = harness.state().graph();
    graph.wires().filter(|(_, to)| runtimes.contains(&graph[to.node].runtime.as_str())).count()
}

fn node(runtime: &triq::Runtime, name: &str) -> Option<NodeInfo> {
    runtime.inventory().nodes.into_iter().find(|node| node.name == name)
}

fn output(snarl: &Snarl<Card>, name: &str) -> OutPinId {
    let found = snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let output = drawn.value.node.outputs.iter().position(|output| output.service == name)?;
        Some(OutPinId { node, output })
    });
    found.unwrap_or_else(|| panic!("no output {name}"))
}

fn input(snarl: &Snarl<Card>, name: &str) -> InPinId {
    let found = snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let input = (0..drawn.value.node.inputs.len()).find(|input| drawn.value.input(*input) == name)?;
        Some(InPinId { node, input })
    });
    found.unwrap_or_else(|| panic!("no input {name}"))
}

#[test]
fn the_workbench_shows_the_machine_and_changes_it_on_request() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .wgpu()
        .build_ui_state(|ui, bench: &mut Workbench| bench.show(ui), Workbench::new(Feed::start(|| {})?));

    // It shows what is there without being told where to look.
    assert!(until(&mut harness, |harness| shows(harness, &robot) && shows(harness, &desk)), "runtimes are listed");
    for label in ["imu", "smoother", "viewer", "New filter", "raw: Pose", "smooth: Pose", "image: u8[]"] {
        assert!(shows(&harness, label), "{label:?} is on screen");
    }
    assert!(shows(&harness, "waiting"), "a connection without its output says so");
    assert_eq!(wires(&harness, [&robot, &desk]), 2);

    // Where data flows, the pins say how fast.
    assert!(harness.query_all_by_label_contains("/s").next().is_none(), "nothing is flowing yet");
    let flowing = until(&mut harness, |harness| {
        scene.imu.pose.send(Pose::default()).expect("send");
        harness.query_all_by_label_contains("/s").count() >= 3
    });
    assert!(flowing, "the sensor's output, and the filter's input and output, show a rate");

    // Create a node from the factory, through the form.
    click(&mut harness, "New filter");
    assert!(until(&mut harness, |harness| shows(harness, "Create")));
    click(&mut harness, "Create");
    assert!(wait_for(|| node(&scene.robot, "filter1").is_some()), "the runtime that owns the factory created it");
    assert_eq!(
        node(&scene.robot, "filter1").unwrap().params,
        serde_json::json!({ "alpha": 0.25 }),
        "with the defaults"
    );
    assert!(until(&mut harness, |harness| shows(harness, "filter1")), "and it appears on the graph");

    // Draw a wire, as dragging from an output pin to an input pin does.
    let (from, to) = (format!("{robot}/imu/pose"), format!("{robot}/filter1/raw"));
    let (out_pin, in_pin) = (output(harness.state().graph(), &from), input(harness.state().graph(), &to));
    harness.state_mut().wire(out_pin, in_pin);
    assert!(wait_for(|| node(&scene.robot, "filter1").unwrap().inputs[0].sources.len() == 1), "the owner connected it");
    assert!(until(&mut harness, |harness| wires(harness, [&robot, &desk]) == 3), "and the wire appears");

    // The new node was placed before it was wired; tidying moves it right of what feeds it.
    let x = |harness: &Harness<'_, Workbench>, name: &str| {
        harness.state().graph().nodes_pos().find(|(_, card)| card.node.name == name).unwrap().0.x
    };
    assert!(x(&harness, "filter1") <= x(&harness, "imu"));
    click(&mut harness, "Tidy");
    assert!(x(&harness, "imu") < x(&harness, "filter1"));

    // A wire between different message types is refused before anything is asked.
    let image = input(harness.state().graph(), &format!("{desk}/viewer/image"));
    harness.state_mut().wire(out_pin, image);
    assert!(until(&mut harness, |harness| harness.query_by_label_contains("carry different messages").is_some()));

    // Select the node: pause it, then remove the wire and the node.
    click(&mut harness, "filter1");
    assert!(until(&mut harness, |harness| shows(harness, "Pause")));
    click(&mut harness, "Pause");
    assert!(wait_for(|| node(&scene.robot, "filter1").unwrap().state == NodeState::Paused));
    assert!(until(&mut harness, |harness| shows(harness, "Resume")));

    harness.state_mut().unwire(out_pin, in_pin);
    assert!(wait_for(|| node(&scene.robot, "filter1").unwrap().inputs[0].sources.is_empty()));

    // A picture of the state so far, for a human to look at. Drawing it needs a GPU;
    // without one the rest of the test still stands.
    harness.run_steps(3);
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("workbench.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    click(&mut harness, "Remove");
    assert!(wait_for(|| node(&scene.robot, "filter1").is_none()));
    assert!(until(&mut harness, |harness| !shows(harness, "filter1")), "and it leaves the graph");
    Ok(())
}
