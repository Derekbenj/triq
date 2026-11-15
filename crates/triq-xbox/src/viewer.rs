//! A node that shows what a controller is doing, for any program drawn with egui.
//!
//! This is all a viewer is: a kind of node with an input, run by the program
//! that has the window, and a function that draws what its nodes last
//! received. triq does not know it is a viewer, and neither does the workbench.
use crate::XboxState;
use egui::{ProgressBar, Sense, Stroke, Ui, vec2};
use std::collections::BTreeMap;
use std::time::Duration;
use triq::prelude::*;
use triq::{Local, LocalFactory};

/// The kind of node this offers.
pub const CONTROLLER_VIEW: &str = "controller_view";

/// States arrive whether or not anything else on screen changes, so a
/// window with a view in it is redrawn this often.
const REFRESH: Duration = Duration::from_millis(16);

/// How wide a stick is drawn, in points.
const STICK: f32 = 56.0;

#[derive(Ports)]
pub struct ControllerViewPorts {
    pub state: Input<XboxState>,
}

/// Every controller view of one program. Make one, and call
/// [`ControllerViews::show`] where the controllers should be drawn.
pub struct ControllerViews {
    requests: LocalFactory<ControllerViewPorts, ()>,
    views: BTreeMap<String, View>,
}

struct View {
    ports: ControllerViewPorts,
    paused: bool,
    /// The newest state received, once there is one.
    state: Option<XboxState>,
}

impl ControllerViews {
    /// Offer `controller_view` nodes on `runtime`.
    pub fn new(runtime: &Runtime) -> triq::Result<Self> {
        Ok(Self { requests: runtime.local_factory(CONTROLLER_VIEW)?, views: BTreeMap::new() })
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Draw every view, one after the other in the direction `ui` lays things out.
    /// Call it once per frame.
    pub fn show(&mut self, ui: &mut Ui) {
        self.serve();
        for (name, view) in &mut self.views {
            ui.vertical(|ui| view.show(ui, name));
        }
        if !self.views.is_empty() {
            ui.ctx().request_repaint_after(REFRESH);
        }
    }

    /// Do what other programs asked for since the last frame.
    fn serve(&mut self) {
        while let Some(request) = self.requests.try_next() {
            match request {
                Local::Spawn { name, ports, .. } => {
                    self.views.insert(name, View { ports, paused: false, state: None });
                }
                Local::Pause { name } => self.set_paused(&name, true),
                Local::Resume { name } => self.set_paused(&name, false),
                // Dropping the view drops its ports, which is what ends the node.
                Local::Remove { name } => _ = self.views.remove(&name),
                Local::Params { .. } => {}
            }
        }
    }

    fn set_paused(&mut self, name: &str, paused: bool) {
        if let Some(view) = self.views.get_mut(name) {
            view.paused = paused;
        }
    }
}

impl View {
    fn show(&mut self, ui: &mut Ui, name: &str) {
        ui.strong(name);
        // Only the newest state is worth drawing. A paused view reads nothing and keeps what it shows.
        match (!self.paused).then(|| self.ports.state.latest()) {
            Some(Ok(Some(state))) => self.state = Some(*state),
            Some(Err(error)) => _ = ui.colored_label(ui.visuals().error_fg_color, error.to_string()),
            _ => {}
        }
        let Some(state) = self.state else {
            ui.weak("nothing received yet");
            return;
        };
        ui.weak(match (self.paused, state.connected) {
            (true, _) => "paused",
            (false, true) => "connected",
            (false, false) => "no controller",
        });
        ui.horizontal(|ui| {
            stick(ui, state.left_x, state.left_y);
            stick(ui, state.right_x, state.right_y);
            ui.vertical(|ui| {
                trigger(ui, "LT", state.left_trigger);
                trigger(ui, "RT", state.right_trigger);
            });
        });
        let pressed = state.pressed_names();
        ui.label(if pressed.is_empty() { "no buttons".to_owned() } else { pressed.join(" ") });
    }
}

/// A stick as a dot in a circle. Up on the stick is up on the screen.
fn stick(ui: &mut Ui, x: i16, y: i16) {
    let (response, painter) = ui.allocate_painter(vec2(STICK, STICK), Sense::hover());
    let (centre, reach) = (response.rect.center(), STICK / 2.0 - 5.0);
    painter.circle_stroke(centre, reach, Stroke::new(1.0, ui.visuals().weak_text_color()));
    let pushed = vec2(f32::from(x), f32::from(y)) / 32768.0 * reach;
    painter.circle_filled(centre + pushed, 4.0, ui.visuals().strong_text_color());
}

/// A trigger as a bar that fills as it is pulled.
fn trigger(ui: &mut Ui, name: &str, pulled: i16) {
    ui.add(ProgressBar::new(f32::from(pulled.max(0)) / 32767.0).desired_width(70.0).text(name));
}
