//! A window for a project's own views of its messages.
//!
//! The workbench shows what holds for any node. What only a project can draw,
//! a controller's sticks or a camera's picture, is a **view**: a kind of node
//! with an input, and something that draws what a node of that kind last
//! received. A project writes its kinds of view and a program like this:
//!
//! ```no_run
//! # use triq::prelude::*;
//! # use triq_views::{egui, Kind, Latest};
//! # #[derive(Debug, Clone, Copy, ZeroCopySend, Fields)]
//! # #[repr(C)]
//! # struct Reading { celsius: f32 }
//! # struct FrameViews;
//! # impl FrameViews { fn new(_: &Runtime, _: &egui::Context) -> triq::Result<Self> { Ok(Self) } }
//! # impl Kind for FrameViews {
//! #     fn names(&mut self) -> Vec<String> { Vec::new() }
//! #     fn show(&mut self, _: &mut egui::Ui, _: &str) {}
//! # }
//! fn main() -> triq_views::eframe::Result {
//!     triq_views::run(Runtime::builder("views"), |views| {
//!         // A kind that some crate wrote.
//!         views.add(FrameViews::new(views.runtime(), views.window())?);
//!         // One written on the spot: the message it listens to, and how the newest is drawn.
//!         let degrees = |ui: &mut egui::Ui, reading: &Reading| _ = ui.label(format!("{:.1} degrees", reading.celsius));
//!         views.add(Latest::new(views.runtime(), "gauge", degrees)?);
//!         Ok(())
//!     })
//! }
//! ```
//!
//! That is a program with a window in which every view is a pane that the
//! user arranges. The views are nodes of its runtime, `views` here, so the
//! workbench, or any other tool, lists the kinds, creates views and wires them
//! like any other node. Nothing here is known to triq or to the workbench.
//!
//! - [`run`] is the whole program: it starts the runtime it is given, and the window, which also
//!   remembers how the views were arranged.
//! - [`Views`] is what `run` hands over to be told the kinds: the list of views and their panes.
//! - [`Kind`] is what a kind of view is to the window: the views there are, and how one is drawn.
//! - [`Latest`] is a kind made in one call, for a message small enough to be read as the window draws.
//! - [`panes`] is how panes are arranged, here and in the workbench alike.
#![forbid(unsafe_code)]

mod latest;
pub mod panes;
mod views;
#[cfg(feature = "window")]
mod window;

pub use latest::{Latest, Shown};
pub use views::{Kind, Views};
#[cfg(feature = "window")]
pub use window::run;

/// The egui that views are drawn with. A crate that defines a kind of view uses this one.
#[cfg(feature = "window")]
pub use eframe;
pub use egui;
