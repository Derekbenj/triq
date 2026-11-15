//! A node that shows camera frames, for any program drawn with egui.
//!
//! This is all a viewer is: a kind of node with an input, run by the program
//! that has the window, and a function that draws what its nodes last
//! received. triq does not know it is a viewer, and neither does the workbench.
use crate::Frame;
use egui::{ColorImage, Image, TextureHandle, TextureOptions, Ui, vec2};
use std::collections::BTreeMap;
use std::time::Duration;
use triq::prelude::*;
use triq::{Local, LocalFactory};

/// The kind of node this offers.
pub const FRAME_VIEW: &str = "frame_view";

/// Frames arrive whether or not anything else on screen changes, so a
/// window with a view in it is redrawn this often.
const REFRESH: Duration = Duration::from_millis(16);

/// How tall a picture is drawn, in points.
const HEIGHT: f32 = 180.0;

#[derive(Ports)]
pub struct FrameViewPorts {
    pub image: Input<[u8], Frame>,
}

/// Every frame view of one program. Make one, and call [`FrameViews::show`]
/// where the pictures should be drawn.
pub struct FrameViews {
    requests: LocalFactory<FrameViewPorts, ()>,
    views: BTreeMap<String, View>,
}

struct View {
    ports: FrameViewPorts,
    paused: bool,
    /// The newest good frame, as a texture the window can draw.
    picture: Option<(Frame, TextureHandle)>,
    /// Why the newest frame could not be drawn, if it could not.
    problem: Option<String>,
}

impl FrameViews {
    /// Offer `frame_view` nodes on `runtime`.
    pub fn new(runtime: &Runtime) -> triq::Result<Self> {
        Ok(Self { requests: runtime.local_factory(FRAME_VIEW)?, views: BTreeMap::new() })
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
                    self.views.insert(name, View { ports, paused: false, picture: None, problem: None });
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
        // A paused view reads nothing and keeps what it shows.
        if !self.paused {
            self.take_newest(ui, name);
        }
        if let Some(problem) = &self.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        let Some((frame, texture)) = &self.picture else {
            ui.weak("no frame yet");
            return;
        };
        let paused = if self.paused { ", paused" } else { "" };
        ui.weak(format!("{} x {}, frame {}{paused}", frame.width, frame.height, frame.number));
        let size = vec2(HEIGHT * frame.width as f32 / frame.height as f32, HEIGHT);
        ui.add(Image::new(texture).fit_to_exact_size(size));
    }

    /// Only the newest frame is worth drawing. Its pixels are read where they
    /// lie in shared memory and copied once, into a texture for the window.
    fn take_newest(&mut self, ui: &Ui, name: &str) {
        let sample = match self.ports.image.latest() {
            Ok(Some(sample)) => sample,
            Ok(None) => return,
            Err(error) => return self.problem = Some(error.to_string()),
        };
        let frame = *sample.header();
        let (width, height, number) = (frame.width as usize, frame.height as usize, frame.number);
        let largest = ui.ctx().input(|input| input.max_texture_side);
        // The header is another program's word for what the bytes are. Check it before trusting it.
        self.problem = if sample.is_empty() || width as u128 * height as u128 * 4 != sample.len() as u128 {
            Some(format!("frame {number}: {} bytes is not {width} x {height} pixels", sample.len()))
        } else if width > largest || height > largest {
            Some(format!("frame {number}: {width} x {height} is too large to draw"))
        } else {
            None
        };
        if self.problem.is_some() {
            return;
        }
        let image = ColorImage::from_rgba_unmultiplied([width, height], &sample);
        match &mut self.picture {
            Some((shown, texture)) => {
                *shown = frame;
                texture.set(image, TextureOptions::LINEAR);
            }
            None => self.picture = Some((frame, ui.ctx().load_texture(name, image, TextureOptions::LINEAR))),
        }
    }
}
