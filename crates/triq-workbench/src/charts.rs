//! Numbers over time, drawn as charts.
//!
//! On the left is everything that has numbers, with each as it stands: what a
//! connection did, how fast an output sends, and what is inside the messages
//! of a topic. A number dragged onto a chart is drawn there as a line over
//! time, and several can share a chart, each in a colour of its own. The top
//! and bottom of a chart are always the largest and the smallest value in view.
//! The list can be hidden, to give the charts its room.
//!
//! There is one chart to begin with, and a button adds another beside what is
//! there. Any of them is closed by the cross beside its name, also the last.
//! A chart is taken by its name and put down beside another, on its
//! middle to share its place, or inside it to make a group of the two: see
//! `panes.rs`. The line between two charts is dragged to give one more room.
//! `egui_tiles` does the arranging.
//!
//! A chart ends at the present, which the window moves on with every frame it
//! draws: the lines slide along evenly, however often points arrive. A number
//! from inside a message is drawn right up to the present. What a runtime
//! reports is drawn a moment behind it, where the report has arrived.
//!
//! A chart is of one kind from when it is made. Besides numbers over time
//! (`lines.rs`), there are charts of numbers against each other (`curves.rs`):
//! two numbers of one message, as a point that moves on a plane, or three, as
//! a point that moves in space.
use crate::history::History;
use crate::signals::{Signals, Wanted};
use curves::Curve;
use eframe::egui::{self, Sense, Ui, UiBuilder};
use egui_tiles::{Behavior, Container, SimplificationOptions, Tile, TileId, Tiles, Tree, UiResponse};
use lines::Line;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use triq_views::panes;
use view::View;

mod curves;
mod lines;
mod list;
mod pen;
mod view;

pub use curves::sparse;
pub use lines::{Trace, extent, thinned, written};
pub use list::Sort;
pub use pen::{color, pieces};
pub use view::{Frame, Ground, Lens, Point, clipped, ground_reach, marks, round_step, tick};

/// Where the numbers come from: what the runtimes report of their connections
/// and outputs, and what is read out of the messages themselves.
#[derive(Clone, Copy)]
pub struct Recorded<'a> {
    pub history: &'a History,
    pub signals: &'a Signals,
}

/// The least width that the list of numbers is laid out in, and the least that it leaves the
/// charts beside it. A window that is narrower than both cuts off what does not fit.
const NARROWEST_LIST: f32 = 180.0;
const NARROWEST_CHARTS: f32 = 260.0;

/// What a chart draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum Kind {
    /// Numbers over time, each as a line.
    #[default]
    Time,
    /// Two numbers of one message against each other: a curve on a plane.
    Plane,
    /// Three numbers of one message against each other: a curve in space.
    Space,
}

impl Kind {
    /// How many numbers make a point of a curve.
    fn axes(self) -> usize {
        match self {
            Kind::Time => 1,
            Kind::Plane => 2,
            Kind::Space => 3,
        }
    }

    /// What a chart with nothing on it says it takes.
    fn hint(self) -> &'static str {
        match self {
            Kind::Time => "Drag a number here.",
            Kind::Plane => "Drag two numbers of one message here: the one along x first, then the one along y.",
            Kind::Space => "Drag three numbers of one message here: the ones along x, y and z, in that order.",
        }
    }
}

/// One chart: the number it is called by, what kind it is, how much of the
/// past it shows, and what is drawn on it.
#[derive(Serialize, Deserialize)]
struct Chart {
    number: usize,
    /// A chart that was kept before there were kinds is of numbers over time.
    #[serde(default)]
    kind: Kind,
    /// How many seconds of the past it shows. A chart that was kept before each had its own shows the usual.
    #[serde(default = "Chart::usual")]
    seconds: f64,
    /// On a chart of numbers over time: its lines.
    lines: Vec<Line>,
    /// On a chart of numbers against each other: its curves, and how it is looked at.
    #[serde(default)]
    curves: Vec<Curve>,
    #[serde(default)]
    view: View,
    /// Whether what is drawn on it is named over it.
    #[serde(default = "Chart::named")]
    legend: bool,
}

impl Chart {
    fn new(number: usize, kind: Kind) -> Self {
        // In space a unit is as long along one axis as along another, until the chart is told otherwise.
        let view = View { equal: kind == Kind::Space, ..View::default() };
        Self { number, kind, seconds: Self::usual(), lines: Vec::new(), curves: Vec::new(), view, legend: true }
    }

    /// A chart names what is drawn on it until it is told not to.
    fn named() -> bool {
        true
    }

    /// How many seconds of the past a chart shows until it is told otherwise.
    fn usual() -> f64 {
        30.0
    }

    fn name(&self) -> String {
        format!("Chart {}", self.number)
    }

    /// The first colour that nothing on the chart was given.
    fn free(&self) -> usize {
        let given = |shade: &usize| {
            self.lines
                .iter()
                .map(|line| &line.pen)
                .chain(self.curves.iter().map(|curve| &curve.pen))
                .any(|pen| pen.shade == *shade)
        };
        (0..).find(|shade| !given(shade)).unwrap_or_default()
    }

    /// Draw a line on the chart, unless one that follows the same number is
    /// there already. It takes the first colour that is free.
    fn put(&mut self, mut line: Line) {
        if !self.lines.iter().any(|drawn| drawn.trace == line.trace) {
            line.pen.shade = self.free();
            self.lines.push(line);
        }
    }

    /// Draw a curve on the chart, in the first colour that is free.
    fn trace(&mut self, mut curve: Curve) {
        curve.pen.shade = self.free();
        self.curves.push(curve);
    }

    /// Draw numbers out of the list on the chart, as putting them down on it does.
    fn take_in(&mut self, numbers: &[Trace]) {
        for number in numbers {
            match (self.kind, number) {
                (Kind::Time, _) => self.put(Line::new(number.clone())),
                // A number of a message goes along the first axis that a curve of its topic
                // has free. If none has, it begins a curve, along x.
                (_, Trace::Inside { topic, number }) => {
                    let axes = self.kind.axes();
                    let free = |curve: &mut Curve| curve.axes[..axes].iter().position(|axis| axis.is_none());
                    let mut of_topic = self.curves.iter_mut().filter(|curve| curve.topic == *topic);
                    match of_topic.find_map(|curve| Some((free(curve)?, curve))) {
                        Some((axis, curve)) => curve.axes[axis] = Some(number.clone()),
                        None => {
                            let mut curve = Curve::new(topic);
                            curve.axes[0] = Some(number.clone());
                            self.trace(curve);
                        }
                    }
                }
                // What a runtime counts is not inside a message, and has nothing to be drawn against.
                _ => {}
            }
        }
    }

    /// Take what was given that colour off this chart and draw it on another of its kind.
    fn hand(&mut self, shade: usize, onto: &mut Chart) {
        if self.kind != onto.kind {
            return;
        }
        if let Some(line) = self.lines.iter().position(|line| line.pen.shade == shade) {
            onto.put(self.lines.remove(line));
        }
        if let Some(curve) = self.curves.iter().position(|curve| curve.pen.shade == shade) {
            onto.trace(self.curves.remove(curve));
        }
    }
}

/// What is dragged onto a chart.
#[derive(Clone)]
enum Carried {
    /// Numbers out of the list, in the order they were chosen in.
    Numbers(Vec<Trace>),
    /// A line or a curve of a chart, on its way to another: the number of the
    /// chart it is on, and which of that chart's colours it was given.
    Line { chart: usize, shade: usize },
    /// The number along an axis of a curve, on its way to another axis of the curve: the
    /// number of the chart, which of its colours the curve was given, and the axis.
    Slot { chart: usize, shade: usize, axis: usize },
}

/// What is held over a place on a chart, to be put down there, and whether it was just let go of.
fn over(ui: &Ui, place: egui::Rect) -> Option<(Arc<Carried>, bool)> {
    ui.ctx().pointer_interact_pos().filter(|pointer| place.contains(*pointer))?;
    let carried = egui::DragAndDrop::payload::<Carried>(ui.ctx())?;
    Some((carried, ui.input(|input| input.pointer.any_released())))
}

/// A line of the chart of that number that is held over a row of the chart's names, to be put
/// down there: which of the chart's colours it was given, and whether it was just let go of.
fn held_over(ui: &Ui, chart: usize, row: egui::Rect) -> Option<(usize, bool)> {
    let (carried, let_go) = over(ui, row)?;
    match *carried {
        Carried::Line { chart: from, shade } if from == chart => Some((shade, let_go)),
        _ => None,
    }
}

/// Show where a line that is held over a row of names will go: above the row or below it.
fn mark(ui: &Ui, row: egui::Rect, above: bool) {
    let edge = if above { row.top() } else { row.bottom() };
    ui.painter().hline(row.x_range(), edge, egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
}

/// Draw something that is dragged onto a chart, and can be clicked as well:
/// as `Ui::dnd_drag_source` does, which takes no clicks.
fn carried(
    ui: &mut Ui,
    id: egui::Id,
    carried: Carried,
    contents: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    if !ui.ctx().is_being_dragged(id) {
        let drawn = contents(ui);
        return ui.interact(drawn.rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
    }
    egui::DragAndDrop::set_payload(ui.ctx(), carried);
    // On its way, it is drawn over everything else, where the pointer is.
    let layer = egui::LayerId::new(egui::Order::Tooltip, id);
    let drawn = ui.scope_builder(UiBuilder::new().layer_id(layer), contents).inner;
    if let Some(pointer) = ui.ctx().pointer_interact_pos() {
        let to_pointer = egui::emath::TSTransform::from_translation(pointer - drawn.rect.center());
        ui.ctx().transform_layer_shapes(layer, to_pointer);
    }
    drawn
}

/// The charts, with all that the next run of the workbench is to find as it was.
#[derive(Serialize, Deserialize)]
pub struct Charts {
    /// The charts, and how they share the room: side by side, one under
    /// another, or in one place with one of them in front. Each of those can
    /// hold more of the same.
    panes: Tree<Chart>,
    /// How many charts there have been. The next is called by the next number.
    made: usize,
    /// Whether the list of numbers is shown beside the charts.
    listed: bool,
    /// The numbers in the list that are chosen, to be dragged onto a chart together.
    #[serde(skip)]
    chosen: Vec<Trace>,
    /// What the list of numbers is in order of.
    #[serde(default)]
    sorted: Sort,
}

impl Default for Charts {
    fn default() -> Self {
        let panes = Tree::new_tabs(egui::Id::unique("charts"), vec![Chart::new(1, Kind::Time)]);
        Self { panes, made: 1, listed: true, chosen: Vec::new(), sorted: Sort::default() }
    }
}

impl Charts {
    /// Draw a number on the chart of that number, as dropping it there does.
    pub fn add(&mut self, chart: usize, trace: Trace) {
        if let Some(chart) = self.charts_mut().find(|candidate| candidate.number == chart) {
            chart.take_in(&[trace]);
        }
    }

    /// Every chart, also one that is behind another.
    fn charts(&self) -> impl Iterator<Item = &Chart> {
        self.panes.tiles.tiles().filter_map(|tile| match tile {
            Tile::Pane(chart) => Some(chart),
            Tile::Container(_) => None,
        })
    }

    fn charts_mut(&mut self) -> impl Iterator<Item = &mut Chart> {
        self.panes.tiles.tiles_mut().filter_map(|tile| match tile {
            Tile::Pane(chart) => Some(chart),
            Tile::Container(_) => None,
        })
    }

    /// True when no chart has anything on it: nothing moves, so nothing needs drawing again.
    pub fn is_empty(&self) -> bool {
        self.charts().all(|chart| chart.lines.is_empty() && chart.curves.is_empty())
    }

    /// Draw the list of numbers and the charts. `now` is the present, the time
    /// at the right edge of every chart, in seconds since 1970.
    pub fn show(&mut self, ui: &mut Ui, recorded: Recorded<'_>, now: f64) {
        // In a narrow window the list gives way to the charts, down to the least it is laid out in.
        let most = (ui.available_width() - NARROWEST_CHARTS).max(NARROWEST_LIST);
        let list = egui::Panel::left("numbers").default_size(300.0).size_range(NARROWEST_LIST..=most);
        // A list that is hidden has no topic open that anyone could see.
        let numbers = |ui: &mut Ui| list::numbers(ui, recorded, &mut self.chosen, &mut self.sorted);
        let opened = if self.listed { list.show(ui, numbers).inner } else { BTreeSet::new() };
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.toggle_value(&mut self.listed, "Numbers").on_hover_text("Show or hide the list of numbers");
                ui.separator();
                let about = "An empty chart, beside the others. Take it by its name to put it where it should be.";
                if ui.button("New chart").on_hover_text(about).clicked() {
                    self.new_chart(Kind::Time);
                }
                let about = "An empty chart of two numbers of one message against each other: a point on a plane.";
                if ui.button("New 2D chart").on_hover_text(about).clicked() {
                    self.new_chart(Kind::Plane);
                }
                let about = "An empty chart of three numbers of one message against each other: a point in space.";
                if ui.button("New 3D chart").on_hover_text(about).clicked() {
                    self.new_chart(Kind::Space);
                }
            });
            if self.charts().next().is_none() {
                ui.weak("There is no chart. The buttons above make one.");
            }
            let mut drawn = Drawn { recorded, now, forward: None, moved: None };
            let carried = self.panes.dragged_id(ui.ctx());
            self.panes.ui(&mut drawn, ui);
            if panes::tidy(&mut self.panes, carried) {
                ui.ctx().request_repaint();
            }
            if let Some(chart) = drawn.forward {
                self.panes.make_active(|tile, _| tile == chart);
            }
            // What was put down on another chart is taken off the one it was on.
            if let Some((from, shade, onto)) = drawn.moved {
                let mut charts: Vec<&mut Chart> = self.charts_mut().collect();
                let place = |charts: &[&mut Chart], number| charts.iter().position(|chart| chart.number == number);
                if let (Some(from), Some(onto)) = (place(&charts, from), place(&charts, onto))
                    && let Ok([from, onto]) = charts.get_disjoint_mut([from, onto])
                {
                    from.hand(shade, onto);
                }
            }
        });
        recorded.signals.watch(self.wanted(opened));
    }

    /// What is to be read out of messages: the topics that are open in the
    /// list, and every number from inside a message that is on a chart.
    fn wanted(&self, opened: BTreeSet<String>) -> Wanted {
        let mut wanted: Wanted = opened.into_iter().map(|topic| (topic, BTreeSet::new())).collect();
        for chart in self.charts() {
            for line in &chart.lines {
                if let Trace::Inside { topic, number } = &line.trace {
                    wanted.entry(topic.clone()).or_default().insert(number.clone());
                }
            }
            for curve in &chart.curves {
                wanted.entry(curve.topic.clone()).or_default().extend(curve.axes.iter().flatten().cloned());
            }
        }
        wanted
    }

    /// Add an empty chart. The charts share the room as a grid, until the user
    /// arranges them another way: then a new one goes beside what is there.
    fn new_chart(&mut self, kind: Kind) {
        self.made += 1;
        let chart = self.panes.tiles.insert_pane(Chart::new(self.made, kind));
        let root = self.panes.root;
        self.panes.root = Some(match root.and_then(|root| Some((root, self.panes.tiles.get_mut(root)?))) {
            Some((root, Tile::Container(Container::Grid(grid)))) => {
                grid.add_child(chart);
                root
            }
            Some((root, _)) => self.panes.tiles.insert_grid_tile(vec![root, chart]),
            None => chart,
        });
    }
}

/// Written over the top of a chart: how much of the past it shows and, for
/// numbers against each other, how it is looked at. Under that is what is
/// drawn on it, by name, unless the chart is told not to name it.
fn written_over(ui: &mut Ui, chart: &mut Chart, recorded: Recorded<'_>) {
    // The lines pass behind what is written, and show through what it is written on.
    let backing = egui::Frame::new().fill(ui.visuals().extreme_bg_color.gamma_multiply(0.7)).inner_margin(2);
    backing.show(ui, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label("Show the last");
            let seconds = egui::DragValue::new(&mut chart.seconds).range(1.0..=History::LONGEST).suffix(" s");
            ui.add(seconds).labelled_by(label.id).on_hover_text("How much of the past this chart shows");
            if chart.kind != Kind::Time {
                let about = "One scale for every axis: a unit is as long along one as along another";
                ui.toggle_value(&mut chart.view.equal, "1:1").on_hover_text(about);
                let about = "Show all there is again, after zooming with the wheel or dragging the chart";
                if ui.button("Fit").on_hover_text(about).clicked() {
                    chart.view.held = None;
                }
            }
            if chart.kind == Kind::Space {
                let label = ui.label("Ground");
                let lying = egui::ComboBox::from_id_salt("ground").selected_text(chart.view.ground.name()).width(40.0);
                let chosen = lying.show_ui(ui, |ui| {
                    for ground in Ground::ALL {
                        ui.selectable_value(&mut chart.view.ground, ground, ground.name());
                    }
                });
                let about = "Which two axes are the ground, where the grid is. The third is up.";
                chosen.response.labelled_by(label.id).on_hover_text(about);
            }
            let about = "Show or hide the names of what is drawn on this chart";
            ui.toggle_value(&mut chart.legend, "Legend").on_hover_text(about);
        });
        // A chart with nothing on it says what it takes, whether it names things or not.
        if !chart.legend && !(chart.lines.is_empty() && chart.curves.is_empty()) {
            return;
        }
        match chart.kind {
            Kind::Time => lines::names(ui, chart.number, &mut chart.lines, recorded),
            kind => curves::names(ui, chart.number, kind, &mut chart.curves),
        }
    });
}

/// How the panes are drawn. Each is a chart, under a bar that has its name on
/// it: the chart is taken by its name to be put down elsewhere, and closed by
/// the cross beside it.
struct Drawn<'a> {
    recorded: Recorded<'a>,
    /// The present: the time at the right edge of every chart.
    now: f64,
    /// A chart to bring in front of those it shares its place with.
    forward: Option<TileId>,
    /// A line that was put down on a chart: the number of the chart it is on, which of that
    /// chart's colours it was given, and the number of the chart it was put down on.
    moved: Option<(usize, usize, usize)>,
}

impl Behavior<Chart> for Drawn<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, _: TileId, chart: &mut Chart) -> UiResponse {
        let (_, landed) = ui.dnd_drop_zone::<Carried, ()>(egui::Frame::canvas(ui.style()), |ui| {
            ui.set_min_size(ui.available_size());
            // The lines have the whole chart. Their names are written over them.
            let whole = ui.available_rect_before_wrap();
            let (name, shown) = (chart.name(), (self.now - chart.seconds, self.now));
            match chart.kind {
                Kind::Time => lines::lines(ui, &name, &chart.lines, self.recorded, shown.0, shown.1),
                kind => curves::draw(ui, (&name, kind), &mut chart.curves, &mut chart.view, self.recorded, shown),
            }
            ui.scope_builder(UiBuilder::new().max_rect(whole), |ui| written_over(ui, chart, self.recorded));
        });
        match landed.as_deref() {
            Some(Carried::Numbers(numbers)) => chart.take_in(numbers),
            Some(Carried::Line { chart: from, shade }) => self.moved = Some((*from, *shade, chart.number)),
            // A number that leaves its slot and is put down beside the slots stays where it was.
            Some(Carried::Slot { .. }) | None => {}
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, chart: &Chart) -> egui::WidgetText {
        chart.name().into()
    }

    /// The name of a group on a bar lists the charts in it.
    fn tab_title_for_tile(&mut self, tiles: &Tiles<Chart>, place: TileId) -> egui::WidgetText {
        match tiles.get(place) {
            Some(Tile::Pane(chart)) => self.tab_title_for_pane(chart),
            _ => {
                let charts = panes::panes_in(tiles, place).into_iter().filter_map(|chart| tiles.get_pane(&chart));
                let numbers: Vec<String> = charts.map(|chart| chart.number.to_string()).collect();
                format!("Charts {}", numbers.join(", ")).into()
            }
        }
    }

    /// What is held over the name of a chart brings the chart forward, to be put down on it.
    fn on_tab_button(&mut self, _: &mut Tiles<Chart>, chart: TileId, name: egui::Response) -> egui::Response {
        if name.dnd_hover_payload::<Carried>().is_some() {
            self.forward = Some(chart);
        }
        name.on_hover_text(
            "Drag the name to put the chart beside another, or onto the middle of one to share its place",
        )
    }

    /// What is behind a name is closed with it: a chart, or every chart of a group. The last
    /// chart can be closed like any other, and another made with the buttons above.
    fn is_tab_closable(&self, _: &Tiles<Chart>, _: TileId) -> bool {
        true
    }

    fn simplification_options(&self) -> SimplificationOptions {
        panes::rules()
    }
}
