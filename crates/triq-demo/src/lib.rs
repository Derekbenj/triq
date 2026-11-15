//! The workbench with a project's own viewers in the same window.
//!
//! The workbench knows nothing about controllers or camera frames, and the
//! two viewers know nothing about the workbench. A project puts them together
//! like this. The views are nodes of this program's own runtime, so the
//! workbench above lists and draws them like any other node: create one,
//! drag an output to its input, and what it receives is drawn underneath.
use triq::Runtime;
use triq_bevy::FrameViews;
use triq_workbench::feed::Feed;
use triq_workbench::{Workbench, eframe, egui};
use triq_xbox::ControllerViews;

pub struct Demo {
    pub workbench: Workbench,
    controllers: ControllerViews,
    frames: FrameViews,
    /// The runtime the views are nodes of. Declared last, so it goes after them.
    _runtime: Runtime,
}

impl Demo {
    /// `runtime` is this program's own; the kinds of view are offered on it.
    pub fn new(feed: Feed, runtime: Runtime) -> triq::Result<Self> {
        Ok(Self {
            workbench: Workbench::new(feed),
            controllers: ControllerViews::new(&runtime)?,
            frames: FrameViews::new(&runtime)?,
            _runtime: runtime,
        })
    }

    /// Draw one frame: the views along the bottom, the workbench in the rest.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("views").show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    self.controllers.show(ui);
                    self.frames.show(ui);
                    if self.controllers.is_empty() && self.frames.is_empty() {
                        ui.weak(
                            "Views appear here. Create a controller_view or a frame_view node, \
                             then drag a controller's state or a camera's image to its input.",
                        );
                    }
                });
            });
        });
        self.workbench.show(ui);
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.show(ui);
    }
}
