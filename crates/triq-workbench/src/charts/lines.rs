//! A chart of numbers over time: the lines on it, their names, and how they are drawn.
use super::pen::{self, Pen};
use super::{Carried, Kind, Recorded, carried};
use crate::history::{Connection, Stat};
use eframe::egui::{self, Align, Layout, Pos2, Rect, Role, Sense, Stroke, Ui, UiBuilder, WidgetInfo};
use serde::{Deserialize, Serialize};

/// How far behind the present the lines of what runtimes report end, in
/// seconds. The machine is read ten times a second, so the newest point is up
/// to a tenth of a second old, and a little more by the time it has been
/// fetched. Ended this far back, such a line always has a point to run on to,
/// and slides along instead of jumping forward with every reading.
const BEHIND: f64 = 0.15;

/// What a line on a chart follows: a number, over time. It is known by what
/// it is called, so a chart that is kept from one run to the next draws it
/// again when something of those names is running.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub(super) fn word(&self) -> &str {
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
    pub(super) fn unit(&self) -> &'static str {
        match self {
            Trace::Heard { stat, .. } => stat.unit(),
            Trace::Sent { .. } => Stat::Rate.unit(),
            Trace::Inside { .. } => "",
        }
    }

    /// The number as it stands.
    pub(super) fn latest(&self, recorded: Recorded<'_>) -> Option<f64> {
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

/// One line on a chart: the number it follows, and what it is drawn with.
#[derive(Serialize, Deserialize)]
pub(super) struct Line {
    pub(super) trace: Trace,
    /// Kept as the line's own, as charts were kept before a line had more than a colour.
    #[serde(flatten)]
    pub(super) pen: Pen,
}

impl Line {
    /// A line that follows a number. The chart it is put on gives it a colour.
    pub(super) fn new(trace: Trace) -> Self {
        Self { trace, pen: Pen::default() }
    }
}

/// Written over the chart of that number: the colour, name and newest value of
/// each line. The colour is a button that opens a picker, and the line is taken
/// by its name to be put down on another chart. Of the two buttons after the
/// value, the first opens how the line is drawn, and the last takes it off.
pub(super) fn names(ui: &mut Ui, chart: usize, lines: &mut Vec<Line>, recorded: Recorded<'_>) {
    if lines.is_empty() {
        ui.weak(Kind::Time.hint());
        return;
    }
    let mut taken_off = None;
    for (number, line) in lines.iter_mut().enumerate() {
        // A row is known by what its line follows, and keeps its picker when the rows above it go.
        let row = ui.push_id(egui::Id::unique(&line.trace), |ui| {
            ui.horizontal(|ui| {
                line.pen.well(ui);
                let taken = Carried::Line { chart, shade: line.pen.shade };
                let name = carried(ui, ui.scope_id().with("name"), taken, |ui| ui.label(line.trace.name()));
                let name = name.on_hover_text("Drag it onto another chart to move it there");
                if let Trace::Heard { connection, .. } = &line.trace {
                    name.on_hover_text(format!("from {}", connection.from));
                }
                // A number may be on a chart before anything by its name is running.
                match line.trace.latest(recorded) {
                    Some(value) => ui.weak(written(value) + line.trace.unit()),
                    None => ui.weak("nothing yet"),
                };
                pen::more(ui, |ui| line.pen.fading(ui));
                ui.small_button("×").on_hover_text("Take this line off the chart").clicked()
            })
        });
        if row.inner.inner {
            taken_off = Some(number);
        }
    }
    if let Some(number) = taken_off {
        lines.remove(number);
    }
}

/// The lines of the chart of that name, from the time `since` at its left
/// edge to `until` at its right. The chart reaches from the smallest value in
/// view at the bottom to the largest at the top, and says at its right edge
/// what those are.
pub(super) fn lines(ui: &mut Ui, name: &str, drawn: &[Line], recorded: Recorded<'_>, since: f64, until: f64) {
    let (whole, chart) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    // The drawing is called by the chart's name, for whoever cannot see it.
    chart.widget_info(|| WidgetInfo::labeled(Role::Canvas, true, name));
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
    // A point is as old as it is far from the right edge, where the present is.
    let aged = |place: Pos2| (place, (inside.right() - place.x) / inside.width());
    for (line, points) in drawn.iter().zip(&lines) {
        let placed: Vec<Pos2> = points.iter().map(place).collect();
        let placed: Vec<(Pos2, f32)> = thinned(&placed).into_iter().map(aged).collect();
        line.pen.draw(&painter, &placed, dark);
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
