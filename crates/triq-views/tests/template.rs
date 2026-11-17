//! The template that ships as an example, run as a test: it is built with
//! every test run, and does what it says it does.
#[path = "support/station.rs"]
mod station;
#[allow(dead_code)]
#[path = "../examples/template.rs"]
mod template;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use station::{unique, wait_for};
use triq::prelude::*;
use triq_views::Views;
use triq_views::egui::{self, accesskit::Role};

/// What the gauge says, if there is one and it has something to say.
fn reading(harness: &Harness<'_, Option<Views>>) -> Option<String> {
    harness.query_all_by_label_contains("°C").next().and_then(|reading| reading.value())
}

#[test]
fn the_template_shows_a_gauge_of_its_thermometer_at_once() -> triq::Result {
    // The views are made as `triq_views::run` makes them for the template's `main`, and told its kinds.
    let name = unique("template");
    let mut harness = Harness::builder().with_size(egui::vec2(700.0, 400.0)).wgpu().build_ui_state(
        |ui, state: &mut Option<Views>| {
            let made = || {
                let mut views = Views::new(Runtime::new(&name).expect("a runtime"), ui.ctx());
                template::kinds(&mut views).expect("the kinds of view");
                views
            };
            state.get_or_insert_with(made).show(ui);
        },
        None,
    );

    // Nobody has to create or wire anything: there is a gauge, in a pane, and it shows a temperature.
    let shown = wait_for(|| {
        harness.step();
        reading(&harness).is_some()
    });
    assert!(shown, "the gauge shows a reading");
    assert_eq!(harness.query_all_by_role_and_label(Role::Button, "gauge1").count(), 2, "listed, and on its bar");
    // The thermometer goes on sending, and the gauge follows it.
    let first = reading(&harness);
    let follows = wait_for(|| {
        harness.step();
        reading(&harness) != first
    });
    assert!(follows, "the reading changes: {first:?}");

    // To any tool it is a runtime like another: a kind that can be created, and two nodes with a wire between them.
    let inventory = harness.state().as_ref().expect("the views").runtime().inventory();
    let kinds: Vec<&str> = inventory.factories.iter().map(|factory| factory.kind.as_str()).collect();
    let nodes: Vec<&str> = inventory.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!((kinds, nodes), (vec!["gauge"], vec!["gauge1", "thermometer"]));
    let gauge = inventory.nodes.iter().find(|node| node.name == "gauge1").expect("the gauge");
    let source = &gauge.inputs[0].sources[0];
    assert!(source.connected && source.output == format!("{name}/thermometer/reading"), "{source:?}");
    Ok(())
}
