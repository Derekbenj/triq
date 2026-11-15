//! A Bevy scene whose rig and cameras are triq nodes.
//!
//! This file holds what other programs need in order to talk to the scene:
//! the messages and the settings. `rig.rs` is how a controller moves the rig.
//! With the `engine` feature, `scene.rs` is the Bevy program itself. With the
//! `viewer` feature, `viewer.rs` draws camera frames in any program that has
//! an egui window.
mod rig;
#[cfg(feature = "engine")]
mod scene;
#[cfg(feature = "viewer")]
mod viewer;

pub use rig::RigState;
#[cfg(feature = "engine")]
pub use scene::{CAMERA, RIG, app};
#[cfg(feature = "viewer")]
pub use viewer::{FRAME_VIEW, FrameViewPorts, FrameViews};

use serde::{Deserialize, Serialize};
use triq::prelude::*;
use triq_xbox::XboxState;

/// Where something is: a position, and a rotation as an x, y, z, w quaternion.
#[derive(Debug, Clone, Copy, PartialEq, ZeroCopySend)]
#[repr(C)]
#[type_name("Pose")]
pub struct Pose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

impl Pose {
    pub const IDENTITY: Self = Self { translation: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0] };

    /// This pose moved by `offset`, where `offset` is measured in this pose's own axes.
    pub fn offset(&self, offset: [f32; 3]) -> Self {
        let [x, y, z, w] = self.rotation;
        // Rotate the offset by the quaternion.
        let t = [
            2.0 * (y * offset[2] - z * offset[1]),
            2.0 * (z * offset[0] - x * offset[2]),
            2.0 * (x * offset[1] - y * offset[0]),
        ];
        let turned = [
            offset[0] + w * t[0] + y * t[2] - z * t[1],
            offset[1] + w * t[1] + z * t[0] - x * t[2],
            offset[2] + w * t[2] + x * t[1] - y * t[0],
        ];
        Self { translation: std::array::from_fn(|axis| self.translation[axis] + turned[axis]), rotation: self.rotation }
    }
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A rig's pose, and the row of points on it that cameras can be mounted on.
/// The points are evenly spaced along the rig's own x axis, centred on the rig.
#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend)]
#[repr(C)]
#[type_name("Mounts")]
pub struct Mounts {
    pub rig: Pose,
    pub count: u32,
    pub spacing: f32,
}

impl Mounts {
    /// Where mount point `index` is in the world; `None` if the rig has no such point.
    pub fn point(&self, index: u32) -> Option<Pose> {
        let along = (index as f32 - (self.count.checked_sub(1)? as f32) / 2.0) * self.spacing;
        (index < self.count).then(|| self.rig.offset([along, 0.0, 0.0]))
    }
}

/// What a camera frame is, beside its pixels. The pixels are RGBA, 4 bytes
/// each, row after row; this travels with them as the message's header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend)]
#[repr(C)]
#[type_name("Frame")]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Counts the frames this camera has sent.
    pub number: u64,
}

#[derive(Ports)]
pub struct RigPorts {
    pub controller: Input<XboxState>,
    pub pose: Output<Pose>,
    /// Many cameras and viewers may listen to one rig.
    #[port(subscribers = 32)]
    pub mounts: Output<Mounts>,
}

#[derive(Ports)]
pub struct CameraPorts {
    pub mounts: Input<Mounts>,
    /// Frames are large: each listener keeps only the newest two.
    #[port(queue = 2)]
    pub image: Output<[u8], Frame>,
}

/// A rig's settings. All of them can be changed while it runs; a new starting
/// pose puts the rig there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigParams {
    pub translation: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Metres per second with a stick or trigger fully pressed.
    pub move_speed: f32,
    /// Radians per second with the right stick fully pressed.
    pub turn_speed: f32,
    /// How far a stick or trigger must move before it counts, from 0 to 1.
    pub deadzone: f32,
    /// The rig stops if it has heard nothing from its controller for this long.
    pub timeout_ms: u64,
    /// How many camera mount points the rig has, and how far apart they are.
    pub mounts: u32,
    pub spacing: f32,
}

impl Default for RigParams {
    fn default() -> Self {
        Self {
            translation: [0.0, 1.5, 6.0],
            yaw: 0.0,
            pitch: 0.0,
            move_speed: 4.0,
            turn_speed: 1.5,
            deadzone: 0.12,
            timeout_ms: 250,
            mounts: 2,
            spacing: 0.24,
        }
    }
}

/// A camera's settings. All of them can be changed while it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraParams {
    pub width: u32,
    pub height: u32,
    /// Which of the rig's mount points the camera sits on.
    pub mount: u32,
    pub fov_degrees: f32,
    /// Frames sent per second, at most.
    pub hz: f32,
}

impl Default for CameraParams {
    fn default() -> Self {
        Self { width: 320, height: 240, mount: 0, fov_degrees: 60.0, hz: 30.0 }
    }
}

/// Copy a frame as the GPU returns it, with every row padded to a fixed
/// stride, into `pixels` without the padding. False, with nothing copied,
/// when `padded` is not a frame of this size.
pub fn unpadded(padded: &[u8], width: u32, height: u32, pixels: &mut [u8]) -> bool {
    let row = width as usize * 4;
    let stride = padded.len() / height.max(1) as usize;
    if row == 0 || stride < row || pixels.len() != row * height as usize {
        return false;
    }
    for (source, target) in padded.chunks(stride).zip(pixels.chunks_mut(row)) {
        target.copy_from_slice(&source[..row]);
    }
    true
}
