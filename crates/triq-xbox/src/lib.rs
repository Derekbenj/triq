//! Xbox-style controllers as triq nodes.
//!
//! [`XboxState`] is the message. [`Host`] is the body of a program that
//! watches for controllers, offers a factory of controller nodes, and
//! publishes what each node's controller is doing. With the `sdl` feature,
//! `SdlPads` reads real controllers; the `triq-xbox` program puts the two together.
//! With the `viewer` feature, [`ControllerViews`] draws controller states in
//! any program that has an egui window.
mod host;
#[cfg(feature = "sdl")]
mod sdl;
#[cfg(feature = "viewer")]
mod viewer;

pub use host::{ControllerParams, ControllerPorts, Device, FACTORY, Host, Pads};
#[cfg(feature = "sdl")]
pub use sdl::SdlPads;
#[cfg(feature = "viewer")]
pub use viewer::{CONTROLLER_VIEW, ControllerViewPorts, ControllerViews};

use triq::prelude::*;

/// Bits of [`XboxState::buttons`]. Several may be set at once.
pub mod buttons {
    pub const A: u32 = 1 << 0;
    pub const B: u32 = 1 << 1;
    pub const X: u32 = 1 << 2;
    pub const Y: u32 = 1 << 3;
    pub const BACK: u32 = 1 << 4;
    pub const START: u32 = 1 << 5;
    pub const GUIDE: u32 = 1 << 6;
    pub const LEFT_STICK: u32 = 1 << 7;
    pub const RIGHT_STICK: u32 = 1 << 8;
    pub const LEFT_BUMPER: u32 = 1 << 9;
    pub const RIGHT_BUMPER: u32 = 1 << 10;
    pub const DPAD_UP: u32 = 1 << 11;
    pub const DPAD_DOWN: u32 = 1 << 12;
    pub const DPAD_LEFT: u32 = 1 << 13;
    pub const DPAD_RIGHT: u32 = 1 << 14;
    pub const SHARE: u32 = 1 << 15;

    pub(crate) const NAMES: [(u32, &str); 16] = [
        (A, "A"),
        (B, "B"),
        (X, "X"),
        (Y, "Y"),
        (BACK, "Back"),
        (START, "Start"),
        (GUIDE, "Xbox"),
        (LEFT_STICK, "L3"),
        (RIGHT_STICK, "R3"),
        (LEFT_BUMPER, "LB"),
        (RIGHT_BUMPER, "RB"),
        (DPAD_UP, "D-pad Up"),
        (DPAD_DOWN, "D-pad Down"),
        (DPAD_LEFT, "D-pad Left"),
        (DPAD_RIGHT, "D-pad Right"),
        (SHARE, "Share"),
    ];
}

/// Everything a controller is doing at one moment: a whole snapshot, not an event.
///
/// Sticks are raw values, -32768 to 32767, with positive Y pointing down.
/// Triggers run from 0 to 32767. A state with `connected` false is all zeros:
/// it tells listeners that the controller is gone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend)]
#[repr(C)]
#[type_name("XboxState")]
pub struct XboxState {
    pub buttons: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: i16,
    pub right_trigger: i16,
    pub connected: bool,
}

impl XboxState {
    /// Whether every button in `mask` is down.
    pub fn pressed(&self, mask: u32) -> bool {
        self.buttons & mask == mask
    }

    /// The names of the buttons that are down.
    pub fn pressed_names(&self) -> Vec<&'static str> {
        buttons::NAMES.iter().filter(|(mask, _)| self.pressed(*mask)).map(|(_, name)| *name).collect()
    }
}
