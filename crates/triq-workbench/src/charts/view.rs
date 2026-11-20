//! How a chart of numbers against each other is looked at: what part of its
//! points it shows, and where that puts a point on the screen.
//!
//! A chart shows all of its points until it is zoomed or moved by hand. From
//! then on it shows what it was left at, until it is told to show all again.
//! Zooming is about the middle of the chart, and so is turning a chart in space.
//!
//! In space, z is up, and nothing is drawn smaller for being further away: a
//! unit is as long at the back as at the front.
use eframe::egui::{Pos2, Rect, Vec2, vec2};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6};

/// How far a chart in space is turned for each point of the screen it is dragged, in radians.
const TURN: f32 = 0.01;

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
}

impl Default for View {
    /// A chart in space is first looked at from the front and a little to the right, from above.
    fn default() -> Self {
        Self { equal: false, held: None, from: (-FRAC_PI_3, FRAC_PI_6) }
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

    /// The least and the most that it reaches to along an axis.
    pub fn reach(&self, axis: usize) -> (f64, f64) {
        (self.middle[axis] - self.spans[axis] / 2.0, self.middle[axis] + self.spans[axis] / 2.0)
    }
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

    /// A chart in space, looked at from so far round its upright axis, z, and from so far
    /// above the level: both in radians. What the frame reaches across is a box, with each
    /// axis along an edge, that is turned about its middle and fits the room whichever way it
    /// is turned. With `equal`, a unit is as long along every axis, and the longest fills its edge.
    pub fn space(frame: Frame, room: Rect, equal: bool, (round, above): (f32, f32)) -> Lens {
        // The box is never wider on the screen than from one of its corners to the opposite one.
        let edge = f64::from(room.width().min(room.height())) / 3f64.sqrt();
        let scales = frame.spans.map(|span| edge / span);
        let scales = if equal { [scales.into_iter().fold(f64::INFINITY, f64::min); 3] } else { scales };
        // Which way is right on the screen, and which way is up, as seen from there.
        let ((sin_round, cos_round), (sin_above, cos_above)) = (round.sin_cos(), above.sin_cos());
        let (right, up) = ([-sin_round, cos_round, 0.0], [-sin_above * cos_round, -sin_above * sin_round, cos_above]);
        Lens { frame, room, ways: std::array::from_fn(|axis| vec2(right[axis], -up[axis])), scales }
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
