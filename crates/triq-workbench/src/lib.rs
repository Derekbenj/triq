//! See the runtimes on this machine as a graph, and rewire them.
//!
//! The workbench owns no nodes. It shows what `triq::discover` reports and
//! turns what the user does into requests to the runtimes that own the nodes.
#![forbid(unsafe_code)]

mod app;
pub mod charts;
pub mod colors;
pub mod draft;
pub mod feed;
pub mod graph;
pub mod history;
pub mod settings;
pub mod signals;

pub use app::Workbench;

pub use eframe;
pub use eframe::egui;
pub use triq_snarl as snarl;
