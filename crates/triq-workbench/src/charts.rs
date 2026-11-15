//! The second tab: what connections did over time, drawn as charts.
//!
//! On the left is every connection with its numbers as they stand. A number
//! dragged onto a chart is drawn there as a line over time; several can share
//! a chart, and there can be several charts. The top and bottom of a chart
//! are always the largest and the smallest value in view.
use crate::colors;
use crate::history::{Connection, History, Stat};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Sense, Stroke, Ui, UiBuilder};

/// One line on a chart: one number of one connection, over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub connection: Connection,
    pub stat: Stat,
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

    /// Draw the list of connections and the charts.
    pub fn show(&mut self, ui: &mut Ui, history: &History) {
        egui::Panel::left("connections").default_size(300.0).show(ui, |ui| connections(ui, history));
        egui::CentralPanel::default().show(ui, |ui| self.grid(ui, history));
    }

    /// The charts, in rows of `across`, sharing the room there is.
    fn grid(&mut self, ui: &mut Ui, history: &History) {
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
                    let remove = names(ui, traces, history, alone);
                    lines(ui, traces, history, seconds);
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

/// Every connection with its numbers as they stand. Each number can be dragged onto a chart.
fn connections(ui: &mut Ui, history: &History) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.weak("Drag a number onto a chart to see it over time.");
        ui.separator();
        for connection in history.connections() {
            ui.vertical(|ui| {
                ui.strong(&connection.to);
                ui.weak(format!("from {}", connection.from));
                let newest = history.points(connection).last();
                for stat in Stat::ALL {
                    let trace = Trace { connection: connection.clone(), stat };
                    let id = egui::Id::unique(("number", &connection.to, &connection.from, stat.name()));
                    ui.dnd_drag_source(id, trace, |ui| {
                        ui.horizontal(|ui| {
                            // The values line up in a column of their own.
                            let name = ui.label(stat.name());
                            ui.add_space(64.0 - name.rect.width());
                            ui.weak(newest.map_or(String::new(), |point| written(stat.of(point)) + stat.unit()));
                        });
                    });
                }
            });
            ui.separator();
        }
        if history.connections().next().is_none() {
            ui.weak("Nothing is connected.");
        }
    });
}

/// At the top of a chart: the colour, name and newest value of each line,
/// with a button that takes the line off. True if the user wants the chart itself gone.
fn names(ui: &mut Ui, traces: &mut Vec<Trace>, history: &History, alone: bool) -> bool {
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
            let name = ui.label(format!("{} {}", trace.connection.to, trace.stat.name()));
            name.on_hover_text(format!("from {}", trace.connection.from));
            if let Some(point) = history.points(&trace.connection).last() {
                ui.weak(written(trace.stat.of(point)) + trace.stat.unit());
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

/// The lines of a chart, over the last `seconds`. The chart reaches from the
/// smallest value in view at the bottom to the largest at the top.
fn lines(ui: &mut Ui, traces: &[Trace], history: &History, seconds: f64) {
    let (whole, chart) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    // Under the lines, a row that says what time the left and right edges are.
    let (area, foot) = whole.split_top_bottom_at_y(whole.bottom() - ui.spacing().interact_size.y);
    let Some(newest) = history.newest() else { return };
    let since = newest - seconds;
    let in_view = |trace: &Trace| -> Vec<(f64, f64)> {
        let points = history.points(&trace.connection).filter(|point| point.at >= since);
        points.map(|point| (point.at, trace.stat.of(point))).collect()
    };
    let lines: Vec<Vec<(f64, f64)>> = traces.iter().map(in_view).collect();
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
                    let (name, stat) = (&trace.connection.to, trace.stat);
                    ui.label(format!("{name} {} {}{}", stat.name(), written(*value), stat.unit()));
                }
            }
            ui.weak(format!("{:.1} s ago", newest - at));
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
