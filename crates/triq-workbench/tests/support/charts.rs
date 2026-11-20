//! The charts drawn without a window, for the tests that handle them as a user would.
//! Included with `#[path]`, after `scene` and `window`.
#![allow(dead_code)]
use super::scene::*;
use super::window::{drag, drawn};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use std::time::Duration;
use triq_workbench::charts::{Charts, Recorded, runtime_of};
use triq_workbench::colors::Colors;
use triq_workbench::egui::{self, accesskit::Role};
use triq_workbench::history::History;
use triq_workbench::signals::Signals;

pub struct Shown {
    pub charts: Charts,
    pub history: History,
    pub signals: Signals,
    /// The time at the right edge of every chart.
    pub now: f64,
}

pub type Window<'a> = Harness<'a, Shown>;

/// The row of one number of a connection in the list: the thing to drag.
pub fn number<'a>(harness: &'a Window<'_>, input: &'a str, stat: &'a str) -> Node<'a> {
    // A chart may be written on with the same name. The list is to the left of the charts.
    let named = harness.query_all_by_label(input).min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
    let connection = named.expect("its name in the list").parent().expect("the connection's part of the list");
    connection.get_by_label(stat)
}

/// Where to press on one number in the list. Topics of other programs on the
/// machine may be listed above it, so it is brought into view first.
pub fn grab(harness: &mut Window<'_>, entry: &str, word: &str) -> egui::Pos2 {
    number(harness, entry, word).scroll_to_me();
    harness.run_steps(3);
    number(harness, entry, word).rect().center()
}

pub fn shows(harness: &Window<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// A place on each empty chart: where it says so.
pub fn empty(harness: &Window<'_>) -> Vec<egui::Pos2> {
    harness.query_all_by_label("Drag a number here.").map(|hint| hint.rect().center()).collect()
}

/// Type into the box on a chart that says how much of the past it shows.
pub fn show_the_last(harness: &mut Window<'_>, chart: &str, seconds: f64) {
    let room = room(harness, chart);
    let past = harness.query_all_by_role(Role::SpinButton).find(|number| room.contains(number.rect().center()));
    past.expect("a box on the chart").set_value_accesskit(seconds);
    harness.run_steps(3);
}

/// Whether something is written on a chart.
pub fn written_on(harness: &Window<'_>, chart: &str, label: &str) -> bool {
    let room = room(harness, chart);
    harness.query_all_by_label(label).any(|written| room.contains(written.rect().center()))
}

/// The charts in a window. The list holds every topic on the machine, and they are first
/// read a moment after the start: the window opens after that, so that the topics of other
/// programs do not come into the list, and move its rows, under a pointer that took aim.
pub fn open(shown: Shown) -> Window<'static> {
    assert!(wait_for(|| !shown.signals.topics().is_empty()), "the topics of the machine are listed");
    // Each runtime that has something in the history is given a colour, as the workbench gives it one.
    let mut colors = Colors::default();
    let mut harness = Harness::builder().with_size(egui::vec2(1300.0, 800.0)).wgpu().build_ui_state(
        move |ui, shown: &mut Shown| {
            let named: Vec<String> = shown.history.outputs().cloned().collect();
            let heard: Vec<String> = shown.history.connections().map(|connection| connection.to.clone()).collect();
            let runtimes: std::collections::BTreeSet<&str> =
                named.iter().chain(&heard).map(|name| runtime_of(name)).collect();
            colors.update(runtimes);
            let recorded = Recorded { history: &shown.history, signals: &shown.signals };
            shown.charts.show(ui, recorded, &colors, shown.now);
        },
        shown,
    );
    harness.run_steps(3);
    harness
}

/// Where the name of a chart is, on the bar above it: the thing to take a chart by.
pub fn name(harness: &Window<'_>, chart: &str) -> egui::Rect {
    harness.get_by_role_and_label(Role::Button, chart).rect()
}

/// The name of a chart on the bar above it: a click brings the chart forward.
pub fn name_of<'a>(harness: &'a Window<'_>, chart: &'a str) -> Node<'a> {
    harness.get_by_role_and_label(Role::Button, chart)
}

/// Where a chart is drawn, under its bar.
pub fn room(harness: &Window<'_>, chart: &str) -> egui::Rect {
    harness.get_by_role_and_label(Role::Canvas, chart).rect()
}

/// Press the button that adds a chart.
pub fn add(harness: &mut Window<'_>) {
    harness.get_by_label("New chart").click();
    harness.run_steps(3);
}

/// Take a chart by its name and put it down at a place.
pub fn put(harness: &mut Window<'_>, chart: &str, at: egui::Pos2) {
    let name = name(harness, chart);
    drag(harness, name.center(), at);
    harness.run_steps(10);
}

/// The middle of one half of where a chart is drawn: `side` points from the chart's middle to that half.
pub fn half(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    let room = room(harness, chart);
    room.center() + side * room.size() / 4.0
}

/// Where to put a chart down to make it the neighbour of another on that side: high on the
/// half, where the highlight takes in the bar above the chart as well.
pub fn beside(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    half(harness, chart, side) - egui::vec2(0.0, 30.0)
}

/// Where to put a chart down to have it go in with another, as a group of the two: low on the
/// half, where the highlight is all under the bar.
pub fn within(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    half(harness, chart, side) + egui::vec2(0.0, 30.0)
}

/// Press the cross beside the name of a chart.
pub fn close(harness: &mut Window<'_>, chart: &str) {
    let name = name(harness, chart);
    let cross = harness.query_all_by_label("Close").find(|cross| name.contains(cross.rect().center()));
    cross.expect("a cross beside the name").click();
    harness.run_steps(3);
}

/// A window for arranging charts in. The scene was read twice while nothing
/// happened in it: enough to put numbers on charts, though every line is flat.
pub fn arranging(scene: &Scene, charts: Charts) -> triq::Result<Window<'static>> {
    let (mut history, reading) = (History::default(), scene.inventories());
    history.record(&reading);
    history.record(&later(&reading, scene.inventories(), [Duration::from_secs(2); 2]));
    let now = history.newest().expect("a point");
    Ok(open(Shown { charts, history, signals: Signals::start()?, now }))
}

/// The colour a line on a chart is drawn in, as the button beside its name says it: `#rrggbbaa`.
pub fn colour(harness: &Window<'_>, line: &str) -> String {
    let names = harness.get_by_label(line).parent().expect("the name's row");
    names.get_by_role(Role::ColorWell).value().expect("a colour")
}

/// The lines that a chart draws: for each piece, the places it passes through and its colour.
pub fn pieces(harness: &Window<'_>, chart: &str) -> Vec<(Vec<egui::Pos2>, egui::Color32)> {
    let room = room(harness, chart).expand(1.0);
    let lines = drawn(harness).0.into_iter().filter(|line| line.width == 2.0 && room.contains_rect(line.within));
    lines.filter(|line| line.points.len() > 1).map(|line| (line.points, line.color)).collect()
}

/// The pieces that the lines of a chart are drawn in, from left to right: how strong the colour of each is.
pub fn strengths(harness: &Window<'_>, chart: &str) -> Vec<u8> {
    let mut pieces: Vec<(f32, u8)> =
        pieces(harness, chart).into_iter().map(|(points, color)| (points[0].x, color.a())).collect();
    pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
    pieces.into_iter().map(|(_, strength)| strength).collect()
}

/// Where a chart draws the newest point of each of its curves: a dot.
pub fn dots(harness: &Window<'_>, chart: &str) -> Vec<egui::Pos2> {
    let room = room(harness, chart).expand(1.0);
    let dots = drawn(harness).1.into_iter().filter(|(_, size, within)| *size == 4.0 && room.contains_rect(*within));
    dots.map(|(middle, ..)| middle).collect()
}

/// The present by the clock of the machine: when a message that is sent now is stamped.
pub fn present() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("a clock").as_secs_f64()
}
