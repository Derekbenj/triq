//! The frame view, drawn without a window, with a pretend camera sending to it.
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_bevy::{FRAME_VIEW, Frame, FrameViews};

#[derive(Ports)]
struct Camera {
    image: Output<[u8], Frame>,
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Send a frame of one colour.
fn send(camera: &Camera, width: u32, height: u32, number: u64, colour: [u8; 4]) -> triq::Result {
    let pixels: Vec<u8> = colour.iter().copied().cycle().take((width * height * 4) as usize).collect();
    camera.image.send_slice_with(&pixels, Frame { width, height, number })?;
    Ok(())
}

fn shows(harness: &Harness<'_, FrameViews>, label: &str) -> bool {
    harness.query_all_by_label_contains(label).next().is_some()
}

/// Draw frames until `label` is on screen; false when two seconds pass first.
fn until_shows(harness: &mut Harness<'_, FrameViews>, label: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        harness.step();
        if shows(harness, label) || Instant::now() > deadline {
            return shows(harness, label);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Whether the window, as drawn, has a pixel of exactly this colour. `None` without a GPU to draw with.
fn drawn(harness: &mut Harness<'_, FrameViews>, colour: [u8; 4]) -> Option<bool> {
    harness.run_steps(2);
    let window = harness.render().ok()?;
    Some(window.pixels().any(|pixel| pixel.0 == colour))
}

#[test]
fn a_frame_view_draws_the_newest_frame_it_received() -> triq::Result {
    let rt = Runtime::new(&format!("fv{}", std::process::id()))?;
    let camera: Camera = rt.attach("camera")?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(520.0, 260.0))
        .wgpu()
        .build_ui_state(|ui, views: &mut FrameViews| _ = ui.horizontal(|ui| views.show(ui)), FrameViews::new(&rt)?);
    harness.step();
    assert!(harness.state().is_empty(), "there is no view until one is asked for");

    // A view is a node like any other: created from a factory, and wired by its input.
    rt.spawn_kind(FRAME_VIEW, "left", Placement::Dedicated, json!(null))?;
    rt.connect("camera/image", "left/image")?;
    assert!(until_shows(&mut harness, "no frame yet") && shows(&harness, "left"));
    let node = rt.inventory().nodes.into_iter().find(|node| node.name == "left").expect("the view's node");
    assert_eq!((node.kind.as_str(), node.inputs[0].port.message.to_string().as_str()), (FRAME_VIEW, "u8[] + Frame"));

    // The picture on screen is the picture that was sent.
    send(&camera, 4, 3, 7, RED)?;
    assert!(until_shows(&mut harness, "4 x 3, frame 7"));
    assert_ne!(drawn(&mut harness, RED), Some(false), "the frame is drawn");
    assert_ne!(drawn(&mut harness, GREEN), Some(true));

    // Of several frames that arrive between two draws, the newest is shown. Frames may change size.
    send(&camera, 4, 3, 8, RED)?;
    send(&camera, 8, 2, 9, GREEN)?;
    assert!(until_shows(&mut harness, "8 x 2, frame 9"));
    assert_ne!(drawn(&mut harness, GREEN), Some(false));
    assert_ne!(drawn(&mut harness, RED), Some(true));

    // A frame that is not what its header says is refused, and the last good one stays.
    camera.image.send_slice_with(&[0; 12], Frame { width: 4, height: 4, number: 10 })?;
    assert!(until_shows(&mut harness, "frame 10: 12 bytes is not 4 x 4 pixels"));
    assert!(shows(&harness, "8 x 2, frame 9"));
    send(&camera, 2, 2, 11, RED)?;
    assert!(until_shows(&mut harness, "2 x 2, frame 11"));
    assert!(!shows(&harness, "is not"), "the complaint goes when a good frame comes");

    // A paused view keeps what it shows.
    rt.pause("left")?;
    assert!(until_shows(&mut harness, "paused"));
    send(&camera, 2, 2, 12, GREEN)?;
    harness.run_steps(5);
    assert!(shows(&harness, "frame 11"));
    rt.resume("left")?;
    assert!(until_shows(&mut harness, "frame 12"));

    // Two views side by side, for a picture a human can look at.
    rt.spawn_kind(FRAME_VIEW, "right", Placement::Dedicated, json!(null))?;
    rt.connect("camera/image", "right/image")?;
    send(&camera, 64, 48, 13, [40, 120, 220, 255])?;
    assert!(until_shows(&mut harness, "right") && until_shows(&mut harness, "frame 13"));
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("frame-view.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    rt.remove("left")?;
    rt.remove("right")?;
    harness.run_steps(3);
    assert!(harness.state().is_empty());
    Ok(())
}
