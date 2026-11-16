//! `triq-workbench`: run it, and it shows whatever triq runtimes are on this machine.
use triq_workbench::feed::Feed;
use triq_workbench::{Workbench, eframe, egui};

fn main() -> eframe::Result {
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
