//! Real controllers, through SDL3.
//!
//! SDL's event pump belongs to the thread that created it, which on macOS
//! must be the program's main thread. That is why the controller nodes are
//! run by the application and not by a triq thread.
use crate::{Pads, XboxState, buttons};
use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::joystick::JoystickId;
use std::collections::BTreeMap;

pub struct SdlPads {
    // Dropped in this order: controllers and the event pump before SDL itself.
    open: BTreeMap<u32, Gamepad>,
    events: sdl3::EventPump,
    gamepads: sdl3::GamepadSubsystem,
    _sdl: sdl3::Sdl,
}

impl SdlPads {
    /// Call this on the program's main thread.
    pub fn new() -> triq::Result<Self> {
        // The program has no window, so it must hear controllers in the background.
        sdl3::hint::set(sdl3::hint::names::JOYSTICK_ALLOW_BACKGROUND_EVENTS, "1");
        let sdl = sdl3::init()?;
        let gamepads = sdl.gamepad()?;
        let events = sdl.event_pump()?;
        Ok(Self { open: BTreeMap::new(), events, gamepads, _sdl: sdl })
    }

    /// Let SDL notice controllers coming and going. False when SDL asks the program to quit.
    pub fn pump(&mut self) -> bool {
        !self.events.poll_iter().any(|event| matches!(event, Event::Quit { .. }))
    }
}

impl Pads for SdlPads {
    fn devices(&mut self) -> Vec<(u32, String)> {
        let mut devices: Vec<(u32, String)> = self
            .gamepads
            .gamepads()
            .unwrap_or_default()
            .into_iter()
            .map(|id| (id.raw(), self.gamepads.name_for_id(id).unwrap_or_else(|_| format!("Gamepad {}", id.raw()))))
            .collect();
        devices.sort();
        devices
    }

    fn open(&mut self, id: u32) -> triq::Result {
        if !self.devices().iter().any(|(plugged, _)| *plugged == id) {
            return Err(triq::Error::Unknown(format!("controller {id}: it is not plugged in")));
        }
        self.open.insert(id, self.gamepads.open(JoystickId::new(id))?);
        Ok(())
    }

    fn close(&mut self, id: u32) {
        self.open.remove(&id);
    }

    fn read(&mut self, id: u32) -> Option<XboxState> {
        let pad = self.open.get(&id).filter(|pad| pad.connected())?;
        const BUTTONS: [(Button, u32); 16] = [
            (Button::South, buttons::A),
            (Button::East, buttons::B),
            (Button::West, buttons::X),
            (Button::North, buttons::Y),
            (Button::Back, buttons::BACK),
            (Button::Start, buttons::START),
            (Button::Guide, buttons::GUIDE),
            (Button::LeftStick, buttons::LEFT_STICK),
            (Button::RightStick, buttons::RIGHT_STICK),
            (Button::LeftShoulder, buttons::LEFT_BUMPER),
            (Button::RightShoulder, buttons::RIGHT_BUMPER),
            (Button::DPadUp, buttons::DPAD_UP),
            (Button::DPadDown, buttons::DPAD_DOWN),
            (Button::DPadLeft, buttons::DPAD_LEFT),
            (Button::DPadRight, buttons::DPAD_RIGHT),
            (Button::Misc1, buttons::SHARE),
        ];
        Some(XboxState {
            buttons: BUTTONS.iter().filter(|(button, _)| pad.button(*button)).map(|(_, bit)| bit).sum(),
            left_x: pad.axis(Axis::LeftX),
            left_y: pad.axis(Axis::LeftY),
            right_x: pad.axis(Axis::RightX),
            right_y: pad.axis(Axis::RightY),
            left_trigger: pad.axis(Axis::TriggerLeft),
            right_trigger: pad.axis(Axis::TriggerRight),
            connected: true,
        })
    }
}
