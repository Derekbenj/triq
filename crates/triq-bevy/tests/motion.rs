//! How a controller moves the rig, and where its cameras sit. No engine involved.
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;
use triq_bevy::{Mounts, Pose, RigParams, RigState, unpadded};
use triq_xbox::XboxState;

const SECOND: Duration = Duration::from_secs(1);

fn held(change: impl FnOnce(&mut XboxState)) -> XboxState {
    let mut state = XboxState { connected: true, ..Default::default() };
    change(&mut state);
    state
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn the_left_stick_moves_the_rig_over_the_ground() {
    let params = RigParams { translation: [0.0, 1.0, 0.0], move_speed: 2.0, ..Default::default() };
    let mut rig = RigState::new(&params);
    rig.step(Some(&held(|state| state.left_x = 32767)), &params, SECOND);
    assert!(close(rig.pose().translation, [2.0, 1.0, 0.0]), "right: {:?}", rig.pose());
    // Up on the stick is a negative value and means forward, which is -z.
    rig.step(Some(&held(|state| state.left_y = -32768)), &params, SECOND);
    assert!(close(rig.pose().translation, [2.0, 1.0, -2.0]), "forward: {:?}", rig.pose());
}

#[test]
fn triggers_raise_and_lower_and_the_right_stick_turns() {
    let params = RigParams { translation: [0.0; 3], move_speed: 1.0, turn_speed: FRAC_PI_2, ..Default::default() };
    let mut rig = RigState::new(&params);
    rig.step(Some(&held(|state| state.right_trigger = 32767)), &params, SECOND);
    rig.step(Some(&held(|state| state.left_trigger = 32767)), &params, Duration::from_millis(250));
    assert!(close(rig.pose().translation, [0.0, 0.75, 0.0]));

    // A quarter turn to the left: what was forward is now to the left.
    rig.step(Some(&held(|state| state.right_x = -32768)), &params, SECOND);
    rig.step(Some(&held(|state| state.left_y = -32768)), &params, SECOND);
    assert!(close(rig.pose().translation, [-1.0, 0.75, 0.0]), "{:?}", rig.pose());
}

#[test]
fn nothing_moves_without_a_live_controller_or_inside_the_deadzone() {
    let params = RigParams::default();
    let mut rig = RigState::new(&params);
    let start = rig.pose();
    rig.step(None, &params, SECOND);
    rig.step(Some(&XboxState { left_x: 32767, ..Default::default() }), &params, SECOND); // not connected
    rig.step(Some(&held(|state| state.left_x = 3000)), &params, SECOND); // a resting stick is never exactly zero
    assert_eq!(rig.pose(), start);
}

#[test]
fn cameras_mount_on_a_row_of_points_that_turns_with_the_rig() {
    let rig = Pose { translation: [10.0, 2.0, 0.0], rotation: Pose::IDENTITY.rotation };
    let mounts = Mounts { rig, count: 2, spacing: 0.24 };
    assert!(close(mounts.point(0).unwrap().translation, [9.88, 2.0, 0.0]));
    assert!(close(mounts.point(1).unwrap().translation, [10.12, 2.0, 0.0]));
    assert_eq!(mounts.point(2), None, "there is no third point");

    // Turned a quarter to the left, the row runs along z.
    let (sin, cos) = (FRAC_PI_2 / 2.0).sin_cos();
    let turned = Mounts { rig: Pose { rotation: [0.0, sin, 0.0, cos], ..rig }, count: 3, spacing: 1.0 };
    assert!(close(turned.point(0).unwrap().translation, [10.0, 2.0, 1.0]));
    assert!(close(turned.point(1).unwrap().translation, [10.0, 2.0, 0.0]));
    assert!(close(turned.point(2).unwrap().translation, [10.0, 2.0, -1.0]));
    assert_eq!(turned.point(1).unwrap().rotation, turned.rig.rotation);
}

#[test]
fn a_frame_read_from_the_gpu_loses_its_row_padding() {
    // Two rows of three RGBA pixels, each row padded to 16 bytes.
    let padded: Vec<u8> = (0..32).collect();
    let mut pixels = [0u8; 24];
    assert!(unpadded(&padded, 3, 2, &mut pixels));
    assert_eq!(pixels[..12], padded[..12]);
    assert_eq!(pixels[12..], padded[16..28]);
}

#[test]
fn a_frame_of_another_size_is_refused() {
    // A camera that was just resized may still be handed a frame of its old size.
    let mut pixels = [7u8; 24];
    assert!(!unpadded(&[0; 16], 3, 2, &mut pixels), "rows too short for the width");
    assert!(!unpadded(&[], 3, 2, &mut pixels), "no data at all");
    assert!(!unpadded(&[0; 32], 3, 2, &mut [0; 20]), "nowhere to put it");
    assert_eq!(pixels, [7u8; 24], "and nothing is written");
}
