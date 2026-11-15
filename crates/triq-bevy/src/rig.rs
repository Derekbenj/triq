//! How a controller moves the rig. Plain arithmetic; no engine, no transport.
use crate::{Pose, RigParams};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;
use triq_xbox::XboxState;

const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

/// Where the rig is and which way it faces.
#[derive(Debug, Clone)]
pub struct RigState {
    translation: [f32; 3],
    yaw: f32,
    pitch: f32,
}

impl RigState {
    pub fn new(params: &RigParams) -> Self {
        Self { translation: params.translation, yaw: params.yaw, pitch: params.pitch.clamp(-MAX_PITCH, MAX_PITCH) }
    }

    /// Advance by `dt`. Without a connected controller the rig stays put.
    ///
    /// The left stick moves over the ground relative to where the rig faces,
    /// the right stick turns and tilts, and the triggers raise and lower.
    pub fn step(&mut self, controller: Option<&XboxState>, params: &RigParams, dt: Duration) {
        let Some(controller) = controller.filter(|controller| controller.connected) else { return };
        let dt = dt.as_secs_f32();
        let [left_x, left_y] = stick(controller.left_x, controller.left_y, params.deadzone);
        let [right_x, right_y] = stick(controller.right_x, controller.right_y, params.deadzone);
        let lift =
            trigger(controller.right_trigger, params.deadzone) - trigger(controller.left_trigger, params.deadzone);
        let (moved, turned) = (params.move_speed * dt, params.turn_speed * dt);
        let (sin, cos) = self.yaw.sin_cos();
        self.translation[0] += (cos * left_x + sin * left_y) * moved;
        self.translation[1] += lift * moved;
        self.translation[2] += (-sin * left_x + cos * left_y) * moved;
        self.yaw = (self.yaw - right_x * turned + PI).rem_euclid(TAU) - PI;
        self.pitch = (self.pitch - right_y * turned).clamp(-MAX_PITCH, MAX_PITCH);
    }

    pub fn pose(&self) -> Pose {
        let (sin_yaw, cos_yaw) = (self.yaw * 0.5).sin_cos();
        let (sin_pitch, cos_pitch) = (self.pitch * 0.5).sin_cos();
        Pose {
            translation: self.translation,
            rotation: [cos_yaw * sin_pitch, sin_yaw * cos_pitch, -sin_yaw * sin_pitch, cos_yaw * cos_pitch],
        }
    }
}

/// A stick as x and y from -1 to 1, with the deadzone removed and the rest rescaled.
fn stick(x: i16, y: i16, deadzone: f32) -> [f32; 2] {
    let unit = |raw: i16| if raw < 0 { f32::from(raw) / 32768.0 } else { f32::from(raw) / 32767.0 };
    let (x, y) = (unit(x), unit(y));
    let length = x.hypot(y);
    if length <= deadzone {
        return [0.0; 2];
    }
    let scale = (length.min(1.0) - deadzone) / (1.0 - deadzone) / length;
    [x * scale, y * scale]
}

fn trigger(raw: i16, deadzone: f32) -> f32 {
    let value = f32::from(raw.max(0)) / 32767.0;
    if value <= deadzone { 0.0 } else { (value - deadzone) / (1.0 - deadzone) }
}
