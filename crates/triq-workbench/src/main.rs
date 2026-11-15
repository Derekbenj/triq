//! `triq-workbench`: run it, and it shows whatever triq runtimes are on this machine.
use triq_workbench::feed::Feed;
use triq_workbench::{Workbench, eframe};

fn main() -> eframe::Result {
    eframe::run_native(
        "triq workbench",
        eframe::NativeOptions::default(),
        Box::new(|creation| {
            let window = creation.egui_ctx.clone();
            let feed = Feed::start(move || window.request_repaint())?;
            Ok(Box::new(Workbench::new(feed)))
        }),
    )
}
