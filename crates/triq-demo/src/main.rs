//! `triq-demo [runtime name]`: the workbench, with controller and camera views under it.
use triq::Runtime;
use triq_demo::Demo;
use triq_workbench::feed::Feed;
use triq_workbench::{eframe, egui};

fn main() -> eframe::Result {
    let name = std::env::args().nth(1).unwrap_or_else(|| "views".into());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 950.0]),
        ..Default::default()
    };
    eframe::run_native(
        "triq demo",
        options,
        Box::new(move |creation| {
            let window = creation.egui_ctx.clone();
            let feed = Feed::start(move || window.request_repaint())?;
            Ok(Box::new(Demo::new(feed, Runtime::new(&name)?)?))
        }),
    )
}
