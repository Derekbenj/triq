//! How a chart of numbers against each other is looked at: what part of its
//! points it shows, and where that puts a point on the screen.
//!
//! A chart shows all of its points until it is zoomed or moved by hand. From
//! then on it shows what it was left at, until it is told to show all again.
//! Zooming is about the middle of the chart, and so is turning a chart in space.
//!
//! In space, two of the axes are the ground and the third is up. Nothing is
//! drawn smaller for being further away: a unit is as long at the back as at
//! the front.
use eframe::egui::{Pos2, Rect, Vec2, vec2};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6};

/// How far a chart in space is turned for each point of the screen it is dragged, in radians.
const TURN: f32 = 0.01;
/// How much further a chart reaches than to a point it is made to show, so that the point is not on its very edge.
const SPARE: f64 = 1.1;
/// How many lines of a grid are drawn at the most. More than that could not be told apart.
const MOST: f64 = 200.0;

/// Which two axes of a chart in space are the ground. The third is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ground {
    #[default]
    Xy,
    Xz,
    Yz,
}

impl Ground {
    pub const ALL: [Ground; 3] = [Ground::Xy, Ground::Xz, Ground::Yz];

    /// The two axes along the ground and the one that is up, in the order in which the third
    /// is to the left of the first as seen from the second: so nothing is drawn mirrored.
    pub fn axes(self) -> [usize; 3] {
        match self {
            Ground::Xy => [0, 1, 2],
            Ground::Xz => [2, 0, 1],
            Ground::Yz => [1, 2, 0],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ground::Xy => "xy",
            Ground::Xz => "xz",
            Ground::Yz => "yz",
        }
    }
}

/// A point of a curve: its numbers along x, y and z. On a plane, z is 0.
pub type Point = [f64; 3];

/// How a chart is looked at, as far as that was said by hand.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct View {
    /// Whether a unit is as long along one axis as along another, so that shapes are as they are.
    pub(super) equal: bool,
    /// What part of its points the chart shows, once that was set by hand.
    pub(super) held: Option<Frame>,
    /// Where a chart in space is looked at from: how far round the upright axis, and from how
    /// far above the level, both in radians.
    pub(super) from: (f32, f32),
    /// Which two axes of a chart in space are the ground.
    pub(super) ground: Ground,
}

impl Default for View {
    /// A chart in space is first looked at from the front and a little to the right, from above.
    fn default() -> Self {
        Self { equal: false, held: None, from: (-FRAC_PI_3, FRAC_PI_6), ground: Ground::Xy }
    }
}

impl View {
    /// Turn a chart in space as it is dragged by so much on the screen: what faces the eye
    /// goes with the pointer. It is never turned further than to straight above or below.
    pub(super) fn turn(&mut self, by: Vec2) {
        self.from = (self.from.0 - by.x * TURN, (self.from.1 + by.y * TURN).clamp(-FRAC_PI_2, FRAC_PI_2));
    }
}

/// What part of its points a chart shows: the point in its middle, and how
/// much of each axis it reaches across.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub middle: Point,
    pub spans: Point,
}

impl Frame {
    /// The part that takes in all of some points. It is about the middle of them, or about
    /// `middle`: the point that a chart keeps in its middle. Along an axis that the points do
    /// not differ on, it reaches across 1. `None` without a point.
    pub fn around(points: impl Iterator<Item = Point>, middle: Option<Point>) -> Option<Frame> {
        let mut reach: Option<(Point, Point)> = None;
        for point in points {
            let (low, high) = reach.get_or_insert((point, point));
            for axis in 0..3 {
                (low[axis], high[axis]) = (low[axis].min(point[axis]), high[axis].max(point[axis]));
            }
        }
        let (low, high) = reach?;
        let middle = middle.unwrap_or_else(|| std::array::from_fn(|axis| (low[axis] + high[axis]) / 2.0));
        // As far to either side of the middle as the point that is furthest from it.
        let spans = std::array::from_fn(|axis| {
            let span = 2.0 * (high[axis] - middle[axis]).max(middle[axis] - low[axis]);
            if span > 0.0 { span } else { 1.0 }
        });
        Some(Frame { middle, spans })
    }

    /// The same middle from nearer by: at twice the size, half as much is shown.
    pub fn zoomed(self, by: f64) -> Frame {
        Frame { spans: self.spans.map(|span| span / by), ..self }
    }

    /// The same middle from far enough away to show a point as well, in the same shape and
    /// with a little to spare. As it is, if it reaches the point already.
    pub fn taking_in(self, point: Point) -> Frame {
        // How far out the point is along each axis, where 1 is as far as the frame reaches.
        let out = |axis: usize| 2.0 * (point[axis] - self.middle[axis]).abs() / self.spans[axis];
        let furthest = (0..3).map(out).fold(0.0, f64::max);
        if furthest <= 1.0 { self } else { self.zoomed(1.0 / (furthest * SPARE)) }
    }
}

/// A round distance for the lines of a grid to be apart: the least of 1, 2 and 5 times a
/// power of ten that is no less than `least`.
pub fn round_step(least: f64) -> f64 {
    let power = 10f64.powi(least.log10().floor() as i32);
    [1.0, 2.0, 5.0].into_iter().map(|times| times * power).find(|step| *step >= least).unwrap_or(10.0 * power)
}

/// The values that the lines of a grid are at: every multiple of `step` from `least` to
/// `most`. None at all, if those would be more lines than could be told apart.
pub fn marks(least: f64, most: f64, step: f64) -> Vec<f64> {
    let (first, last) = ((least / step).ceil(), (most / step).floor());
    if last - first > MOST {
        return Vec::new();
    }
    (first as i64..=last as i64).map(|times| times as f64 * step).collect()
}

/// A value of a grid as it is written beside its line: with as many decimals as the step has.
pub fn tick(value: f64, step: f64) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    // Nothing is written as minus nothing.
    format!("{:.decimals$}", value + 0.0)
}

/// The part of a line from one place to another that is in a room, in the direction the line
/// had. `None` if no part of it is.
pub fn clipped([from, to]: [Pos2; 2], room: Rect) -> Option<[Pos2; 2]> {
    let way = to - from;
    let (mut begin, mut end) = (0.0f32, 1.0f32);
    // Across and up in turn: how far along the line it comes into the room, and goes out of it.
    let sides =
        [(way.x, room.left() - from.x, room.right() - from.x), (way.y, room.top() - from.y, room.bottom() - from.y)];
    for (step, least, most) in sides {
        if step == 0.0 {
            if least > 0.0 || most < 0.0 {
                return None;
            }
        } else {
            let (one, other) = (least / step, most / step);
            (begin, end) = (begin.max(one.min(other)), end.min(one.max(other)));
        }
    }
    (begin <= end).then(|| [from + way * begin, from + way * end])
}

/// Where the points of a chart are on the screen.
pub struct Lens {
    frame: Frame,
    /// The room the chart has: the middle of what it shows is in the middle of it.
    room: Rect,
    /// For each axis: which way it points on the screen, and how many points of the screen
    /// one unit along it is.
    ways: [Vec2; 3],
    scales: [f64; 3],
}

impl Lens {
    /// A chart on a plane: x across the room and y up it. What the frame reaches across fills
    /// the room. With `equal`, a unit is as long across as up, and the chart shows more along
    /// one axis than the frame asks for, never less.
    pub fn flat(frame: Frame, room: Rect, equal: bool) -> Lens {
        let (across, up) = (f64::from(room.width()) / frame.spans[0], f64::from(room.height()) / frame.spans[1]);
        let scales = if equal { [across.min(up), across.min(up), 0.0] } else { [across, up, 0.0] };
        Lens { frame, room, ways: [vec2(1.0, 0.0), vec2(0.0, -1.0), Vec2::ZERO], scales }
    }

    /// A chart in space, looked at from so far round its upright axis, and from so far above
    /// the ground: both in radians. What the frame reaches across is a box, with each axis
    /// along an edge, that is turned about its middle and fits the room whichever way it is
    /// turned. With `equal`, a unit is as long along every axis, and the longest fills its edge.
    pub fn space(frame: Frame, room: Rect, equal: bool, (round, above): (f32, f32), ground: Ground) -> Lens {
        // The box is never wider on the screen than from one of its corners to the opposite one.
        let edge = f64::from(room.width().min(room.height())) / 3f64.sqrt();
        let scales = frame.spans.map(|span| edge / span);
        let scales = if equal { [scales.into_iter().fold(f64::INFINITY, f64::min); 3] } else { scales };
        // Which way each axis points on the screen, as seen from there: across, and up.
        let ((sin_round, cos_round), (sin_above, cos_above), [along, beside, up]) =
            (round.sin_cos(), above.sin_cos(), ground.axes());
        let mut ways = [Vec2::ZERO; 3];
        ways[along] = vec2(-sin_round, sin_above * cos_round);
        ways[beside] = vec2(cos_round, sin_above * sin_round);
        ways[up] = vec2(0.0, -cos_above);
        Lens { frame, room, ways, scales }
    }

    /// How many points of the screen a unit along an axis is, when the axis is seen from the side.
    pub fn scale(&self, axis: usize) -> f64 {
        self.scales[axis]
    }

    /// The point of a plane that is drawn at a place on the screen. The plane is where the
    /// third of three axes is at `level`, and the other two are along it. `None` if the plane
    /// is seen from its own level: it is a line then, and no place is one point of it.
    pub fn on_plane(&self, at: Pos2, [along, beside, up]: [usize; 3], level: f64) -> Option<Point> {
        let way = |axis: usize| {
            (f64::from(self.ways[axis].x) * self.scales[axis], f64::from(self.ways[axis].y) * self.scales[axis])
        };
        let ((ax, ay), (bx, by), (ux, uy)) = (way(along), way(beside), way(up));
        // What is left of the way from the middle to the place, once the way up to the plane is taken off.
        let (from_middle, climb) = (at - self.room.center(), level - self.frame.middle[up]);
        let (x, y) = (f64::from(from_middle.x) - ux * climb, f64::from(from_middle.y) - uy * climb);
        let turned = ax * by - ay * bx;
        if turned.abs() < 1e-6 * (ax.hypot(ay) * bx.hypot(by)).max(f64::MIN_POSITIVE) {
            return None;
        }
        let mut point = [0.0; 3];
        point[along] = self.frame.middle[along] + (x * by - y * bx) / turned;
        point[beside] = self.frame.middle[beside] + (ax * y - ay * x) / turned;
        point[up] = level;
        Some(point)
    }

    /// Where a point is on the screen.
    pub fn at(&self, point: Point) -> Pos2 {
        let along =
            |axis: usize| self.ways[axis] * ((point[axis] - self.frame.middle[axis]) * self.scales[axis]) as f32;
        self.room.center() + along(0) + along(1) + along(2)
    }

    /// What the chart shows once it was dragged by so much on the screen: every point is that
    /// much further along.
    pub fn moved(&self, by: Vec2) -> Frame {
        let mut frame = self.frame;
        for axis in (0..3).filter(|axis| self.scales[*axis] > 0.0) {
            frame.middle[axis] -= f64::from(by.dot(self.ways[axis])) / self.scales[axis];
        }
        frame
    }

    /// On a plane: the values at the left and right edges of the room, and at its bottom and top.
    pub fn edges(&self) -> [(f64, f64); 2] {
        let half = [self.room.width(), self.room.height()].map(|side| f64::from(side) / 2.0);
        [0, 1].map(|axis| {
            let reach = half[axis] / self.scales[axis];
            (self.frame.middle[axis] - reach, self.frame.middle[axis] + reach)
        })
    }
}
