//! A kind of view made in one call, drawn without a window, with a pretend station sending to it.
#[path = "support/station.rs"]
mod station;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use station::*;
use triq::prelude::*;
use triq_views::{Kind, Latest, egui};

type Window<'a> = Harness<'a, Latest<Reading>>;

fn shows(harness: &Window<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Draw frames until `label` is on screen.
fn until_shows(harness: &mut Window<'_>, label: &str) -> bool {
    wait_for(|| {
        harness.step();
        shows(harness, label)
    })
}

#[test]
fn a_view_of_a_small_message_is_one_call() -> triq::Result {
    let rt = Runtime::new(&unique("latest"))?;
    let station: Station = rt.attach("station")?;
    // A window as a program might draw it: every view under its name, one after the other.
    let mut harness = Harness::builder().with_size(egui::vec2(420.0, 200.0)).wgpu().build_ui_state(
        |ui, gauges: &mut Latest<Reading>| {
            for name in gauges.names() {
                ui.strong(&name);
                gauges.show(ui, &name);
            }
        },
        gauges(&rt)?,
    );
    harness.step();
    assert!(harness.state_mut().names().is_empty(), "there is no view until one is asked for");

    // The call offered the kind on the runtime, for any program to see: a node with one input,
    // for the message the view draws, which the window runs itself.
    let offered = rt.inventory().factories;
    let (kind, port) = (&offered[0], &offered[0].ports[0]);
    assert_eq!((kind.kind.as_str(), kind.local, offered.len()), ("gauge", true, 1));
    assert_eq!((port.name.as_str(), port.message.to_string().as_str()), ("input", "Reading"));

    // A view is a node like any other: created from the factory, and wired by its input.
    rt.spawn_kind("gauge", "attic", Placement::Dedicated, json!(null))?;
    rt.connect("station/reading", "attic/input")?;
    assert!(until_shows(&mut harness, "attic") && shows(&harness, "nothing received yet"));

    // What was written to draw a reading is all that draws it.
    station.reading.send(Reading { celsius: 21.5 })?;
    assert!(until_shows(&mut harness, "21.5 degrees"));
    // Of several messages that arrive between two frames, the newest is drawn.
    for celsius in [22.0, 23.0, 24.5] {
        station.reading.send(Reading { celsius })?;
    }
    assert!(until_shows(&mut harness, "24.5 degrees") && !shows(&harness, "21.5 degrees"));

    // A paused view keeps what it shows, and says so.
    rt.pause("attic")?;
    assert!(until_shows(&mut harness, "paused"));
    station.reading.send(Reading { celsius: 30.0 })?;
    harness.run_steps(5);
    assert!(shows(&harness, "24.5 degrees"));
    rt.resume("attic")?;
    assert!(until_shows(&mut harness, "30.0 degrees") && !shows(&harness, "paused"));

    rt.remove("attic")?;
    assert!(wait_for(|| {
        harness.step();
        !shows(&harness, "attic")
    }));
    assert!(harness.state_mut().names().is_empty());
    Ok(())
}
