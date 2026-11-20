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
//! there. A chart is taken by its name and put down beside another, on its
//! middle to share its place, or inside it to make a group of the two: see
//! `panes.rs`. The line between two charts is dragged to give one more room.
//! `egui_tiles` does the arranging.
//!
//! A chart ends at the present, which the window moves on with every frame it
//! draws: the lines slide along evenly, however often points arrive. A number
//! from inside a message is drawn right up to the present. What a runtime
//! reports is drawn a moment behind it, where the report has arrived.
use crate::history::History;
use crate::signals::{Signals, Wanted};
use eframe::egui::{self, Sense, Ui, UiBuilder};
use egui_tiles::{Behavior, Container, SimplificationOptions, Tile, TileId, Tiles, Tree, UiResponse};
use lines::Line;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use triq_views::panes;

mod lines;
mod list;
mod pen;

pub use lines::{Trace, extent, thinned, written};
pub use pen::{color, pieces};

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

/// One chart: the number it is called by, how much of the past it shows, and the lines drawn on it.
#[derive(Serialize, Deserialize)]
struct Chart {
    number: usize,
    /// How many seconds of the past it shows. A chart that was kept before each had its own shows the usual.
    #[serde(default = "Chart::usual")]
    seconds: f64,
    lines: Vec<Line>,
}

impl Chart {
    fn new(number: usize) -> Self {
        Self { number, seconds: Self::usual(), lines: Vec::new() }
    }

    /// How many seconds of the past a chart shows until it is told otherwise.
    fn usual() -> f64 {
        30.0
    }

    fn name(&self) -> String {
        format!("Chart {}", self.number)
    }

    /// Draw a line on the chart, unless one that follows the same number is
    /// there already. It takes the first colour that no line on the chart was given.
    fn put(&mut self, mut line: Line) {
        if self.lines.iter().any(|drawn| drawn.trace == line.trace) {
            return;
        }
        let free = (0..).find(|shade| self.lines.iter().all(|line| line.pen.shade != *shade));
        line.pen.shade = free.unwrap_or_default();
        self.lines.push(line);
    }

    /// Take off the line that was given that colour.
    fn take(&mut self, shade: usize) -> Option<Line> {
        let line = self.lines.iter().position(|line| line.pen.shade == shade)?;
        Some(self.lines.remove(line))
    }
}

/// What is dragged onto a chart.
#[derive(Clone)]
enum Carried {
    /// Numbers out of the list, in the order they were chosen in.
    Numbers(Vec<Trace>),
    /// A line of a chart, on its way to another: the number of the chart it
    /// is on, and which of that chart's colours it was given.
    Line { chart: usize, shade: usize },
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
}

impl Default for Charts {
    fn default() -> Self {
        let panes = Tree::new_tabs(egui::Id::unique("charts"), vec![Chart::new(1)]);
        Self { panes, made: 1, listed: true, chosen: Vec::new() }
    }
}

impl Charts {
    /// Draw a line on the chart of that number, as dropping a number onto it does.
    pub fn add(&mut self, chart: usize, trace: Trace) {
        if let Some(chart) = self.chart(chart) {
            chart.put(Line::new(trace));
        }
    }

    /// The chart of that number.
    fn chart(&mut self, number: usize) -> Option<&mut Chart> {
        self.panes.tiles.tiles_mut().find_map(|tile| match tile {
            Tile::Pane(chart) if chart.number == number => Some(chart),
            _ => None,
        })
    }

    /// True when no chart has a line on it: nothing moves, so nothing needs drawing again.
    pub fn is_empty(&self) -> bool {
        self.traces().next().is_none()
    }

    /// Whether there is a chart at all. What was kept from another run might be anything.
    pub(crate) fn has_a_chart(&self) -> bool {
        self.panes.tiles.tiles().any(|tile| tile.is_pane())
    }

    /// Draw the list of numbers and the charts. `now` is the present, the time
    /// at the right edge of every chart, in seconds since 1970.
    pub fn show(&mut self, ui: &mut Ui, recorded: Recorded<'_>, now: f64) {
        // In a narrow window the list gives way to the charts, down to the least it is laid out in.
        let most = (ui.available_width() - NARROWEST_CHARTS).max(NARROWEST_LIST);
        let list = egui::Panel::left("numbers").default_size(300.0).size_range(NARROWEST_LIST..=most);
        // A list that is hidden has no topic open that anyone could see.
        let numbers = |ui: &mut Ui| list::numbers(ui, recorded, &mut self.chosen);
        let opened = if self.listed { list.show(ui, numbers).inner } else { BTreeSet::new() };
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.toggle_value(&mut self.listed, "Numbers").on_hover_text("Show or hide the list of numbers");
                ui.separator();
                let about = "An empty chart, beside the others. Take it by its name to put it where it should be.";
                if ui.button("New chart").on_hover_text(about).clicked() {
                    self.new_chart();
                }
            });
            let mut drawn = Drawn { recorded, now, forward: None, moved: None };
            let carried = self.panes.dragged_id(ui.ctx());
            self.panes.ui(&mut drawn, ui);
            if panes::tidy(&mut self.panes, carried) {
                ui.ctx().request_repaint();
            }
            if let Some(chart) = drawn.forward {
                self.panes.make_active(|tile, _| tile == chart);
            }
            // A line that was put down on another chart is taken off the one it was on.
            if let Some((from, shade, onto)) = drawn.moved.filter(|(from, _, onto)| from != onto)
                && let Some(line) = self.chart(from).and_then(|chart| chart.take(shade))
                && let Some(chart) = self.chart(onto)
            {
                chart.put(line);
            }
        });
        recorded.signals.watch(self.wanted(opened));
    }

    /// The lines of every chart, also of one that is behind another.
    fn traces(&self) -> impl Iterator<Item = &Trace> {
        let charts = self.panes.tiles.tiles().filter_map(|tile| match tile {
            Tile::Pane(chart) => Some(chart),
            Tile::Container(_) => None,
        });
        charts.flat_map(|chart| &chart.lines).map(|line| &line.trace)
    }

    /// What is to be read out of messages: the topics that are open in the
    /// list, and every number from inside a message that is on a chart.
    fn wanted(&self, opened: BTreeSet<String>) -> Wanted {
        let mut wanted: Wanted = opened.into_iter().map(|topic| (topic, BTreeSet::new())).collect();
        for trace in self.traces() {
            if let Trace::Inside { topic, number } = trace {
                wanted.entry(topic.clone()).or_default().insert(number.clone());
            }
        }
        wanted
    }

    /// Add an empty chart. The charts share the room as a grid, until the user
    /// arranges them another way: then a new one goes beside what is there.
    fn new_chart(&mut self) {
        self.made += 1;
        let chart = self.panes.tiles.insert_pane(Chart::new(self.made));
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

/// Written over the top of a chart: how much of the past it shows, and under
/// that what is drawn on it, by name.
fn written_over(ui: &mut Ui, chart: &mut Chart, recorded: Recorded<'_>) {
    // The lines pass behind what is written, and show through what it is written on.
    let backing = egui::Frame::new().fill(ui.visuals().extreme_bg_color.gamma_multiply(0.7)).inner_margin(2);
    backing.show(ui, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label("Show the last");
            let seconds = egui::DragValue::new(&mut chart.seconds).range(5.0..=History::LONGEST).suffix(" s");
            ui.add(seconds).labelled_by(label.id).on_hover_text("How much of the past this chart shows");
        });
        lines::names(ui, chart.number, &mut chart.lines, recorded);
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
            lines::lines(ui, &chart.name(), &chart.lines, self.recorded, self.now - chart.seconds, self.now);
            ui.scope_builder(UiBuilder::new().max_rect(whole), |ui| written_over(ui, chart, self.recorded));
        });
        match landed.as_deref() {
            Some(Carried::Numbers(numbers)) => numbers.iter().for_each(|trace| chart.put(Line::new(trace.clone()))),
            Some(Carried::Line { chart: from, shade }) => self.moved = Some((*from, *shade, chart.number)),
            None => {}
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

    /// What is behind a name is closed with it, unless that is every chart
    /// there is: the last stays, so that there is always one to put a number on.
    fn is_tab_closable(&self, tiles: &Tiles<Chart>, place: TileId) -> bool {
        panes::panes_in(tiles, place).len() < tiles.tiles().filter(|tile| tile.is_pane()).count()
    }

    fn simplification_options(&self) -> SimplificationOptions {
        panes::rules()
    }
}
