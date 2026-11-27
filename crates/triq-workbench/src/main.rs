//! `triq-workbench`: run it, and it shows whatever triq runtimes are on this machine.
use triq_workbench::feed::Feed;
use triq_workbench::{Workbench, eframe, egui};

/// What this program takes: nothing.
const TAKES: &str = "\
usage: triq-workbench
  the window that draws every runtime on this machine: its nodes, their settings and
  their wires, and charts of what they send. It takes nothing: what it shows it finds.";

fn main() -> eframe::Result {
    triq::help(TAKES);
    // Room for four windows side by side and one under another.
    let viewport = egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]);
    eframe::run_native(
        "triq workbench",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(|creation| {
            let window = creation.egui_ctx.clone();
            let feed = Feed::start(move || window.request_repaint())?;
            // As it was when it last closed: its windows, its charts, and what was on them.
            Ok(Box::new(Workbench::new(feed).remembered(creation.storage)))
        }),
    )
}
