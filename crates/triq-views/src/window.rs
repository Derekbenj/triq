//! The whole of a program whose window is its views.
use crate::Views;
use eframe::egui;

/// What the arrangement of the views is kept under, between one run of the program and the next.
const LAYOUT: &str = "views";

/// Run a program whose window is its views. `runtime` is the runtime the
/// views will be nodes of, as the program wants it: its name, which is also
/// the window's, and the pools it has for views whose nodes do work. `kinds` is
/// called once, when the window is there, to add the kinds of view the
/// program has.
///
/// The window remembers its size and place, and how the views were arranged:
/// a view is where it was as soon as a node of its name exists again.
pub fn run(runtime: triq::Builder, kinds: impl FnOnce(&mut Views) -> triq::Result + 'static) -> eframe::Result {
    // Started before there is a window, which is called by its name.
    let runtime = runtime.start().map_err(|error| eframe::Error::AppCreation(error.into()))?;
    let viewport = egui::ViewportBuilder::default().with_title(runtime.name()).with_inner_size([1100.0, 800.0]);
    eframe::run_native(
        // What is kept between runs is kept under this.
        &format!("triq-views-{}", runtime.name()),
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(move |creation| {
            let mut views = Views::new(runtime, &creation.egui_ctx);
            kinds(&mut views)?;
            if let Some(layout) = creation.storage.and_then(|kept| kept.get_string(LAYOUT)) {
                views.arrange(&layout);
            }
            Ok(Box::new(Window(views)))
        }),
    )
}

struct Window(Views);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.0.show(ui);
    }

    /// Called now and then, and when the window closes.
    fn save(&mut self, kept: &mut dyn eframe::Storage) {
        if let Some(layout) = self.0.layout() {
            kept.set_string(LAYOUT, layout);
        }
    }
}
