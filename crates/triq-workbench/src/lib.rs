//! See the runtimes on this machine as a graph, and rewire them.
//!
//! The workbench owns no nodes. It shows what `triq::discover` reports and
//! turns what the user does into requests to the runtimes that own the nodes.
mod app;
pub mod colors;
pub mod feed;
pub mod graph;

pub use app::Workbench;

pub use eframe;
pub use eframe::egui;
pub use egui_snarl as snarl;
