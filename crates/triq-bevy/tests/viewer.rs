//! The frame view, drawn without a window, with a pretend camera sending to it.
//! The view is a node the runtime runs; the window draws what it left.
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_bevy::{FRAME_VIEW, Frame, FrameViews, thumbnail};

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
    let rt = Runtime::builder(&format!("fv{}", std::process::id())).pool("views", 2).start()?;
    let camera: Camera = rt.attach("camera")?;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(520.0, 260.0))
        .wgpu()
        .build_ui_state(|ui, views: &mut FrameViews| _ = ui.horizontal(|ui| views.show(ui)), FrameViews::new(&rt)?);
    harness.step();
    assert!(harness.state().is_empty(), "there is no view until one is asked for");

    // A view is a node like any other: created from a factory, placed on a pool, and wired by its input.
    let view = |rt: &Runtime| rt.inventory().nodes.into_iter().find(|node| node.name == "left").expect("its node");
    assert!(!rt.inventory().factories[0].local, "the runtime runs the views, not the window");
    rt.spawn_kind(FRAME_VIEW, "left", Placement::pool("views"), json!(null))?;
    rt.connect("camera/image", "left/image")?;
    assert!(until_shows(&mut harness, "no frame yet") && shows(&harness, "left"));
    let node = view(&rt);
    assert_eq!((node.kind.as_str(), node.inputs[0].port.message.to_string().as_str()), (FRAME_VIEW, "u8[] + Frame"));
    assert_eq!(node.placement, Some(Placement::pool("views")));

    // The picture on screen is the picture that was sent.
    send(&camera, 4, 3, 7, RED)?;
    assert!(until_shows(&mut harness, "4 x 3, frame 7"));
    assert!(view(&rt).runs > 0, "the frame was taken on the pool's thread");
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
    send(&camera, 2, 2, 12, GREEN)?;
    std::thread::sleep(Duration::from_millis(100));
    harness.run_steps(5);
    assert!(shows(&harness, "frame 11"));
    rt.resume("left")?;
    assert!(until_shows(&mut harness, "frame 12"));

    // A large frame is drawn like a small one, from fewer pixels than were sent.
    send(&camera, 1920, 1080, 13, RED)?;
    assert!(until_shows(&mut harness, "1920 x 1080, frame 13"));
    assert_ne!(drawn(&mut harness, RED), Some(false));

    // Two views side by side, for a picture a human can look at.
    rt.spawn_kind(FRAME_VIEW, "right", Placement::Dedicated, json!(null))?;
    rt.connect("camera/image", "right/image")?;
    send(&camera, 64, 48, 14, [40, 120, 220, 255])?;
    assert!(until_shows(&mut harness, "right") && until_shows(&mut harness, "64 x 48, frame 14"));
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("frame-view.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    rt.remove("left")?;
    rt.remove("right")?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !harness.state().is_empty() && Instant::now() < deadline {
        harness.step();
    }
    assert!(harness.state().is_empty(), "a view that is removed leaves the window");
    Ok(())
}

#[test]
fn a_large_picture_is_scaled_down_by_averaging() {
    // Four by four pixels: the left half white, the right half black on top and red below.
    let (white, black, red) = ([255; 4], [0, 0, 0, 255], RED);
    let rows = [
        [white, white, black, black],
        [white, white, black, black],
        [white, white, red, red],
        [white, white, red, red],
    ];
    let pixels: Vec<u8> = rows.iter().flatten().flatten().copied().collect();

    // Small enough already, it is taken as it is.
    let whole = thumbnail(&pixels, 4, 4, 4);
    assert_eq!((whole.size, whole.pixels[2].to_array(), whole.pixels[10].to_array()), ([4, 4], black, red));
    // To two rows, each pixel is the average of a square of four.
    let half = thumbnail(&pixels, 4, 4, 2);
    assert_eq!(half.size, [2, 2]);
    let colours: Vec<[u8; 4]> = half.pixels.iter().map(|pixel| pixel.to_array()).collect();
    assert_eq!(colours, [white, black, white, red]);
    let mixed = thumbnail(&pixels, 4, 4, 1);
    assert_eq!((mixed.size, mixed.pixels[0].to_array()), ([1, 1], [191, 127, 127, 255]));

    // A camera's frame comes down to what a window can draw quickly, and keeps its shape.
    let frame = vec![200u8; 1920 * 1080 * 4];
    assert_eq!(thumbnail(&frame, 1920, 1080, 360).size, [640, 360]);
    // One far wider than it is tall is held to four times as wide as the rows allowed.
    assert_eq!(thumbnail(&vec![0u8; 4000 * 10 * 4], 4000, 10, 100).size, [400, 1]);
}
