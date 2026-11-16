//! The second tab: numbers over time, drawn as charts.
//!
//! On the left is everything that has numbers, with each as it stands: what a
//! connection did, how fast an output sends, and what is inside the messages
//! of a topic. A number dragged onto a chart is drawn there as a line over
//! time; several can share a chart, and there can be several charts. The top
//! and bottom of a chart are always the largest and the smallest value in view.
//!
//! A chart ends at a time it is told, which the window moves on with every
//! frame it draws: the lines slide along evenly, however often points arrive.
use crate::colors;
use crate::history::{Connection, History, Stat};
use crate::signals::{Signals, Wanted};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Sense, Stroke, Ui, UiBuilder};
use std::collections::BTreeSet;

/// Where the numbers come from: what the runtimes report of their connections
/// and outputs, and what is read out of the messages themselves.
#[derive(Clone, Copy)]
pub struct Recorded<'a> {
    pub history: &'a History,
    pub signals: &'a Signals,
}

/// One line on a chart: a number, over time.
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

    /// The number between two times: pairs of time and value, oldest first.
    fn line(&self, recorded: Recorded<'_>, since: f64, until: f64) -> Vec<(f64, f64)> {
        match self {
            Trace::Heard { connection, stat } => recorded.history.line(connection, *stat, since, until),
            Trace::Sent { output } => recorded.history.sent(output, since, until),
            Trace::Inside { topic, number } => recorded.signals.line(topic, number, since, until),
        }
    }
}

pub struct Charts {
    /// Each chart is the lines drawn on it.
    charts: Vec<Vec<Trace>>,
    /// How many charts sit side by side.
    across: usize,
    /// How many seconds of the past a chart shows.
    seconds: f64,
}

impl Default for Charts {
    fn default() -> Self {
        Self { charts: vec![Vec::new()], across: 1, seconds: 30.0 }
    }
}

impl Charts {
    /// Draw a line on a chart, as dropping a number onto it does.
    pub fn add(&mut self, chart: usize, trace: Trace) {
        if !self.charts[chart].contains(&trace) {
            self.charts[chart].push(trace);
        }
    }

    /// How much of the past to show and how to arrange the charts, to be drawn in a row.
    pub fn controls(&mut self, ui: &mut Ui) {
        let label = ui.label("Show the last");
        let seconds = egui::DragValue::new(&mut self.seconds).range(5.0..=History::LONGEST).suffix(" s");
        ui.add(seconds).labelled_by(label.id);
        let label = ui.label("Charts side by side");
        ui.add(egui::DragValue::new(&mut self.across).range(1..=4)).labelled_by(label.id);
        if ui.small_button("Add chart").clicked() {
            self.charts.push(Vec::new());
        }
    }

    /// True when no chart has a line on it: nothing moves, so nothing needs drawing again.
    pub fn is_empty(&self) -> bool {
        self.charts.iter().all(Vec::is_empty)
    }

    /// Draw the list of numbers and the charts. `now` is the time at the
    /// right edge of every chart, in seconds since 1970.
    pub fn show(&mut self, ui: &mut Ui, recorded: Recorded<'_>, now: f64) {
        let opened = egui::Panel::left("numbers").default_size(300.0).show(ui, |ui| numbers(ui, recorded)).inner;
        egui::CentralPanel::default().show(ui, |ui| self.grid(ui, recorded, now));
        recorded.signals.watch(self.wanted(opened));
    }

    /// What is to be read out of messages: the topics that are open in the
    /// list, and every number from inside a message that is on a chart.
    fn wanted(&self, opened: BTreeSet<String>) -> Wanted {
        let mut wanted: Wanted = opened.into_iter().map(|topic| (topic, BTreeSet::new())).collect();
        for trace in self.charts.iter().flatten() {
            if let Trace::Inside { topic, number } = trace {
                wanted.entry(topic.clone()).or_default().insert(number.clone());
            }
        }
        wanted
    }

    /// The charts, in rows of `across`, sharing the room there is.
    fn grid(&mut self, ui: &mut Ui, recorded: Recorded<'_>, now: f64) {
        let room = ui.available_rect_before_wrap();
        let across = self.across.min(self.charts.len());
        let down = self.charts.len().div_ceil(across);
        let each = egui::vec2(room.width() / across as f32, room.height() / down as f32);
        let (alone, seconds) = (self.charts.len() == 1, self.seconds);
        let (mut dropped, mut removed) = (None, None);
        for (number, traces) in self.charts.iter_mut().enumerate() {
            let corner = room.min + egui::vec2((number % across) as f32 * each.x, (number / across) as f32 * each.y);
            let place = Rect::from_min_size(corner, each).shrink(3.0);
            ui.scope_builder(UiBuilder::new().max_rect(place).id_salt(number), |ui| {
                let (chart, landed) = ui.dnd_drop_zone::<Trace, bool>(egui::Frame::canvas(ui.style()), |ui| {
                    ui.set_min_size(ui.available_size());
                    let remove = names(ui, traces, recorded, alone);
                    lines(ui, traces, recorded, now - seconds, now);
                    remove
                });
                if let Some(trace) = landed {
                    dropped = Some((number, Trace::clone(&trace)));
                }
                if chart.inner {
                    removed = Some(number);
                }
            });
        }
        if let Some((number, trace)) = dropped {
            self.add(number, trace);
        }
        if let Some(number) = removed {
            self.charts.remove(number);
        }
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
    // A topic that is no output of a triq runtime has nobody to count for it.
    let topics = signals.topics();
    let others = topics.iter().filter(|topic| !outputs.contains(&topic.name)).map(|topic| {
        let carried = format!("{}{}", topic.message, if topic.slice { "[]" } else { "" });
        (topic.name.clone(), format!("a topic of another program: {carried}"), Vec::new(), true)
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
        ui.weak("Its sender does not say what is inside.");
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

/// At the top of a chart: the colour, name and newest value of each line,
/// with a button that takes the line off. True if the user wants the chart itself gone.
fn names(ui: &mut Ui, traces: &mut Vec<Trace>, recorded: Recorded<'_>, alone: bool) -> bool {
    let (mut taken_off, mut remove) = (None, false);
    // The button for the chart itself sits at the end of the first row.
    let mut end_of_row = |ui: &mut Ui, first: bool| {
        if first && !alone {
            let end = Layout::right_to_left(Align::Center);
            ui.with_layout(end, |ui| remove = ui.small_button("Remove chart").clicked());
        }
    };
    for (number, trace) in traces.iter().enumerate() {
        ui.horizontal(|ui| {
            let (dash, _) = ui.allocate_exact_size(egui::vec2(12.0, ui.spacing().interact_size.y), Sense::hover());
            let ink = color(number, ui.visuals().dark_mode);
            ui.painter().rect_filled(Rect::from_center_size(dash.center(), egui::vec2(12.0, 3.0)), 0.0, ink);
            let name = ui.label(trace.name());
            if let Trace::Heard { connection, .. } = trace {
                name.on_hover_text(format!("from {}", connection.from));
            }
            if let Some(value) = trace.latest(recorded) {
                ui.weak(written(value) + trace.unit());
            }
            if ui.small_button("×").on_hover_text("Take this line off the chart").clicked() {
                taken_off = Some(number);
            }
            end_of_row(ui, number == 0);
        });
    }
    if traces.is_empty() {
        ui.horizontal(|ui| {
            ui.weak("Drag a number here.");
            end_of_row(ui, true);
        });
    }
    if let Some(number) = taken_off {
        traces.remove(number);
    }
    remove
}

/// The lines of a chart, from the time `since` at its left edge to `until` at
/// its right. The chart reaches from the smallest value in view at the bottom
/// to the largest at the top.
fn lines(ui: &mut Ui, traces: &[Trace], recorded: Recorded<'_>, since: f64, until: f64) {
    let (whole, chart) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    // Under the lines, a row that says what time the left and right edges are.
    let (area, foot) = whole.split_top_bottom_at_y(whole.bottom() - ui.spacing().interact_size.y);
    let seconds = until - since;
    let lines: Vec<Vec<(f64, f64)>> = traces.iter().map(|trace| trace.line(recorded, since, until)).collect();
    let Some((low, high)) = extent(lines.iter().flatten().map(|(_, value)| *value)) else { return };

    // A line along the very top or bottom is drawn whole, not cut in half by the edge.
    let inside = area.shrink2(egui::vec2(0.0, 2.0));
    let place = |(at, value): &(f64, f64)| {
        let (across, up) = ((at - since) / seconds, (value - low) / (high - low));
        egui::pos2(egui::lerp(inside.x_range(), across as f32), egui::lerp(inside.bottom()..=inside.top(), up as f32))
    };
    let (painter, dark) = (ui.painter_at(area), ui.visuals().dark_mode);
    for (number, line) in lines.iter().enumerate() {
        let placed: Vec<Pos2> = line.iter().map(place).collect();
        painter.line(thinned(&placed), Stroke::new(2.0, color(number, dark)));
    }
    let corner = |ui: &mut Ui, within: Rect, layout: Layout, text: String| {
        ui.scope_builder(UiBuilder::new().max_rect(within).layout(layout), |ui| ui.weak(text));
    };
    corner(ui, area, Layout::top_down(Align::Min), written(high));
    corner(ui, area, Layout::bottom_up(Align::Min), written(low));
    corner(ui, foot, Layout::left_to_right(Align::Center), format!("{seconds} s ago"));
    corner(ui, foot, Layout::right_to_left(Align::Center), "now".to_owned());

    // Under the pointer: what each line was at that time, if it has a point within a second of it.
    if let Some(pointer) = chart.hover_pos().filter(|pointer| area.contains(*pointer)) {
        let at = since + f64::from((pointer.x - area.left()) / area.width()) * seconds;
        painter.vline(pointer.x, area.y_range(), Stroke::new(1.0, ui.visuals().weak_text_color()));
        chart.on_hover_ui_at_pointer(|ui| {
            for (trace, line) in traces.iter().zip(&lines) {
                let nearest = line.iter().min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()));
                if let Some((_, value)) = nearest.filter(|(time, _)| (time - at).abs() <= 1.0) {
                    ui.label(format!("{} {}{}", trace.name(), written(*value), trace.unit()));
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

/// The colour of the first, second, third line on a chart.
pub fn color(line: usize, dark: bool) -> Color32 {
    colors::shade(line, dark)
}
