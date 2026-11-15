//! The controller view, drawn without a window, with a pretend controller sending to it.
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_xbox::{CONTROLLER_VIEW, ControllerViews, XboxState, buttons};

#[derive(Ports)]
struct Pad {
    state: Output<XboxState>,
}

fn shows(harness: &Harness<'_, ControllerViews>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Draw frames until `done` holds; false when two seconds pass first.
fn until(harness: &mut Harness<'_, ControllerViews>, done: impl Fn(&Harness<'_, ControllerViews>) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        harness.step();
        if done(harness) || Instant::now() > deadline {
            return done(harness);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn held(buttons: u32) -> XboxState {
    XboxState { connected: true, buttons, ..Default::default() }
}

#[test]
fn a_controller_view_draws_the_newest_state_it_received() -> triq::Result {
    let rt = Runtime::new(&format!("xv{}", std::process::id()))?;
    let pad: Pad = rt.attach("pad")?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 200.0))
        .wgpu()
        .build_ui_state(|ui, views: &mut ControllerViews| views.show(ui), ControllerViews::new(&rt)?);
    harness.step();
    assert!(harness.state().is_empty(), "there is no view until one is asked for");

    // A view is a node like any other: created from a factory, and wired by its input.
    rt.spawn_kind(CONTROLLER_VIEW, "watch", Placement::Dedicated, json!(null))?;
    rt.connect("pad/state", "watch/state")?;
    assert!(until(&mut harness, |harness| shows(harness, "watch") && shows(harness, "nothing received yet")));
    let node = rt.inventory().nodes.into_iter().find(|node| node.name == "watch").expect("the view's node");
    assert_eq!((node.kind.as_str(), node.inputs[0].port.message.to_string().as_str()), (CONTROLLER_VIEW, "XboxState"));

    let state =
        XboxState { left_x: 20_000, left_y: -30_000, right_trigger: 32_767, ..held(buttons::A | buttons::LEFT_BUMPER) };
    pad.state.send(state)?;
    assert!(until(&mut harness, |harness| shows(harness, "A LB")), "the buttons that are down are named");
    assert!(shows(&harness, "connected"));
    // A picture for a human to look at. Drawing it needs a GPU; without one the rest still stands.
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("controller-view.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // Of several states that arrive between two frames, the newest is drawn.
    for buttons in [buttons::B, buttons::X, buttons::Y] {
        pad.state.send(held(buttons))?;
    }
    assert!(until(&mut harness, |harness| shows(harness, "Y")));
    pad.state.send(held(0))?;
    assert!(until(&mut harness, |harness| shows(harness, "no buttons")));

    // A controller that went away sends one all-zero state.
    pad.state.send(XboxState::default())?;
    assert!(until(&mut harness, |harness| shows(harness, "no controller")));

    // A paused view keeps what it shows.
    rt.pause("watch")?;
    assert!(until(&mut harness, |harness| shows(harness, "paused")));
    pad.state.send(held(buttons::START))?;
    harness.run_steps(5);
    assert!(!shows(&harness, "Start"));
    rt.resume("watch")?;
    assert!(until(&mut harness, |harness| shows(harness, "Start")));

    rt.remove("watch")?;
    assert!(until(&mut harness, |harness| !shows(harness, "watch")));
    assert!(harness.state().is_empty());
    Ok(())
}
