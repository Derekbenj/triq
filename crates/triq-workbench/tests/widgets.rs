//! The graph and the charts as two widgets, for a window that arranges them its own way.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq_workbench::Workbench;
use triq_workbench::egui;
use triq_workbench::feed::Feed;
use window::*;

#[test]
fn the_graph_and_the_charts_are_placed_where_a_window_wants_them() -> triq::Result {
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    // A window of a project's own: the charts along the bottom, the graph in the rest.
    let mut harness = Harness::builder().with_size(egui::vec2(1500.0, 1000.0)).wgpu().build_ui_state(
        |ui, bench: &mut Workbench| {
            egui::Panel::bottom("charts").default_size(400.0).show(ui, |ui| bench.show_charts(ui));
            bench.show_graph(ui);
        },
        Workbench::new(Feed::start(|| {})?),
    );
    let both = until(&mut harness, |harness| {
        harness.query_by_label("raw: Pose").is_some() && harness.query_by_label(&filtered).is_some()
    });
    assert!(both, "the cards and the list of numbers are there at once");
    harness.run_steps(4);
    for label in ["Tidy", "Fit", "Chart 1", "Drag a number here."] {
        assert!(harness.query_by_label(label).is_some(), "{label:?} is on screen");
    }
    // Each brings what belongs to it, and neither brings the tabs: the window decides what is where.
    assert!(harness.query_by_label("Graph").is_none() && harness.query_by_label("Charts").is_none());
    let (card, chart) = (seen(&harness, &text(&harness, "raw: Pose")), harness.get_by_label("Drag a number here."));
    assert!(card.y < chart.rect().top(), "the graph is above the charts");
    Ok(())
}
