//! A chart of numbers against each other: two numbers of one message, as a
//! curve on a plane, or three, as a curve in space.
//!
//! The newest point of a curve is drawn as a dot. Its trail, if that is asked
//! for, is the path the point took over the time the chart shows. Like every
//! number of a message, a curve has a past from when it was put on a chart.
use super::lines::written;
use super::pen::{self, Pen};
use super::view::{Frame, Lens, Point, View};
use super::{Carried, Kind, Recorded, carried};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Layout, Painter, PointerButton, Pos2, Rect, Role, Sense, Stroke, Ui, UiBuilder, WidgetInfo,
};
use serde::{Deserialize, Serialize};

/// How large the dot at the newest point of a curve is.
const DOT: f32 = 4.0;
/// How far the wheel is turned, in points of the screen, to make a chart e times the size.
const ZOOM: f32 = 200.0;

/// One curve on a chart: numbers of one message against each other.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Curve {
    /// The topic whose messages the numbers are inside.
    pub(super) topic: String,
    /// The numbers along x, y and z, where one was chosen.
    pub(super) axes: [Option<String>; 3],
    pub(super) pen: Pen,
    /// Whether the path its point took is drawn behind the point.
    trail: bool,
    /// Whether the chart keeps its newest point in the middle.
    follow: bool,
    /// Whether the axes say which of its numbers are along them.
    labelled: bool,
}

impl Curve {
    /// A curve of the numbers of a topic, with none along any axis yet. The chart it is put
    /// on gives it a colour.
    pub(super) fn new(topic: &str) -> Self {
        let (topic, axes) = (topic.to_owned(), [None, None, None]);
        Self { topic, axes, pen: Pen::default(), trail: false, follow: false, labelled: true }
    }

    /// The numbers along the first so many axes, if every one of them was chosen.
    fn numbers(&self, axes: usize) -> Option<Vec<&str>> {
        self.axes[..axes].iter().map(|axis| axis.as_deref()).collect()
    }

    /// Where it is now: its numbers as they stood in the newest message.
    fn newest(&self, recorded: Recorded<'_>, axes: usize) -> Option<Point> {
        let mut point = [0.0; 3];
        for (axis, number) in self.numbers(axes)?.into_iter().enumerate() {
            point[axis] = recorded.signals.latest(&self.topic, number)?;
        }
        Some(point)
    }

    /// The path it took between two times: when each message was sent, and where it was then.
    fn path(&self, recorded: Recorded<'_>, axes: usize, since: f64, until: f64) -> Vec<(f64, Point)> {
        let numbers = self.numbers(axes);
        numbers.map_or(Vec::new(), |numbers| recorded.signals.points(&self.topic, &numbers, since, until))
    }

    /// The box that says which number is along an axis, after the letter of the axis. It opens
    /// the numbers of the topic to choose from.
    ///
    /// A number is along one axis of a curve. One that is chosen for another axis trades
    /// places with the number that was there: so x and y are swapped in one go, and both keep
    /// their past, which a number that is on no chart for a moment would lose.
    fn choose(&mut self, ui: &mut Ui, axis: usize, recorded: Recorded<'_>) {
        let letter = ui.weak(["x", "y", "z"][axis]);
        let listed = recorded.signals.watched(&self.topic).map_or(Vec::new(), |watched| watched.numbers);
        let before = self.axes[axis].clone();
        let along = before.clone().unwrap_or_else(|| "choose".to_owned());
        let numbers = egui::ComboBox::from_id_salt(axis).selected_text(along).show_ui(ui, |ui| {
            for (number, _) in listed {
                ui.selectable_value(&mut self.axes[axis], Some(number.clone()), number);
            }
        });
        numbers.response.labelled_by(letter.id);
        let had_it = (0..self.axes.len()).find(|other| *other != axis && self.axes[*other] == self.axes[axis]);
        if let Some(other) = had_it.filter(|_| self.axes[axis] != before) {
            self.axes[other] = before;
        }
    }
}

/// Written over the chart of that number: a row for each curve. The colour is a button that
/// opens a picker, and the box beside it says whether the axes name the curve's numbers. The
/// curve is taken by the name of its topic to be put down on another chart of the kind, and a
/// box for each axis says which number is along it. Of the two buttons at the end, the first
/// opens how the curve is drawn, and the last takes it off.
pub(super) fn names(ui: &mut Ui, chart: usize, kind: Kind, curves: &mut Vec<Curve>, recorded: Recorded<'_>) {
    if curves.is_empty() {
        ui.weak(kind.hint());
        return;
    }
    let (mut taken_off, mut followed) = (None, None);
    for (number, curve) in curves.iter_mut().enumerate() {
        // A row is known by the colour its curve was given: no two on a chart were given the same.
        // On a chart that is too narrow for all of it, it goes on in the next line.
        let row = ui.push_id(("curve", curve.pen.shade), |ui| {
            ui.horizontal_wrapped(|ui| {
                curve.pen.well(ui);
                ui.checkbox(&mut curve.labelled, "")
                    .on_hover_text("Say on the axes which of its numbers are along them");
                let taken = Carried::Line { chart, shade: curve.pen.shade };
                let name = carried(ui, ui.scope_id().with("name"), taken, |ui| ui.label(&curve.topic));
                name.on_hover_text("Drag it onto another chart of this kind to move it there");
                (0..kind.axes()).for_each(|axis| curve.choose(ui, axis, recorded));
                pen::more(ui, |ui| {
                    ui.checkbox(&mut curve.trail, "Draw its trail")
                        .on_hover_text("The path its point took over the time the chart shows");
                    curve.pen.fading(ui);
                    if ui.checkbox(&mut curve.follow, "Keep its newest point in the middle").changed() && curve.follow {
                        followed = Some(number);
                    }
                });
                ui.small_button("×").on_hover_text("Take this curve off the chart").clicked()
            })
        });
        if row.inner.inner {
            taken_off = Some(number);
        }
    }
    // A chart has one middle: it keeps the point of one curve there at most.
    if let Some(followed) = followed {
        curves.iter_mut().enumerate().for_each(|(number, curve)| curve.follow = number == followed);
    }
    if let Some(number) = taken_off {
        curves.remove(number);
    }
}

/// The curves of the chart of that name. Each is drawn as a dot where it is now and, if its
/// trail is asked for, the path it took from the time `since` to the present, `now`.
///
/// The chart shows all of that, edge to edge, until it is zoomed with the wheel or moved by
/// hand, and from then on what it was left at: that is `view`. On a plane, a drag moves the
/// chart. In space it turns the chart about its middle, and moves it with Shift held or with
/// the other button of the mouse.
pub(super) fn draw(
    ui: &mut Ui,
    (name, kind): (&str, Kind),
    curves: &mut [Curve],
    view: &mut View,
    recorded: Recorded<'_>,
    (since, now): (f64, f64),
) {
    let (whole, chart) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
    // The drawing is called by the chart's name, for whoever cannot see it.
    chart.widget_info(|| WidgetInfo::labeled(Role::Canvas, true, name));
    // Under the curves, a row that says how far the chart reaches.
    let (area, foot) = whole.split_top_bottom_at_y(whole.bottom() - ui.spacing().interact_size.y);
    let axes = kind.axes();
    let path = |curve: &Curve| if curve.trail { curve.path(recorded, axes, since, now) } else { Vec::new() };
    let paths: Vec<Vec<(f64, Point)>> = curves.iter().map(path).collect();
    let newest: Vec<Option<Point>> = curves.iter().map(|curve| curve.newest(recorded, axes)).collect();
    let followed = curves.iter().zip(&newest).find_map(|(curve, newest)| newest.filter(|_| curve.follow));
    let shown = paths.iter().flatten().map(|(_, point)| *point).chain(newest.iter().flatten().copied());
    let Some(all) = Frame::around(shown, followed) else { return };
    // What was set by hand stays as it was left, about the point that is followed if one is.
    let frame = match (view.held, followed) {
        (Some(held), Some(middle)) => Frame { middle, ..held },
        (held, _) => held.unwrap_or(all),
    };
    view.held = view.held.map(|_| frame);

    // A dot on the very edge of what is shown is drawn whole, not cut in half.
    let room = area.shrink(DOT + 2.0);
    let lens = match kind {
        Kind::Space => Lens::space(frame, room, view.equal, view.from),
        _ => Lens::flat(frame, room, view.equal),
    };
    // What is said of the axes is under the curves, which are drawn over it.
    let (painter, dark) = (ui.painter_at(area), ui.visuals().dark_mode);
    match kind {
        Kind::Space => boxed(ui, &painter, (&lens, frame), curves, (area, foot)),
        _ => edged(ui, &lens, curves, (area, foot)),
    }
    for ((curve, path), newest) in curves.iter().zip(&paths).zip(&newest) {
        // A point is as old as it is behind the newest of the path, in the time the chart shows.
        let latest = path.last().map_or(now, |(at, _)| *at);
        let aged = |(at, point): &(f64, Point)| (lens.at(*point), ((latest - at) / (now - since)) as f32);
        let placed: Vec<(Pos2, f32)> = path.iter().map(aged).collect();
        curve.pen.draw(&painter, &sparse(&placed), dark);
        if let Some(newest) = newest {
            painter.circle_filled(lens.at(*newest), DOT, curve.pen.color(dark));
        }
    }

    // Moved with the pointer, the chart lets go of the point it kept in its middle. The wheel
    // zooms, about the middle.
    let (by, other) = (chart.drag_delta(), chart.dragged_by(PointerButton::Secondary));
    let moves = kind != Kind::Space || other || ui.input(|input| input.modifiers.shift);
    if chart.dragged() && by != egui::Vec2::ZERO && moves {
        view.held = Some(lens.moved(by));
        curves.iter_mut().for_each(|curve| curve.follow = false);
    } else if chart.dragged() {
        view.turn(by);
    }
    let zoom = ui.input(|input| input.zoom_delta() * (input.smooth_scroll_delta.y / ZOOM).exp());
    if chart.contains_pointer() && zoom != 1.0 {
        view.held = Some(view.held.unwrap_or(frame).zoomed(f64::from(zoom)));
    }
}

/// What the edges of a chart on a plane say: the values they are at, and which numbers are
/// along the axes. x is named in the middle under the chart, and y in the middle of its right edge.
fn edged(ui: &mut Ui, lens: &Lens, curves: &[Curve], (area, foot): (Rect, Rect)) {
    let [(left, right), (bottom, top)] = lens.edges();
    let corner = |ui: &mut Ui, within: Rect, layout: Layout, value: f64| {
        ui.scope_builder(UiBuilder::new().max_rect(within).layout(layout), |ui| ui.weak(written(value)));
    };
    corner(ui, area, Layout::top_down(Align::Max), top);
    corner(ui, area, Layout::bottom_up(Align::Max), bottom);
    corner(ui, foot, Layout::left_to_right(Align::Center), left);
    corner(ui, foot, Layout::right_to_left(Align::Center), right);
    let (across, up) = (along(ui, curves, 0, None), along(ui, curves, 1, None));
    ui.put(Rect::from_center_size(foot.center(), across.size()), egui::Label::new(across));
    let beside = area.right_center() - up.size() * egui::vec2(1.0, 0.5);
    ui.put(Rect::from_min_size(beside, up.size()), egui::Label::new(up));
}

/// What a chart in space says of its axes. The part of space that it reaches across is drawn
/// as a box, turned as the chart is. Four edges of the box run along each axis: beside the
/// one that is furthest out on the screen is written which axis it is, and which numbers are
/// along it. Under the chart is how far it reaches along each.
fn boxed(ui: &mut Ui, painter: &Painter, (lens, frame): (&Lens, Frame), curves: &[Curve], (area, foot): (Rect, Rect)) {
    // A corner is at the far end of some axes and at the near end of the others: a bit for each.
    let end = |far: usize, axis: usize| if far >> axis & 1 == 1 { frame.reach(axis).1 } else { frame.reach(axis).0 };
    let corner = |far: usize| lens.at(std::array::from_fn(|axis| end(far, axis)));
    let (faint, middle) = (Stroke::new(1.0, ui.visuals().weak_text_color()), lens.at(frame.middle));
    let mut reaches = Vec::new();
    for (axis, letter) in ["x", "y", "z"].into_iter().enumerate() {
        // An edge along this axis leaves each corner that is at its near end.
        let edges: Vec<[Pos2; 2]> =
            (0..8).filter(|near| near >> axis & 1 == 0).map(|near| [corner(near), corner(near | 1 << axis)]).collect();
        for edge in &edges {
            painter.line_segment(*edge, faint);
        }
        // How far the middle of an edge is out from the middle of the box, on the screen.
        let out = |edge: &[Pos2; 2]| edge[0].lerp(edge[1], 0.5) - middle;
        if let Some(outermost) = edges.iter().map(out).max_by(|a, b| a.length_sq().total_cmp(&b.length_sq())) {
            // Clear of the edge, whichever way that is, and all of it on the chart.
            let (names, way) = (along(ui, curves, axis, Some(letter)), outermost.normalized());
            let clear = 8.0 + (way.x * names.size().x).abs() / 2.0 + (way.y * names.size().y).abs() / 2.0;
            let (at, within) = (middle + outermost + way * clear, area.shrink2(names.size() / 2.0));
            ui.put(Rect::from_center_size(at.clamp(within.min, within.max), names.size()), egui::Label::new(names));
        }
        let (least, most) = frame.reach(axis);
        reaches.push(format!("{letter} {} to {}", written(least), written(most)));
    }
    let along_foot = UiBuilder::new().max_rect(foot).layout(Layout::left_to_right(Align::Center));
    ui.scope_builder(along_foot, |ui| ui.weak(reaches.join("   ")));
}

/// Which numbers are along an axis, each in the colour of its curve: of the curves whose
/// numbers the axes are to name. In space, the letter of the axis is written before them.
fn along(ui: &Ui, curves: &[Curve], axis: usize, letter: Option<&str>) -> std::sync::Arc<egui::Galley> {
    let (font, dark) = (egui::TextStyle::Body.resolve(ui.style()), ui.visuals().dark_mode);
    let inked = |color| TextFormat { font_id: font.clone(), color, ..Default::default() };
    let mut names = LayoutJob::default();
    if let Some(letter) = letter {
        names.append(letter, 0.0, inked(ui.visuals().weak_text_color()));
    }
    for curve in curves.iter().filter(|curve| curve.labelled) {
        if let Some(number) = &curve.axes[axis] {
            let apart = if names.is_empty() { "" } else { "  " };
            names.append(&format!("{apart}{number}"), 0.0, inked(curve.pen.color(dark)));
        }
    }
    ui.painter().layout_job(names)
}

/// A trail as it will be drawn, without the points that cannot be told from the one before
/// them. Each comes with how old it is. A point is kept if it is a point of the screen or
/// more from the last that was kept, and the newest is kept wherever it is: a trail of an
/// hour of messages has far more points than can be seen.
pub fn sparse(trail: &[(Pos2, f32)]) -> Vec<(Pos2, f32)> {
    let mut kept: Vec<(Pos2, f32)> = Vec::new();
    for (number, point) in trail.iter().enumerate() {
        let apart = kept.last().is_none_or(|last| last.0.distance_sq(point.0) >= 1.0);
        if apart || number + 1 == trail.len() {
            kept.push(*point);
        }
    }
    kept
}
