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
    open_as_wide_as(1500.0)
}

/// The workbench in a window that is so many points across.
pub fn open_as_wide_as(width: f32) -> triq::Result<Bench<'static>> {
    Ok(Harness::builder()
        .with_size(egui::vec2(width, 900.0))
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

/// Everything in the window that is called by the name of one of the workbench's windows, from the top down.
pub fn called<'a>(harness: &'a Bench<'_>, window: &'a str) -> Vec<Node<'a>> {
    let mut called: Vec<_> = harness.query_all_by_role_and_label(Role::Button, window).collect();
    called.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    called
}

/// The name of a window in the row along the top: what opens and closes it.
pub fn switch<'a>(harness: &'a Bench<'_>, window: &'a str) -> Node<'a> {
    called(harness, window).into_iter().next().unwrap_or_else(|| panic!("no window is called {window:?}"))
}

/// The name of a window on the bar above it: what it is taken by. A window that is closed has none.
pub fn bar<'a>(harness: &'a Bench<'_>, window: &'a str) -> Option<Node<'a>> {
    called(harness, window).into_iter().nth(1)
}

/// Press the cross beside the name of a window, on the bar above it.
pub fn close(harness: &mut Bench<'_>, window: &str) {
    let name = bar(harness, window).expect("an open window").rect();
    let cross = harness.query_all_by_label("Close").find(|cross| name.contains(cross.rect().center()));
    cross.expect("a cross beside the name").click();
    harness.run_steps(3);
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

/// Where the pin of a port is on screen: the circle on the edge of a card, in the row where
/// `label` is written. `after` is whether the pin is to the right of that, as an output's is.
pub fn pin(harness: &Bench<'_>, label: &str, after: bool) -> egui::Pos2 {
    fn circles(shape: &egui::Shape, found: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| circles(shape, found)),
            egui::Shape::Circle(circle) => found.push(circle.center),
            _ => {}
        }
    }
    let mut found = Vec::new();
    harness.output().shapes.iter().for_each(|clipped| circles(&clipped.shape, &mut found));
    let written = seen(harness, &text(harness, label));
    let level = 6.0 * view(harness).scaling;
    let in_row = found.into_iter().filter(|pin| (pin.y - written.y).abs() < level && (pin.x > written.x) == after);
    let nearest = in_row.min_by(|a, b| (a.x - written.x).abs().total_cmp(&(b.x - written.x).abs()));
    nearest.unwrap_or_else(|| panic!("no pin beside {label:?}"))
}

/// Every dot that was drawn in the last frame and is filled in: its middle and its colour.
pub fn discs<State>(harness: &Harness<'_, State>) -> Vec<(egui::Pos2, egui::Color32)> {
    fn filled(shape: &egui::Shape, found: &mut Vec<(egui::Pos2, egui::Color32)>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|shape| filled(shape, found)),
            egui::Shape::Circle(dot) if dot.fill != egui::Color32::TRANSPARENT => found.push((dot.center, dot.fill)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    harness.output().shapes.iter().for_each(|clipped| filled(&clipped.shape, &mut found));
    found
}

/// A line that was drawn, and is not closed to a shape.
pub struct Stroke {
    /// The places it passes through.
    pub points: Vec<egui::Pos2>,
    pub color: egui::Color32,
    pub width: f32,
    /// The part of the window it was drawn in: what lies outside is not seen.
    pub within: egui::Rect,
}

/// Every line that was drawn in the last frame, and every dot: its middle, how large it is, and
/// the part of the window it was drawn in.
pub fn drawn<State>(harness: &Harness<'_, State>) -> (Vec<Stroke>, Vec<(egui::Pos2, f32, egui::Rect)>) {
    type Dots = Vec<(egui::Pos2, f32, egui::Rect)>;
    fn shapes(shape: &egui::Shape, within: egui::Rect, lines: &mut Vec<Stroke>, dots: &mut Dots) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|shape| shapes(shape, within, lines, dots)),
            egui::Shape::Path(path) if !path.closed => {
                if let egui::epaint::ColorMode::Solid(color) = &path.stroke.color {
                    lines.push(Stroke { points: path.points.clone(), color: *color, width: path.stroke.width, within });
                }
            }
            egui::Shape::LineSegment { points, stroke } => {
                lines.push(Stroke { points: points.to_vec(), color: stroke.color, width: stroke.width, within });
            }
            egui::Shape::Circle(circle) => dots.push((circle.center, circle.radius, within)),
            _ => {}
        }
    }
    let (mut lines, mut dots) = (Vec::new(), Vec::new());
    for clipped in &harness.output().shapes {
        shapes(&clipped.shape, clipped.clip_rect, &mut lines, &mut dots);
    }
    (lines, dots)
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

/// Press a button of the mouse on something, move to somewhere else in a few steps, and let go.
pub fn drag_with<State>(harness: &mut Harness<'_, State>, button: PointerButton, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.run_steps(2);
    press(harness, from, button, true);
    for step in 1..=6 {
        harness.hover_at(from + (to - from) * (step as f32 / 6.0));
        harness.run_steps(1);
    }
    press(harness, to, button, false);
}

/// Click with Shift held, as selects one more card in the graph.
pub fn shift_click<State>(harness: &mut Harness<'_, State>, at: egui::Pos2) {
    click_holding(harness, at, egui::Modifiers::SHIFT);
}

/// Click with some of the keys held that change what a click does.
pub fn click_holding<State>(harness: &mut Harness<'_, State>, at: egui::Pos2, shift: egui::Modifiers) {
    harness.hover_at(at);
    harness.run_steps(2);
    let button =
        |pressed| egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: shift };
    for event in [egui::Event::ModifiersChanged(shift), button(true), button(false)] {
        harness.event(event);
    }
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run_steps(2);
}

/// Press a key with the pointer where it is, with some modifiers held.
pub fn key<State>(harness: &mut Harness<'_, State>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(4);
}

/// Two clicks in quick succession. The harness lets a quarter of a second
/// pass with every frame it draws, which is too slow for that: time stands
/// still meanwhile.
pub fn double_click<State>(harness: &mut Harness<'_, State>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run_steps(2);
    harness.input_mut().time = Some(harness.ctx.input(|input| input.time));
    for pressed in [true, false, true, false] {
        let (button, modifiers) = (PointerButton::Primary, egui::Modifiers::NONE);
        harness.event(egui::Event::PointerButton { pos: at, button, pressed, modifiers });
    }
    harness.step();
    harness.input_mut().time = None;
    harness.run_steps(3);
}

/// List what a node hears and sends in the list of numbers, if that is folded away under its
/// name: `entry` is one of them, `runtime/node/port`. Other programs' nodes may be listed above
/// it, so its name is brought into view first. A chart may be written on with the name of an
/// entry: the list is to the left of the charts.
pub fn unfold<State>(harness: &mut Harness<'_, State>, entry: &str) {
    let node = entry.rsplit_once('/').map_or(entry, |(node, _)| node);
    fn name<'a, State>(harness: &'a Harness<'_, State>, node: &'a str) -> Node<'a> {
        let named = harness.query_all_by_label(node).min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
        named.unwrap_or_else(|| panic!("{node} is not in the list of numbers"))
    }
    let edge = name(harness, node).rect().right();
    if harness.query_all_by_label(entry).any(|listed| listed.rect().left() < edge) {
        return;
    }
    name(harness, node).scroll_to_me();
    harness.run_steps(3);
    name(harness, node).click();
    harness.run_steps(6);
}
