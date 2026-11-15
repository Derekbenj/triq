//! The whole window, drawn without one: the workbench creates and wires the
//! views like any other node, and what they receive is drawn under the graph.
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq::{NodeInfo, Remote};
use triq_bevy::{FRAME_VIEW, Frame};
use triq_demo::Demo;
use triq_workbench::egui;
use triq_workbench::feed::Feed;
use triq_workbench::graph::Card;
use triq_workbench::snarl::{InPinId, OutPinId, Snarl};
use triq_xbox::{CONTROLLER_VIEW, XboxState, buttons};

/// A pretend controller and camera, in a runtime of their own.
#[derive(Ports)]
struct Props {
    state: Output<XboxState>,
    image: Output<[u8], Frame>,
}

const RED: [u8; 4] = [255, 0, 0, 255];

impl Props {
    /// One controller state and one red frame.
    fn perform(&self, number: u64) {
        let pixels: Vec<u8> = RED.iter().copied().cycle().take(4 * 3 * 4).collect();
        self.image.send_slice_with(&pixels, Frame { width: 4, height: 3, number }).expect("send a frame");
        self.state
            .send(XboxState { connected: true, buttons: buttons::A, ..Default::default() })
            .expect("send a state");
    }
}

fn shows(harness: &Harness<'_, Demo>, label: &str) -> bool {
    harness.query_all_by_label_contains(label).next().is_some()
}

/// Draw frames until `done` holds; false when five seconds pass first.
fn until(harness: &mut Harness<'_, Demo>, mut done: impl FnMut(&Harness<'_, Demo>) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        harness.step();
        if done(harness) || Instant::now() > deadline {
            return done(harness);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn output(snarl: &Snarl<Card>, name: &str) -> Option<OutPinId> {
    snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let output = drawn.value.node.outputs.iter().position(|output| output.service == name)?;
        Some(OutPinId { node, output })
    })
}

fn input(snarl: &Snarl<Card>, name: &str) -> Option<InPinId> {
    snarl.nodes_ids_data().find_map(|(node, drawn)| {
        let input = (0..drawn.value.node.inputs.len()).find(|input| drawn.value.input(*input) == name)?;
        Some(InPinId { node, input })
    })
}

/// Ask for a connection the way dragging a wire in the workbench does.
fn drag(harness: &mut Harness<'_, Demo>, from: &str, to: &str) {
    let graph = harness.state().workbench.graph();
    let (from, to) = (output(graph, from).expect("the output's pin"), input(graph, to).expect("the input's pin"));
    harness.state_mut().workbench.wire(from, to);
}

fn node(remote: &Remote, name: &str) -> NodeInfo {
    remote.inventory().expect("inventory").nodes.into_iter().find(|node| node.name == name).expect("the node")
}

#[test]
fn views_are_nodes_the_workbench_wires_and_the_window_draws() -> triq::Result {
    let (stage, views) = (format!("stage{}", std::process::id()), format!("views{}", std::process::id()));
    let stage_runtime = Runtime::new(&stage)?;
    let props: Props = stage_runtime.attach("props")?;
    let demo = Demo::new(Feed::start(|| {})?, Runtime::new(&views)?)?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .wgpu()
        .build_ui_state(|ui, demo: &mut Demo| demo.show(ui), demo);

    // The workbench offers the two kinds of view like any other kind of node.
    assert!(until(&mut harness, |harness| shows(harness, "New controller_view") && shows(harness, "New frame_view")));
    assert!(shows(&harness, "Views appear here"), "and says where they will be drawn");

    // Any program may create them. The window draws each one as soon as it exists.
    let remote = Remote::open(&views)?;
    remote.spawn(FRAME_VIEW, "picture", Placement::Dedicated, json!(null))?;
    remote.spawn(CONTROLLER_VIEW, "sticks", Placement::Dedicated, json!(null))?;
    assert!(until(&mut harness, |harness| shows(harness, "no frame yet") && shows(harness, "nothing received yet")));
    assert!(!shows(&harness, "Views appear here"));

    // Wire the pretend camera and controller to them, as dragging in the workbench does.
    assert!(until(&mut harness, |harness| {
        let graph = harness.state().workbench.graph();
        output(graph, &format!("{stage}/props/image")).is_some()
            && input(graph, &format!("{views}/picture/image")).is_some()
    }));
    drag(&mut harness, &format!("{stage}/props/image"), &format!("{views}/picture/image"));
    drag(&mut harness, &format!("{stage}/props/state"), &format!("{views}/sticks/state"));
    assert!(until(&mut harness, |_| {
        node(&remote, "picture").inputs[0].sources.iter().any(|source| source.connected)
            && node(&remote, "sticks").inputs[0].sources.iter().any(|source| source.connected)
    }));

    // What is sent now is drawn under the graph, and the graph says how fast it flows.
    let mut number = 0;
    let flowing = until(&mut harness, |harness| {
        number += 1;
        props.perform(number);
        shows(harness, "4 x 3, frame") && shows(harness, "connected") && shows(harness, "/s")
    });
    assert!(flowing, "the frame, the controller and their rates are on screen");
    harness.run_steps(2);
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("demo.png");
    match harness.render() {
        Ok(window) => {
            assert!(window.pixels().any(|pixel| pixel.0 == RED), "the camera's red frame is in the window");
            window.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?;
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // The workbench refuses a wire between a controller and a frame view: they carry different messages.
    drag(&mut harness, &format!("{stage}/props/state"), &format!("{views}/picture/image"));
    assert!(until(&mut harness, |harness| shows(harness, "carry different messages")));

    // Removed from anywhere, a view leaves the window.
    remote.remove("picture")?;
    remote.remove("sticks")?;
    assert!(until(&mut harness, |harness| shows(harness, "Views appear here")));
    Ok(())
}
