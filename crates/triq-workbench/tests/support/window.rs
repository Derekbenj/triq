//! The workbench drawn without a window, for the tests that handle it as a user
//! would. Included with `#[path]`, after `scene`.
//!
//! The workbench draws the whole machine, and other triq programs may be
//! running beside a test. So a test looks for its own nodes by name, and at
//! what is drawn next to them, never at "all the buttons that say Pause".
#![allow(dead_code)]
use super::scene::wait_for;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use std::sync::{Mutex, MutexGuard, PoisonError};
use triq_workbench::Workbench;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::emath::TSTransform;
use triq_workbench::egui::{self, PointerButton};
use triq_workbench::feed::Feed;

pub type Bench<'a> = Harness<'a, Workbench>;

/// Held by a test that drags numbers out of the list on the Charts tab, from
/// before it makes its scene. The list holds every topic on the machine, so
/// the scene of another test, coming or going, would move its rows under the pointer.
pub fn alone() -> MutexGuard<'static, ()> {
    static LIST: Mutex<()> = Mutex::new(());
    LIST.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn open() -> triq::Result<Bench<'static>> {
    Ok(Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .wgpu()
        .build_ui_state(|ui, bench: &mut Workbench| bench.show(ui), Workbench::new(Feed::start(|| {})?)))
}

/// Draw frames until `done` holds.
pub fn until(harness: &mut Bench<'_>, mut done: impl FnMut(&Bench<'_>) -> bool) -> bool {
    wait_for(|| {
        harness.step();
        done(harness)
    })
}

/// How the graph's own coordinates map to the screen. The graph is drawn on a
/// layer of its own, which is what the user moves and zooms.
pub fn view(harness: &Bench<'_>) -> TSTransform {
    let layers: Vec<_> = harness.ctx.memory(|memory| memory.layer_ids().collect());
    layers.into_iter().find_map(|layer| harness.ctx.layer_transform_to_global(layer)).expect("the graph's layer")
}

/// A piece of text on a card. The lists beside the graph may hold the same
/// words; the graph is drawn after them.
pub fn text<'a>(harness: &'a Bench<'_>, label: &'a str) -> Node<'a> {
    let found = harness.query_all_by_role_and_label(Role::Label, label).last();
    found.unwrap_or_else(|| panic!("no card says {label:?}"))
}

/// The part of a card that holds a piece of text: for the name of a node its
/// header, with the dot and the pause button; for a port its row, with the rate.
pub fn around<'a>(harness: &'a Bench<'_>, label: &'a str) -> Node<'a> {
    text(harness, label).parent().expect("text is drawn inside something")
}

/// Where on screen something drawn on the graph is.
pub fn seen(harness: &Bench<'_>, drawn: &Node<'_>) -> egui::Pos2 {
    view(harness) * drawn.rect().center()
}

pub fn press<State>(harness: &mut Harness<'_, State>, at: egui::Pos2, button: PointerButton, pressed: bool) {
    harness.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE });
    harness.run_steps(2);
}

pub fn click<State>(harness: &mut Harness<'_, State>, at: egui::Pos2, button: PointerButton) {
    harness.hover_at(at);
    harness.run_steps(2);
    press(harness, at, button, true);
    press(harness, at, button, false);
}

/// Press on something, move to somewhere else in a few steps, and let go.
pub fn drag<State>(harness: &mut Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
    drag_by_way_of(harness, &[from, to]);
}

/// Press at the first of some places, move to each of the others in a few steps, and let go at the last.
pub fn drag_by_way_of<State>(harness: &mut Harness<'_, State>, places: &[egui::Pos2]) {
    harness.hover_at(places[0]);
    harness.run_steps(2);
    press(harness, places[0], PointerButton::Primary, true);
    for leg in places.windows(2) {
        for step in 1..=6 {
            harness.hover_at(leg[0] + (leg[1] - leg[0]) * (step as f32 / 6.0));
            harness.run_steps(1);
        }
    }
    press(harness, places[places.len() - 1], PointerButton::Primary, false);
}
