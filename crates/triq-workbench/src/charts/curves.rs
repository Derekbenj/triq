//! A chart of numbers against each other: two numbers of one message, as a
//! curve on a plane, or three, as a curve in space.
//!
//! The newest point of a curve is drawn as a dot. Its trail, if that is asked
//! for, is the path the point took over the time the chart shows. Like every
//! number of a message, a curve has a past from when it was put on a chart.
use super::lines::written;
use super::pen::{self, Pen};
use super::view::{Frame, Lens, Point, View, clipped, ground_reach, marks, round_step, tick};
use super::{Carried, Kind, Recorded, carried, held_over, mark};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Color32, Layout, Painter, PointerButton, Pos2, Rect, Role, Sense, Stroke, Ui, UiBuilder, WidgetInfo,
};
use serde::{Deserialize, Serialize};

/// How large the dot at the newest point of a curve is.
const DOT: f32 = 4.0;
/// How far the wheel is turned, in points of the screen, to make a chart e times the size.
const ZOOM: f32 = 200.0;
/// How far apart the lines of a grid are on the screen, at the least, seen from straight above.
const SPACING: f64 = 60.0;
/// What the axes are called.
const LETTERS: [&str; 3] = ["x", "y", "z"];
/// How wide the axes at the origin of a chart in space are drawn, and in which colours: red,
/// green and blue for x, y and z, as is the custom.
const AXIS: f32 = 1.5;
const AXES: [Color32; 3] =
    [Color32::from_rgb(214, 80, 80), Color32::from_rgb(70, 170, 90), Color32::from_rgb(80, 120, 220)];

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
        let letter = ui.weak(LETTERS[axis]);
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
/// curve is taken by the name of its topic to be put down on another chart of the kind, or on
/// another row here: it takes that row's place, and the curves are drawn in the order of their
/// rows, the last over the others. A box for each axis says which number is along it. Of the
/// three buttons at the end, the eye hides the curve, the next opens how it is drawn, and the
/// last takes it off.
pub(super) fn names(ui: &mut Ui, chart: usize, kind: Kind, curves: &mut Vec<Curve>, recorded: Recorded<'_>) {
    if curves.is_empty() {
        ui.weak(kind.hint());
        return;
    }
    let order: Vec<usize> = curves.iter().map(|curve| curve.pen.shade).collect();
    let (mut taken_off, mut followed, mut put_down) = (None, None, None);
    for (number, curve) in curves.iter_mut().enumerate() {
        // A row is known by the colour its curve was given: no two on a chart were given the same.
        // On a chart that is too narrow for all of it, it goes on in the next line.
        let row = ui.push_id(("curve", curve.pen.shade), |ui| {
            ui.horizontal_wrapped(|ui| {
                curve.pen.well(ui);
                ui.checkbox(&mut curve.labelled, "")
                    .on_hover_text("Say on the axes which of its numbers are along them");
                let taken = Carried::Line { chart, shade: curve.pen.shade };
                // A curve that is hidden is named faintly.
                let named = |ui: &mut Ui| if curve.pen.hidden { ui.weak(&curve.topic) } else { ui.label(&curve.topic) };
                let name = carried(ui, ui.scope_id().with("name"), taken, named);
                name.on_hover_text(
                    "Drag it onto another chart of this kind to move it there, or onto another name here",
                );
                (0..kind.axes()).for_each(|axis| curve.choose(ui, axis, recorded));
                curve.pen.eye(ui);
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
        // Another curve of the chart that is held over the row says where it will go, and goes there.
        let held = held_over(ui, chart, row.response.rect).filter(|(shade, _)| *shade != curve.pen.shade);
        if let Some((from, let_go)) =
            held.and_then(|(shade, let_go)| Some((order.iter().position(|given| *given == shade)?, let_go)))
        {
            mark(ui, row.response.rect, from > number);
            put_down = let_go.then_some((from, number));
        }
    }
    // A chart has one middle: it keeps the point of one curve there at most.
    if let Some(followed) = followed {
        curves.iter_mut().enumerate().for_each(|(number, curve)| curve.follow = number == followed);
    }
    if let Some((from, to)) = put_down {
        let curve = curves.remove(from);
        curves.insert(to, curve);
    } else if let Some(number) = taken_off {
        curves.remove(number);
    }
}

/// The curves of the chart of that name. Each is drawn as a dot where it is now and, if its
/// trail is asked for, the path it took from the time `since` to the present, `now`.
///
/// The chart shows all of that when it first has anything to show, and from then on what it
/// was left at: that is `view`. It stays where it is, and a curve moves over it, unless it is
/// told to keep the curve in its middle. The wheel zooms. On a plane, a drag moves the chart.
/// In space it turns the chart about its middle, and moves it with Shift held or with the
/// other button of the mouse.
///
/// A chart does not lose sight of where a curve is now. By hand it goes no nearer and no
/// further than where it still shows that, and when the curve itself goes where the chart
/// does not reach, the chart is seen from just as much further away as it takes.
///
/// Under the curves is a grid, whose lines stay where they are while the chart moves over
/// them. In space it is the ground, and a curve stands on it: see [`ground`] and [`stand`].
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
    // Under the curves, a row that says which numbers are along the axes.
    let (area, foot) = whole.split_top_bottom_at_y(whole.bottom() - ui.spacing().interact_size.y);
    // The two axes that the grid is along, and the third. In space that is up, and 0 on the ground.
    let (axes, lying) = (kind.axes(), if kind == Kind::Space { view.ground.axes() } else { [0, 1, 2] });
    let grounded = |mut point: Point| {
        point[lying[2]] = 0.0;
        point
    };
    // A curve that is hidden has no points here: it is not drawn, and the chart does not reach for it.
    let trailing = |curve: &&Curve| curve.trail && !curve.pen.hidden;
    let path =
        |curve: &Curve| Some(curve).filter(trailing).map_or(Vec::new(), |curve| curve.path(recorded, axes, since, now));
    let paths: Vec<Vec<(f64, Point)>> = curves.iter().map(path).collect();
    let newest = |curve: &Curve| curve.newest(recorded, axes).filter(|_| !curve.pen.hidden);
    let newest: Vec<Option<Point>> = curves.iter().map(newest).collect();
    let followed = curves.iter().zip(&newest).find_map(|(curve, newest)| newest.filter(|_| curve.follow));
    let points = || paths.iter().flatten().map(|(_, point)| *point).chain(newest.iter().flatten().copied());
    if points().next().is_none() {
        return;
    }
    // All there is to show. In space that is also where on the ground each curve stands, and
    // where every axis is 0: what places are told by.
    let stood = newest.iter().flatten().map(|point| grounded(*point)).chain([[0.0; 3]]);
    let all = Frame::around(points().chain(stood.filter(|_| kind == Kind::Space)), followed);
    let Some(kept) = view.held.or(all) else { return };

    // A dot on the very edge of what is shown is drawn whole, not cut in half.
    let (room, looked) = (area.shrink(DOT + 2.0), *view);
    let lens = |frame: Frame| match kind {
        Kind::Space => Lens::space(frame, room, looked.equal, looked.from, looked.ground),
        _ => Lens::flat(frame, room, looked.equal),
    };
    // How far out a chart that shows some part draws where the curves are now: beyond 1, it has lost sight of one.
    let out = |frame: Frame| newest.iter().flatten().map(|point| lens(frame).out(*point)).fold(0.0, f64::max);
    let frame = match followed {
        // About the point that is followed, if one is, as much is shown as before.
        Some(middle) => Frame { middle, ..kept },
        None => kept.zoomed(1.0 / out(kept).max(1.0)),
    };
    // By hand: moved with the pointer, the chart lets go of the point it kept in its middle.
    // It moves as far as it still shows where the curves are: across and up, or else one of the two.
    let (by, other) = (chart.drag_delta(), chart.dragged_by(PointerButton::Secondary));
    let moves = kind != Kind::Space || other || ui.input(|input| input.modifiers.shift);
    view.held = Some(frame);
    if chart.dragged() && by != egui::Vec2::ZERO && moves {
        let tried = [by, egui::vec2(by.x, 0.0), egui::vec2(0.0, by.y)].map(|by| lens(frame).moved(by));
        view.held = tried.into_iter().find(|moved| out(*moved) <= 1.0).or(view.held);
        curves.iter_mut().for_each(|curve| curve.follow = false);
    } else if chart.dragged() {
        view.turn(by);
    }
    // The wheel zooms about the middle: no nearer than where a curve would be lost sight of.
    let zoom = ui.input(|input| input.zoom_delta() * (input.smooth_scroll_delta.y / ZOOM).exp());
    if let Some(shown) = view.held.filter(|_| chart.contains_pointer() && zoom != 1.0) {
        view.held = Some(shown.zoomed(f64::from(zoom).min(1.0 / out(shown))));
    }

    // What is said of the axes is under the curves, which are drawn over it.
    let (lens, painter, dark) = (lens(frame), ui.painter_at(area), ui.visuals().dark_mode);
    match kind {
        Kind::Space => {
            ground(ui, &painter, &lens, (lying, ground_reach(points())), area);
            origin(ui, &painter, &lens, lying[2], area);
        }
        _ => grid(ui, &painter, &lens, area),
    }
    ui.scope_builder(UiBuilder::new().max_rect(foot).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        for axis in 0..axes {
            ui.add(egui::Label::new(along(ui, curves, axis)));
        }
    });
    for ((curve, path), newest) in curves.iter().zip(&paths).zip(&newest) {
        // A point is as old as it is behind the newest of the path, in the time the chart shows.
        let latest = path.last().map_or(now, |(at, _)| *at);
        let aged = |(at, point): &(f64, Point)| (lens.at(*point), ((latest - at) / (now - since)) as f32);
        let placed: Vec<(Pos2, f32)> = path.iter().map(aged).collect();
        curve.pen.draw(&painter, &sparse(&placed), dark);
        let Some(newest) = *newest else { continue };
        if kind == Kind::Space {
            stand(ui, &painter, &lens, (newest, grounded(newest), lying[2]), (area, curve.pen.color(dark)));
        }
        painter.circle_filled(lens.at(newest), DOT, curve.pen.color(dark));
    }
}

/// The grid under the curves of a chart on a plane: lines at round values of x and y, across
/// the whole chart. The lines are where their values are, so the chart moves over them when
/// it follows a curve. Each says its value where it leaves the chart: those of x at the
/// bottom, and those of y at the right. Where an axis is 0, its line is stronger.
fn grid(ui: &mut Ui, painter: &Painter, lens: &Lens, area: Rect) {
    // What the chart shows: the values that are drawn in two opposite corners.
    let corners = [area.left_bottom(), area.right_top()].map(|corner| lens.on_plane(corner, [0, 1, 2], 0.0));
    let [Some(least), Some(most)] = corners else { return };
    let (faint, font) = (ui.visuals().weak_text_color(), egui::TextStyle::Small.resolve(ui.style()));
    for (axis, other) in [(0, 1), (1, 0)] {
        let step = round_step(SPACING / lens.scale(axis));
        for value in marks(least[axis], most[axis], step) {
            let end = |far: f64| {
                let mut point = [0.0; 3];
                (point[axis], point[other]) = (value, far);
                lens.at(point)
            };
            // From the top to the bottom, or from the left to the right: the value is written at the end.
            let (from, to) = (end(most[other]), end(least[other]));
            let (from, to) = if axis == 0 { (from, to) } else { (to, from) };
            let strength = if value == 0.0 { 0.7 } else { 0.25 };
            painter.line_segment([from, to], Stroke::new(1.0, faint.gamma_multiply(strength)));
            let written = painter.layout_no_wrap(tick(value, step), font.clone(), faint);
            let within = area.shrink2(written.size() / 2.0 + egui::vec2(2.0, 1.0));
            ui.put(Rect::from_center_size(to.clamp(within.min, within.max), written.size()), egui::Label::new(written));
        }
    }
}

/// The ground of a chart in space: the plane where the axis that is up is 0, as a grid. It is
/// a part of the world: it reaches `reach` from where every axis is 0 along each of its two
/// axes, wherever the chart looks, and its lines are at round values. A curve moves over it,
/// and a chart that follows a curve moves over it with the curve.
///
/// The values are written along the axes: those of each axis along the line where the other
/// is 0, with the letter of the axis after the last of them. When the chart does not show
/// that line, they are written along the line nearest to it that the chart shows, a little in
/// from the edge. Where an axis is 0, its line is stronger.
fn ground(ui: &mut Ui, painter: &Painter, lens: &Lens, ([along, beside, up], reach): ([usize; 3], f64), area: Rect) {
    // What of the ground the chart shows: the points of it that are drawn in the corners. Seen
    // from its own level the ground is a line, and nothing of it is drawn.
    let corners = [area.left_top(), area.right_top(), area.right_bottom(), area.left_bottom()];
    let on_ground = |corner: &Pos2| lens.on_plane(*corner, [along, beside, up], 0.0);
    let Some(corners) = corners.iter().map(on_ground).collect::<Option<Vec<Point>>>() else { return };
    let shown = |axis: usize| {
        let values = corners.iter().map(|corner| corner[axis]);
        let (least, most) =
            values.fold((f64::INFINITY, f64::NEG_INFINITY), |(least, most), value| (least.min(value), most.max(value)));
        (least.max(-reach), most.min(reach))
    };
    let at = |axis: usize, value: f64, across: f64| {
        let mut point = [0.0; 3];
        (point[axis], point[along + beside - axis]) = (value, across);
        lens.at(point)
    };
    let (faint, font) = (ui.visuals().weak_text_color(), egui::TextStyle::Small.resolve(ui.style()));
    // The lines are no closer on the screen than the lines of any grid: closer in data, the
    // more an axis points towards the eye. With one scale for both axes they make squares,
    // as large as the axis that is seen more from the side asks for.
    let steps = [along, beside].map(|axis| round_step(SPACING / lens.on_screen(axis)));
    let steps = if lens.scale(along) == lens.scale(beside) { [steps[0].min(steps[1]); 2] } else { steps };
    // Where values are written already: none is written over another.
    let mut taken: Vec<Rect> = Vec::new();
    for (axis, step, across) in [(along, steps[0], steps[1]), (beside, steps[1], steps[0])] {
        let ((least, most), (from, to)) = (shown(axis), shown(along + beside - axis));
        if least > most || from > to || !step.is_finite() || !across.is_finite() {
            continue;
        }
        // The edges of the ground, where the chart shows them.
        for edge in [-reach, reach].map(|end| [at(axis, end, from), at(axis, end, to)]) {
            if let Some(seen) = clipped(edge, area) {
                painter.line_segment(seen, Stroke::new(1.0, faint.gamma_multiply(0.5)));
            }
        }
        // The values of this axis are written along the line where the other is 0, or as near
        // to that as the chart shows, on the side of the line that is lower on the screen.
        let inset = (to - from) / 10.0;
        let ruler = (0f64.clamp(from + inset, to - inset) / across).round() * across;
        let way = (at(axis, most, ruler) - at(axis, least, ruler)).normalized().rot90();
        let below = if way.y < 0.0 { -way } else { way };
        let mut write = |ui: &mut Ui, written: String, ink: Color32, value: f64| {
            let written = ui.painter().layout_no_wrap(written, font.clone(), ink);
            let size = written.size();
            let middle = at(axis, value, ruler)
                + below * (4.0 + (below.x * size.x).abs() / 2.0 + (below.y * size.y).abs() / 2.0);
            let place = Rect::from_center_size(middle, size);
            if area.contains_rect(place) && !taken.iter().any(|other| other.expand(2.0).intersects(place)) {
                ui.put(place, egui::Label::new(written));
                taken.push(place);
            }
        };
        let values = marks(least, most, step);
        for value in &values {
            let strength = if *value == 0.0 { 0.7 } else { 0.25 };
            if let Some(seen) = clipped([at(axis, *value, from), at(axis, *value, to)], area) {
                painter.line_segment(seen, Stroke::new(1.0, faint.gamma_multiply(strength)));
            }
            write(ui, tick(*value, step), faint, *value);
        }
        if let Some(last) = values.last() {
            write(ui, LETTERS[axis].to_owned(), AXES[axis], last + step / 2.0);
        }
    }
}

/// Where every axis of a chart in space is 0: from there, a line along each axis, as long as
/// the lines of the ground are apart. The axis that is up has its letter at the end: those of
/// the other two are written with their values, on the ground.
fn origin(ui: &Ui, painter: &Painter, lens: &Lens, up: usize, area: Rect) {
    for (axis, letter) in LETTERS.into_iter().enumerate() {
        let mut end = [0.0; 3];
        end[axis] = round_step(SPACING / lens.scale(axis));
        // Only if it is on the chart: seen from very near, it may be a long way off.
        let Some([from, to]) = clipped([lens.at([0.0; 3]), lens.at(end)], area) else { continue };
        painter.line_segment([from, to], Stroke::new(AXIS, AXES[axis]));
        if axis == up {
            let font = egui::TextStyle::Small.resolve(ui.style());
            painter.text(to + (to - from).normalized() * 8.0, egui::Align2::CENTER_CENTER, letter, font, AXES[axis]);
        }
    }
}

/// Where a curve stands, in space: a thin line from where it is now straight down, or up, to
/// the ground, and a ring there. Beside the line is how far it is: the value of the axis that
/// is up. It is what tells a point that is near and low from one that is far and high.
fn stand(
    ui: &mut Ui,
    painter: &Painter,
    lens: &Lens,
    (point, foot, up): (Point, Point, usize),
    (area, ink): (Rect, Color32),
) {
    let (top, bottom, thin) = (lens.at(point), lens.at(foot), ink.gamma_multiply(0.6));
    // As much of the line as is on the chart: seen from very near, the ground may be a long way off.
    if let Some(seen) = clipped([top, bottom], area) {
        painter.line_segment(seen, Stroke::new(1.0, thin));
    }
    painter.circle_stroke(bottom, DOT - 1.0, Stroke::new(1.0, thin));
    let font = egui::TextStyle::Small.resolve(ui.style());
    let height = painter.layout_no_wrap(format!("{} {}", LETTERS[up], written(point[up])), font, thin);
    let beside = top.lerp(bottom, 0.5) + egui::vec2(height.size().x / 2.0 + 6.0, 0.0);
    if area.contains(beside) {
        ui.put(Rect::from_center_size(beside, height.size()), egui::Label::new(height));
    }
}

/// An axis as the row under a chart names it: its letter, and the numbers along it, each in
/// the colour of its curve. Only of the curves whose numbers the axes are to name.
fn along(ui: &Ui, curves: &[Curve], axis: usize) -> std::sync::Arc<egui::Galley> {
    let (font, dark) = (egui::TextStyle::Body.resolve(ui.style()), ui.visuals().dark_mode);
    let inked = |color| TextFormat { font_id: font.clone(), color, ..Default::default() };
    let mut names = LayoutJob::default();
    names.append(LETTERS[axis], 0.0, inked(ui.visuals().weak_text_color()));
    for curve in curves.iter().filter(|curve| curve.labelled) {
        if let Some(number) = &curve.axes[axis] {
            names.append(&format!("  {number}"), 0.0, inked(curve.pen.color(dark)));
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
