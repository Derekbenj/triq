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
use crate::colors;
use crate::history::{Connection, History, Stat};
use crate::signals::{Signals, Wanted};
use eframe::egui::color_picker::{Alpha, color_edit_button_srgba};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Role, Sense, Stroke, Ui, UiBuilder, WidgetInfo};
use egui_tiles::{Behavior, Container, SimplificationOptions, Tile, TileId, Tiles, Tree, UiResponse};
use std::collections::BTreeSet;
use triq_views::panes;

/// Where the numbers come from: what the runtimes report of their connections
/// and outputs, and what is read out of the messages themselves.
#[derive(Clone, Copy)]
pub struct Recorded<'a> {
    pub history: &'a History,
    pub signals: &'a Signals,
}

/// How far behind the present the lines of what runtimes report end, in
/// seconds. The machine is read ten times a second, so the newest point is up
/// to a tenth of a second old, and a little more by the time it has been
/// fetched. Ended this far back, such a line always has a point to run on to,
/// and slides along instead of jumping forward with every reading.
const BEHIND: f64 = 0.15;

/// What a line on a chart follows: a number, over time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Trace {
    /// One number of a connection.
    Heard { connection: Connection, stat: Stat },
    /// How many samples a second an output sends.
    Sent { output: String },
    /// One number inside the messages of a topic.
    Inside { topic: String, number: String },
}

impl Trace {
    /// What the number is called among the others of its connection, output or topic.
    fn word(&self) -> &str {
        match self {
            Trace::Heard { stat, .. } => stat.name(),
            Trace::Sent { .. } => Stat::Rate.name(),
            Trace::Inside { number, .. } => number,
        }
    }

    /// What the line is called on a chart.
    fn name(&self) -> String {
        match self {
            Trace::Heard { connection, .. } => format!("{} {}", connection.to, self.word()),
            Trace::Sent { output: of } | Trace::Inside { topic: of, .. } => format!("{of} {}", self.word()),
        }
    }

    /// What the number counts in, written after it. Nobody says what a number inside a message counts in.
    fn unit(&self) -> &'static str {
        match self {
            Trace::Heard { stat, .. } => stat.unit(),
            Trace::Sent { .. } => Stat::Rate.unit(),
            Trace::Inside { .. } => "",
        }
    }

    /// The number as it stands.
    fn latest(&self, recorded: Recorded<'_>) -> Option<f64> {
        match self {
            Trace::Heard { connection, stat } => recorded.history.latest(connection).map(|point| stat.of(point)),
            Trace::Sent { output } => recorded.history.sending(output),
            Trace::Inside { topic, number } => recorded.signals.latest(topic, number),
        }
    }

    /// The number from the time `since` to the present, `now`, as far as it
    /// is drawn: pairs of time and value, oldest first.
    fn line(&self, recorded: Recorded<'_>, since: f64, now: f64) -> Vec<(f64, f64)> {
        match self {
            Trace::Heard { connection, stat } => recorded.history.line(connection, *stat, since, now - BEHIND),
            Trace::Sent { output } => recorded.history.sent(output, since, now - BEHIND),
            // It came with a message, and the message is here.
            Trace::Inside { topic, number } => recorded.signals.line(topic, number, since, now),
        }
    }
}

/// One line on a chart: the number it follows, and the colour it is drawn in.
struct Line {
    trace: Trace,
    /// Which of the colours that lines take in turn it was given.
    shade: usize,
    /// The colour that was chosen for it instead.
    chosen: Option<Color32>,
}

impl Line {
    fn color(&self, dark: bool) -> Color32 {
        self.chosen.unwrap_or_else(|| color(self.shade, dark))
    }
}

/// One chart: the number it is called by, and the lines drawn on it.
struct Chart {
    number: usize,
    lines: Vec<Line>,
}

impl Chart {
    fn new(number: usize) -> Self {
        Self { number, lines: Vec::new() }
    }

    fn name(&self) -> String {
        format!("Chart {}", self.number)
    }

    /// Draw a line on the chart, unless it is there already. It takes the
    /// first colour that no line on the chart was given.
    fn add(&mut self, trace: Trace) {
        if self.lines.iter().any(|line| line.trace == trace) {
            return;
        }
        let free = (0..).find(|shade| self.lines.iter().all(|line| line.shade != *shade));
        self.lines.push(Line { trace, shade: free.unwrap_or_default(), chosen: None });
    }
}

pub struct Charts {
    /// The charts, and how they share the room: side by side, one under
    /// another, or in one place with one of them in front. Each of those can
    /// hold more of the same.
    panes: Tree<Chart>,
    /// How many charts there have been. The next is called by the next number.
    made: usize,
    /// How many seconds of the past a chart shows.
    seconds: f64,
    /// Whether the list of numbers is shown beside the charts.
    listed: bool,
}

impl Default for Charts {
    fn default() -> Self {
        let panes = Tree::new_tabs(egui::Id::unique("charts"), vec![Chart::new(1)]);
        Self { panes, made: 1, seconds: 30.0, listed: true }
    }
}

impl Charts {
    /// Draw a line on the chart of that number, as dropping a number onto it does.
    pub fn add(&mut self, chart: usize, trace: Trace) {
        for tile in self.panes.tiles.tiles_mut() {
            if let Tile::Pane(candidate) = tile
                && candidate.number == chart
            {
                candidate.add(trace.clone());
            }
        }
    }

    /// True when no chart has a line on it: nothing moves, so nothing needs drawing again.
    pub fn is_empty(&self) -> bool {
        self.traces().next().is_none()
    }

    /// Draw the list of numbers and the charts. `now` is the present, the time
    /// at the right edge of every chart, in seconds since 1970.
    pub fn show(&mut self, ui: &mut Ui, recorded: Recorded<'_>, now: f64) {
        // A list that is hidden has no topic open that anyone could see.
        let list = egui::Panel::left("numbers").default_size(300.0);
        let opened = if self.listed { list.show(ui, |ui| numbers(ui, recorded)).inner } else { BTreeSet::new() };
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.toggle_value(&mut self.listed, "Numbers").on_hover_text("Show or hide the list of numbers");
                ui.separator();
                // How much of the past every chart shows.
                let label = ui.label("Show the last");
                let seconds = egui::DragValue::new(&mut self.seconds).range(5.0..=History::LONGEST).suffix(" s");
                ui.add(seconds).labelled_by(label.id);
                ui.separator();
                let about = "An empty chart, beside the others. Take it by its name to put it where it should be.";
                if ui.button("New chart").on_hover_text(about).clicked() {
                    self.new_chart();
                }
            });
            let mut drawn = Drawn { recorded, since: now - self.seconds, now, forward: None };
            let carried = self.panes.dragged_id(ui.ctx());
            self.panes.ui(&mut drawn, ui);
            if panes::tidy(&mut self.panes, carried) {
                ui.ctx().request_repaint();
            }
            if let Some(chart) = drawn.forward {
                self.panes.make_active(|tile, _| tile == chart);
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

/// How the panes are drawn. Each is a chart, under a bar that has its name on
/// it: the chart is taken by its name to be put down elsewhere, and closed by
/// the cross beside it.
struct Drawn<'a> {
    recorded: Recorded<'a>,
    /// The times at the left and the right edge of every chart.
    since: f64,
    now: f64,
    /// A chart to bring in front of those it shares its place with.
    forward: Option<TileId>,
}

impl Behavior<Chart> for Drawn<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, _: TileId, chart: &mut Chart) -> UiResponse {
        let (_, landed) = ui.dnd_drop_zone::<Trace, ()>(egui::Frame::canvas(ui.style()), |ui| {
            ui.set_min_size(ui.available_size());
            // The lines have the whole chart. Their names are written over them.
            let whole = ui.available_rect_before_wrap();
            lines(ui, chart, self.recorded, self.since, self.now);
            ui.scope_builder(UiBuilder::new().max_rect(whole), |ui| names(ui, &mut chart.lines, self.recorded));
        });
        if let Some(trace) = landed {
            chart.add(Trace::clone(&trace));
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

    /// A number that is held over the name of a chart brings the chart forward, to be put down on it.
    fn on_tab_button(&mut self, _: &mut Tiles<Chart>, chart: TileId, name: egui::Response) -> egui::Response {
        if name.dnd_hover_payload::<Trace>().is_some() {
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

/// Everything that has numbers, in order of name: what a node hears and what
/// it sends stand together. Each number can be dragged onto a chart. A topic
/// can be opened, to see the numbers inside its messages; the topics that
/// are open are returned.
fn numbers(ui: &mut Ui, recorded: Recorded<'_>) -> BTreeSet<String> {
    let Recorded { history, signals } = recorded;
    // What each is called, what is said under that, its numbers, and whether it is a topic.
    let heard = history.connections().map(|connection| {
        let numbers = Stat::ALL.map(|stat| Trace::Heard { connection: connection.clone(), stat });
        (connection.to.clone(), format!("from {}", connection.from), numbers.to_vec(), false)
    });
    let outputs: BTreeSet<&String> = history.outputs().collect();
    let sent = outputs
        .iter()
        .map(|output| ((*output).clone(), "output".to_owned(), vec![Trace::Sent { output: (*output).clone() }], true));
    // A topic that no runtime reports has nobody to count for it: another program's,
    // or an output whose program stopped while something still listens to it.
    let topics = signals.topics();
    let others = topics.iter().filter(|topic| !outputs.contains(&topic.name)).map(|topic| {
        let carried = format!("{}{}", topic.message, if topic.slice { "[]" } else { "" });
        (topic.name.clone(), format!("a topic no runtime answers for: {carried}"), Vec::new(), true)
    });
    let mut listed: Vec<_> = heard.chain(sent).chain(others).collect();
    listed.sort_by(|a, b| a.0.cmp(&b.0));

    let mut opened = BTreeSet::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.weak("Drag a number onto a chart to see it over time.");
        ui.separator();
        if listed.is_empty() {
            ui.weak("Nothing is running.");
        }
        for (name, about, numbers, topic) in listed {
            ui.vertical(|ui| {
                ui.strong(&name);
                ui.weak(about);
                for trace in numbers {
                    let value = trace.latest(recorded).map_or(String::new(), |value| written(value) + trace.unit());
                    number(ui, trace, value);
                }
                if topic {
                    egui::CollapsingHeader::new("numbers inside").id_salt(("inside", &name)).show(ui, |ui| {
                        inside(ui, signals, &name);
                        opened.insert(name.clone());
                    });
                }
            });
            ui.separator();
        }
    });
    opened
}

/// The numbers inside the messages of a topic that is open in the list.
fn inside(ui: &mut Ui, signals: &Signals, topic: &str) {
    let Some(watched) = signals.watched(topic) else {
        ui.weak("opening");
        return;
    };
    if let Some(problem) = &watched.problem {
        ui.weak(problem);
    } else if watched.numbers.is_empty() {
        ui.weak("Nobody says what is inside.");
    }
    for (name, value) in watched.numbers {
        number(ui, Trace::Inside { topic: topic.to_owned(), number: name }, value.map_or(String::new(), written));
    }
}

/// One number in the list, with its value as it stands: the thing to drag.
fn number(ui: &mut Ui, trace: Trace, value: String) {
    let word = trace.word().to_owned();
    ui.dnd_drag_source(egui::Id::unique(("number", &trace)), trace, |ui| {
        ui.horizontal(|ui| {
            // The values line up in a column of their own, unless the name is too long for that.
            let word = ui.label(word);
            ui.add_space((64.0 - word.rect.width()).max(8.0));
            ui.weak(value);
        });
    });
}

/// Written over the top of a chart: the colour, name and newest value of each
/// line. The colour is a button that opens a picker, and the last button takes
/// the line off.
fn names(ui: &mut Ui, lines: &mut Vec<Line>, recorded: Recorded<'_>) {
    if lines.is_empty() {
        ui.weak("Drag a number here.");
        return;
    }
    let mut taken_off = None;
    // The lines pass behind the names, and show through what the names are written on.
    let backing = egui::Frame::new().fill(ui.visuals().extreme_bg_color.gamma_multiply(0.7)).inner_margin(2);
    backing.show(ui, |ui| {
        for (number, line) in lines.iter_mut().enumerate() {
            // A row is known by what its line follows, and keeps its picker when the rows above it go.
            let row = ui.push_id(egui::Id::unique(&line.trace), |ui| {
                ui.horizontal(|ui| {
                    let mut color = line.color(ui.visuals().dark_mode);
                    let well = ui.scope(|ui| {
                        ui.spacing_mut().interact_size = egui::vec2(16.0, 14.0);
                        color_edit_button_srgba(ui, &mut color, Alpha::Opaque)
                    });
                    if well.inner.on_hover_text("Choose the colour of this line").changed() {
                        line.chosen = Some(color);
                    }
                    let name = ui.label(line.trace.name());
                    if let Trace::Heard { connection, .. } = &line.trace {
                        name.on_hover_text(format!("from {}", connection.from));
                    }
                    if let Some(value) = line.trace.latest(recorded) {
                        ui.weak(written(value) + line.trace.unit());
                    }
                    ui.small_button("×").on_hover_text("Take this line off the chart").clicked()
                })
            });
            if row.inner.inner {
                taken_off = Some(number);
            }
        }
    });
    if let Some(number) = taken_off {
        lines.remove(number);
    }
}

/// The lines of a chart, from the time `since` at its left edge to `until` at
/// its right. The chart reaches from the smallest value in view at the bottom
/// to the largest at the top, and says at its right edge what those are.
fn lines(ui: &mut Ui, chart: &Chart, recorded: Recorded<'_>, since: f64, until: f64) {
    let drawn = &chart.lines;
    let (whole, chart) = {
        let (whole, drawing) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        // The drawing is called by the chart's name, for whoever cannot see it.
        drawing.widget_info(|| WidgetInfo::labeled(Role::Canvas, true, chart.name()));
        (whole, drawing)
    };
    // Under the lines, a row that says what time the left and right edges are.
    let (area, foot) = whole.split_top_bottom_at_y(whole.bottom() - ui.spacing().interact_size.y);
    let seconds = until - since;
    let lines: Vec<Vec<(f64, f64)>> = drawn.iter().map(|line| line.trace.line(recorded, since, until)).collect();
    let Some((low, high)) = extent(lines.iter().flatten().map(|(_, value)| *value)) else { return };

    // A line along the very top or bottom is drawn whole, not cut in half by the edge.
    let inside = area.shrink2(egui::vec2(0.0, 2.0));
    let place = |(at, value): &(f64, f64)| {
        let (across, up) = ((at - since) / seconds, (value - low) / (high - low));
        egui::pos2(egui::lerp(inside.x_range(), across as f32), egui::lerp(inside.bottom()..=inside.top(), up as f32))
    };
    let (painter, dark) = (ui.painter_at(area), ui.visuals().dark_mode);
    for (line, points) in drawn.iter().zip(&lines) {
        let placed: Vec<Pos2> = points.iter().map(place).collect();
        painter.line(thinned(&placed), Stroke::new(2.0, line.color(dark)));
    }
    let corner = |ui: &mut Ui, within: Rect, layout: Layout, text: String| {
        ui.scope_builder(UiBuilder::new().max_rect(within).layout(layout), |ui| ui.weak(text));
    };
    corner(ui, area, Layout::top_down(Align::Max), written(high));
    corner(ui, area, Layout::bottom_up(Align::Max), written(low));
    corner(ui, foot, Layout::left_to_right(Align::Center), format!("{seconds} s ago"));
    corner(ui, foot, Layout::right_to_left(Align::Center), "now".to_owned());

    // Under the pointer: what each line was at that time, if it has a point within a second of it.
    if let Some(pointer) = chart.hover_pos().filter(|pointer| area.contains(*pointer)) {
        let at = since + f64::from((pointer.x - area.left()) / area.width()) * seconds;
        painter.vline(pointer.x, area.y_range(), Stroke::new(1.0, ui.visuals().weak_text_color()));
        chart.on_hover_ui_at_pointer(|ui| {
            for (line, points) in drawn.iter().zip(&lines) {
                let nearest = points.iter().min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()));
                if let Some((_, value)) = nearest.filter(|(time, _)| (time - at).abs() <= 1.0) {
                    ui.label(format!("{} {}{}", line.trace.name(), written(*value), line.trace.unit()));
                }
            }
            ui.weak(format!("{:.1} s ago", until - at));
        });
    }
}

/// The smallest and the largest of some values: the bottom and top of a chart.
/// A flat line lies along the bottom, with room above it.
pub fn extent(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let (low, high) =
        values.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)));
    // With no values at all, the smallest is still above the largest.
    (low <= high).then_some(if low < high { (low, high) } else { (low, low + 1.0) })
}

/// A line as it will be drawn, without the points that cannot be seen. An hour
/// at ten points a second is far more points than a chart has pixels across;
/// what shows of them in one pixel column is where the line enters and leaves
/// it and how high and low it goes there.
pub fn thinned(line: &[Pos2]) -> Vec<Pos2> {
    let mut kept = Vec::new();
    for column in line.chunk_by(|a, b| a.x.floor() == b.x.floor()) {
        let by_height = |a: &&Pos2, b: &&Pos2| a.y.total_cmp(&b.y);
        let (top, bottom) = (column.iter().min_by(by_height), column.iter().max_by(by_height));
        let mut few = [column[0], *top.unwrap_or(&column[0]), *bottom.unwrap_or(&column[0]), column[column.len() - 1]];
        // Back in the order they happened in, each of them once.
        few.sort_by(|a, b| a.x.total_cmp(&b.x));
        for point in few {
            if kept.last() != Some(&point) {
                kept.push(point);
            }
        }
    }
    kept
}

/// A number as a chart writes it: to three figures or so.
pub fn written(value: f64) -> String {
    match value.abs() {
        size if size >= 100.0 => format!("{value:.0}"),
        size if size >= 10.0 => format!("{value:.1}"),
        _ => format!("{value:.2}"),
    }
}

/// The colour the first, second, third line on a chart is given.
pub fn color(line: usize, dark: bool) -> Color32 {
    colors::shade(line, dark)
}
