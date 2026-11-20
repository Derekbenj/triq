//! What every connection and every output did over time: a point for each
//! time the machine was read.
//!
//! Nothing here is measured by the workbench. A runtime reports counters for
//! each of its connections and outputs; two readings of them give what happened between.
//! The machine may be read many times a second, and every reading makes a
//! point. A rate is taken over the half second before it, and timed by when
//! the samples were sent, so that a steady stream draws a steady line.
use crate::graph::{Reading, seconds_between};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use triq::Inventory;

/// An input and one output it hears. Both are `runtime/node/port`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Connection {
    pub to: String,
    pub from: String,
}

/// What a connection was doing at one reading of its counters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// When the reading was taken: seconds since 1970, by the clock of the runtime that owns the input.
    pub at: f64,
    /// Samples received per second over the half second before, timed by when they were sent.
    pub rate: f64,
    /// Samples lost per second, over the same time: sent, and gone before the input read them.
    pub lost: f64,
    /// The longest time from one sample to the next over the last half second to second, in milliseconds.
    pub gap: f64,
    /// Send-to-receive time of the newest sample, in milliseconds.
    pub latency: f64,
}

/// One of the numbers a point holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stat {
    Rate,
    Lost,
    Gap,
    Latency,
}

impl Stat {
    pub const ALL: [Stat; 4] = [Stat::Rate, Stat::Lost, Stat::Gap, Stat::Latency];

    pub fn name(self) -> &'static str {
        match self {
            Stat::Rate => "rate",
            Stat::Lost => "lost",
            Stat::Gap => "gap",
            Stat::Latency => "latency",
        }
    }

    /// What the number counts in, written after it.
    pub fn unit(self) -> &'static str {
        match self {
            Stat::Rate | Stat::Lost => "/s",
            Stat::Gap | Stat::Latency => " ms",
        }
    }

    pub fn of(self, point: &Point) -> f64 {
        match self {
            Stat::Rate => point.rate,
            Stat::Lost => point.lost,
            Stat::Gap => point.gap,
            Stat::Latency => point.latency,
        }
    }
}

/// The points of every connection and of every output, oldest first.
#[derive(Default)]
pub struct History {
    traces: BTreeMap<Connection, Trace>,
    /// How fast each output sent, by its `runtime/node/port`.
    outputs: BTreeMap<String, Trace>,
}

#[derive(Default)]
struct Trace {
    /// The latest readings, back to the one the next rate is counted from.
    /// Empty when the connection or the output was not there at the last reading.
    recent: VecDeque<Counters>,
    points: VecDeque<Point>,
}

/// What a runtime said about a connection or an output at one reading.
struct Counters {
    pid: u32,
    /// The samples a connection received, or an output sent.
    passed: Reading,
    /// Those a connection lost, or an output could not deliver.
    lost: u64,
}

impl Trace {
    /// Take in a reading of the counters. With an earlier one it makes a
    /// point, which also holds the gap and the latency as they stand.
    fn take(&mut self, now: Counters, gap_us: u64, latency_us: u64) {
        let Reading { count, taken_ns, .. } = now.passed;
        // Counters that went back belong to a node or a program that started
        // again, and nothing is counted across that.
        let follows = |before: &Counters| {
            before.pid == now.pid && before.passed.taken_ns < taken_ns && before.passed.count <= count
        };
        if !self.recent.back().is_some_and(follows) {
            self.recent.clear();
        }
        // Count from the newest reading that is half a second old, or the oldest there is.
        let old_enough = |before: &Counters| taken_ns - before.passed.taken_ns >= History::SPAN;
        while self.recent.len() > 1 && old_enough(&self.recent[1]) {
            self.recent.pop_front();
        }
        if let Some(before) = self.recent.front() {
            let seconds = seconds_between(&before.passed, &now.passed);
            self.points.push_back(Point {
                at: taken_ns as f64 / 1e9,
                rate: (count - before.passed.count) as f64 / seconds,
                lost: now.lost.saturating_sub(before.lost) as f64 / seconds,
                gap: gap_us as f64 / 1000.0,
                latency: latency_us as f64 / 1000.0,
            });
        }
        self.recent.push_back(now);
    }

    /// Let go of the points from before `oldest`. `there` says whether the
    /// last reading had this connection or output. False when nothing is
    /// left to keep it for.
    fn keep(&mut self, there: bool, oldest: f64) -> bool {
        if !there {
            self.recent.clear();
        }
        while self.points.front().is_some_and(|point| point.at < oldest) {
            self.points.pop_front();
        }
        !self.recent.is_empty() || !self.points.is_empty()
    }

    /// One number between two times, as a chart draws it.
    fn line(&self, stat: Stat, since: f64, until: f64) -> Vec<(f64, f64)> {
        between(&self.points, |point| (point.at, stat.of(point)), since, until)
    }
}

/// The part of a line between two times, as a chart draws it: pairs of time
/// and value, oldest first. `place` says when a point was and how high. Where
/// the line crosses either time on its way from one point to the next, it is
/// cut there. The points are found without looking through all of them.
pub fn between<P>(points: &VecDeque<P>, place: impl Fn(&P) -> (f64, f64), since: f64, until: f64) -> Vec<(f64, f64)> {
    let first = points.partition_point(|point| place(point).0 < since);
    let end = points.partition_point(|point| place(point).0 <= until);
    // Where the line is at `time`, if that lies between the point at `index` and the one before it.
    let cut = |index: usize, time: f64| {
        let (before, after) = (place(points.get(index.checked_sub(1)?)?), place(points.get(index)?));
        let along = (time - before.0) / (after.0 - before.0);
        (before.0 < time && time < after.0).then_some((time, before.1 + (after.1 - before.1) * along))
    };
    let in_view = points.range(first.min(end)..end).map(&place);
    cut(first, since).into_iter().chain(in_view).chain(cut(end, until)).collect()
}

impl History {
    /// How many seconds of the past are kept.
    pub const LONGEST: f64 = 3600.0;
    /// How long a rate is counted over, in nanoseconds.
    const SPAN: u64 = 500_000_000;

    /// Take in one reading of the machine.
    pub fn record(&mut self, inventories: &[Inventory]) {
        let (mut heard, mut sending) = (Vec::new(), Vec::new());
        for inventory in inventories {
            let (pid, taken_ns) = (inventory.pid, inventory.taken_ns);
            for node in &inventory.nodes {
                for input in &node.inputs {
                    let to = format!("{}/{}/{}", inventory.runtime, node.name, input.port.name);
                    for source in &input.sources {
                        let connection = Connection { to: to.clone(), from: source.output.clone() };
                        let passed = Reading { count: source.received, sent_ns: source.sent_ns, taken_ns };
                        let trace = self.traces.entry(connection.clone()).or_default();
                        trace.take(Counters { pid, passed, lost: source.lost }, source.gap_us, source.latency_us);
                        heard.push(connection);
                    }
                }
                for output in &node.outputs {
                    let passed = Reading { count: output.sent, sent_ns: output.sent_ns, taken_ns };
                    let trace = self.outputs.entry(output.service.clone()).or_default();
                    trace.take(Counters { pid, passed, lost: output.undelivered }, 0, 0);
                    sending.push(&output.service);
                }
            }
        }
        // What is too old goes, and with it a connection or an output that is gone and has nothing left to show.
        let oldest = self.newest().unwrap_or(0.0) - Self::LONGEST;
        self.traces.retain(|connection, trace| trace.keep(heard.contains(connection), oldest));
        self.outputs.retain(|output, trace| trace.keep(sending.contains(&output), oldest));
    }

    /// The connections that were there at the last reading, sorted by input.
    pub fn connections(&self) -> impl Iterator<Item = &Connection> {
        self.traces.iter().filter(|(_, trace)| !trace.recent.is_empty()).map(|(connection, _)| connection)
    }

    /// What a connection did, oldest first.
    pub fn points(&self, connection: &Connection) -> impl Iterator<Item = &Point> {
        self.traces.get(connection).into_iter().flat_map(|trace| &trace.points)
    }

    /// The newest point of a connection.
    pub fn latest(&self, connection: &Connection) -> Option<&Point> {
        self.traces.get(connection)?.points.back()
    }

    /// One number of a connection between two times, as a chart draws it:
    /// pairs of time and value, oldest first, cut where the line crosses either time.
    pub fn line(&self, connection: &Connection, stat: Stat, since: f64, until: f64) -> Vec<(f64, f64)> {
        self.traces.get(connection).map_or(Vec::new(), |trace| trace.line(stat, since, until))
    }

    /// The outputs that were there at the last reading, sorted by name.
    pub fn outputs(&self) -> impl Iterator<Item = &String> {
        self.outputs.iter().filter(|(_, trace)| !trace.recent.is_empty()).map(|(output, _)| output)
    }

    /// How many samples a second an output sent over the half second before the last reading.
    pub fn sending(&self, output: &str) -> Option<f64> {
        Some(self.outputs.get(output)?.points.back()?.rate)
    }

    /// How fast an output sent between two times, as a chart draws it.
    pub fn sent(&self, output: &str, since: f64, until: f64) -> Vec<(f64, f64)> {
        self.outputs.get(output).map_or(Vec::new(), |trace| trace.line(Stat::Rate, since, until))
    }

    /// When the newest point of any connection or output was taken.
    pub fn newest(&self) -> Option<f64> {
        let traces = self.traces.values().chain(self.outputs.values());
        traces.filter_map(|trace| trace.points.back()).map(|point| point.at).reduce(f64::max)
    }
}
