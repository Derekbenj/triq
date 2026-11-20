//! A chart of numbers against each other: two numbers of one message, as a
//! curve on a plane, or three, as a curve in space.
//!
//! The newest point of a curve is drawn as a dot. Its trail, if that is asked
//! for, is the path the point took over the time the chart shows. Like every
//! number of a message, a curve has a past from when it was put on a chart.
use super::lines::written;
use super::pen::{self, Pen};
use super::view::{Frame, Lens, Point, View, clipped, ground_reach, marks, round_step, tick};
use super::{Carried, Kind, Recorded, Trace, carried, held_over, mark, over};
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

    /// Put a number along an axis. A number is along one axis of a curve: if it was along
    /// another, the two trade places. So x and y are swapped in one go, and both keep their
    /// past, which a number that is on no chart for a moment would lose.
    fn put(&mut self, axis: usize, number: &str) {
        let before = self.axes[axis].replace(number.to_owned());
        let had_it = |other: &usize| *other != axis && self.axes[*other].as_deref() == Some(number);
        if let Some(other) = (0..self.axes.len()).find(had_it) {
            self.axes[other] = before;
        }
    }

    /// The slot of an axis, on a chart with so many axes: it says which number is along the
    /// axis. A number is put there by dragging it onto the slot: out of the list, with any
    /// that were chosen with it going along the axes after this one, or out of another slot
    /// of the curve, which is taken with its number.
    fn slot(&mut self, ui: &mut Ui, chart: usize, (axis, axes): (usize, usize)) {
        let (letter, along) = (LETTERS[axis], self.axes[axis].clone());
        let said = along.clone().unwrap_or_else(|| "drop a number".to_owned());
        // The slot is one piece: on a chart that is too narrow for the row, it goes to the next line whole.
        let framed = |ui: &mut Ui| {
            let (font, faint) = (egui::TextStyle::Body.resolve(ui.style()), ui.visuals().weak_text_color());
            let ink = if along.is_some() { ui.visuals().text_color() } else { faint };
            let mut words = LayoutJob::default();
            words.append(letter, 0.0, TextFormat { font_id: font.clone(), color: faint, ..Default::default() });
            words.append(&said, 6.0, TextFormat { font_id: font, color: ink, ..Default::default() });
            let words = ui.painter().layout_job(words);
            let (place, slot) = ui.allocate_exact_size(words.size() + egui::vec2(8.0, 2.0), Sense::hover());
            let edge = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().rect_stroke(place, 3, edge, egui::StrokeKind::Inside);
            ui.painter().galley(place.min + egui::vec2(4.0, 1.0), words, ink);
            slot
        };
        let slot = match along {
            Some(_) => {
                let taken = Carried::Slot { chart, shade: self.pen.shade, axis };
                carried(ui, ui.scope_id().with(("slot", axis)), taken, framed)
            }
            None => framed(ui),
        };
        slot.widget_info(|| {
            let mut slot = WidgetInfo::labeled(Role::Button, true, format!("{letter} axis"));
            slot.current_text_value = Some(said.clone());
            slot
        });
        slot.clone().on_hover_text("Drag a number of this topic here, out of the list or out of another of these");
        // What is held over the slot and has a place in it: numbers of the curve's topic, in
        // the order they come in, or the number of another slot of the curve.
        let Some((held, let_go)) = over(ui, slot.rect) else { return };
        let of_topic = |number: &Trace| match number {
            Trace::Inside { topic, number } if *topic == self.topic => Some(number.clone()),
            _ => None,
        };
        let (numbers, from) = match &*held {
            Carried::Numbers(numbers) => (numbers.iter().filter_map(of_topic).collect(), None),
            Carried::Slot { chart: on, shade, axis: from } if (*on, *shade) == (chart, self.pen.shade) => {
                (Vec::new(), Some(*from).filter(|from| *from != axis))
            }
            _ => (Vec::new(), None),
        };
        if numbers.is_empty() && from.is_none() {
            return;
        }
        let ready = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
        ui.painter().rect_stroke(slot.rect, 3, ready, egui::StrokeKind::Outside);
        if let_go {
            numbers.iter().zip(axis..axes).for_each(|(number, axis)| self.put(axis, number));
            if let Some(from) = from {
                self.axes.swap(from, axis);
            }
            // It was put down here, and is not put down on the chart as well.
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
    }
}

/// Written over the chart of that number: a row for each curve. The colour is a button that
/// opens a picker, and the box beside it says whether the axes name the curve's numbers. The
/// curve is taken by the name of its topic to be put down on another chart of the kind, or on
/// another row here: it takes that row's place, and the curves are drawn in the order of their
/// rows, the last over the others. A slot for each axis says which number is along it, and
/// takes another that is dragged onto it. Of the three buttons at the end, the eye hides the
/// curve, the next opens how it is drawn, and the last takes it off.
pub(super) fn names(ui: &mut Ui, chart: usize, kind: Kind, curves: &mut Vec<Curve>) {
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
                (0..kind.axes()).for_each(|axis| curve.slot(ui, chart, (axis, kind.axes())));
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
    let lens = |frame: Frame| {
        let lens = match kind {
            Kind::Space => Lens::space(frame, room, looked.equal, looked.from, looked.ground),
            _ => Lens::flat(frame, room, looked.equal),
        };
        lens.reversed(looked.reversed)
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
            let reach = ground_reach(points(), [lying[0], lying[1]], looked.equal);
            ground(ui, &painter, &lens, (lying, reach), area);
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
    // What the chart shows: the values that are drawn in two opposite corners. Which of the
    // two is the smaller goes by which way the axis runs.
    let corners = [area.left_bottom(), area.right_top()].map(|corner| lens.on_plane(corner, [0, 1, 2], 0.0));
    let [Some(one), Some(other)] = corners else { return };
    let shown = |axis: usize| (one[axis].min(other[axis]), one[axis].max(other[axis]));
    let (faint, font) = (ui.visuals().weak_text_color(), egui::TextStyle::Small.resolve(ui.style()));
    for (axis, across) in [(0, 1), (1, 0)] {
        let ((least, most), (from, to), step) = (shown(axis), shown(across), round_step(SPACING / lens.scale(axis)));
        for value in marks(least, most, step) {
            let end = |far: f64| {
                let mut point = [0.0; 3];
                (point[axis], point[across]) = (value, far);
                lens.at(point)
            };
            let ends = [end(from), end(to)];
            let strength = if value == 0.0 { 0.7 } else { 0.25 };
            painter.line_segment(ends, Stroke::new(1.0, faint.gamma_multiply(strength)));
            // The value is written at the bottom of a line that goes up, and at the right of one that goes across.
            let lower = if ends[0].y > ends[1].y { ends[0] } else { ends[1] };
            let at = if axis == 0 { lower } else { egui::pos2(ends[0].x.max(ends[1].x), ends[0].y) };
            let written = painter.layout_no_wrap(tick(value, step), font.clone(), faint);
            let within = area.shrink2(written.size() / 2.0 + egui::vec2(2.0, 1.0));
            ui.put(Rect::from_center_size(at.clamp(within.min, within.max), written.size()), egui::Label::new(written));
        }
    }
}

/// The ground of a chart in space: the plane where the axis that is up is 0, as a grid. It is
/// a part of the world: it reaches `reach` from where every axis is 0 along its two axes,
/// wherever the chart looks, and its lines are at round values. A curve moves over it,
/// and a chart that follows a curve moves over it with the curve.
///
/// The values are written along the axes: those of each axis along the line where the other
/// is 0, with the letter of the axis after the last of them. When the chart does not show
/// that line, they are written along the line nearest to it that the chart shows, a little in
/// from the edge. Where an axis is 0, its line is stronger.
fn ground(
    ui: &mut Ui,
    painter: &Painter,
    lens: &Lens,
    ([along, beside, up], reach): ([usize; 3], [f64; 2]),
    area: Rect,
) {
    // What of the ground the chart shows: the points of it that are drawn in the corners. Seen
    // from its own level the ground is a line, and nothing of it is drawn.
    let corners = [area.left_top(), area.right_top(), area.right_bottom(), area.left_bottom()];
    let on_ground = |corner: &Pos2| lens.on_plane(*corner, [along, beside, up], 0.0);
    let Some(corners) = corners.iter().map(on_ground).collect::<Option<Vec<Point>>>() else { return };
    let shown = |axis: usize| {
        let values = corners.iter().map(|corner| corner[axis]);
        let (least, most) =
            values.fold((f64::INFINITY, f64::NEG_INFINITY), |(least, most), value| (least.min(value), most.max(value)));
        let reach = if axis == along { reach[0] } else { reach[1] };
        ((least.max(-reach), most.min(reach)), reach)
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
        let (((least, most), reach), ((from, to), _)) = (shown(axis), shown(along + beside - axis));
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
        // Write something beside the line, where this axis is at a value: if there is room on the chart.
        let mut write = |ui: &mut Ui, written: String, ink: Color32, value: f64| {
            let written = ui.painter().layout_no_wrap(written, font.clone(), ink);
            let size = written.size();
            let middle = at(axis, value, ruler)
                + below * (4.0 + (below.x * size.x).abs() / 2.0 + (below.y * size.y).abs() / 2.0);
            let place = Rect::from_center_size(middle, size);
            let room = area.contains_rect(place) && !taken.iter().any(|other| other.expand(2.0).intersects(place));
            if room {
                ui.put(place, egui::Label::new(written));
                taken.push(place);
            }
            room
        };
        let mut written = Vec::new();
        for value in marks(least, most, step) {
            let strength = if value == 0.0 { 0.7 } else { 0.25 };
            if let Some(seen) = clipped([at(axis, value, from), at(axis, value, to)], area) {
                painter.line_segment(seen, Stroke::new(1.0, faint.gamma_multiply(strength)));
            }
            if write(ui, tick(value, step), faint, value) {
                written.push(value);
            }
        }
        // The letter of the axis goes after the last value that is written, or else before the first.
        let ends = [written.last().map(|last| last + step / 2.0), written.first().map(|first| first - step / 2.0)];
        let _ = ends.into_iter().flatten().any(|end| write(ui, LETTERS[axis].to_owned(), AXES[axis], end));
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
