//! What every connection did over time: a point for each time the machine was read.
//!
//! Nothing here is measured by the workbench. A runtime reports counters for
//! each of its connections; two readings of them give what happened between.
//! The machine may be read many times a second, and every reading makes a
//! point. A rate is still counted over half a second: over a tenth, one
//! sample more or less would look like ten a second.
use std::collections::{BTreeMap, VecDeque};
use triq::Inventory;

/// An input and one output it hears. Both are `runtime/node/port`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Connection {
    pub to: String,
    pub from: String,
}

/// What a connection was doing at one reading of its counters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// When the reading was taken: seconds since 1970, by the clock of the runtime that owns the input.
    pub at: f64,
    /// Samples received per second, over the half second before.
    pub rate: f64,
    /// Samples lost per second, over the same time: sent, and gone before the input read them.
    pub lost: f64,
    /// The longest time from one sample to the next over the last half second to second, in milliseconds.
    pub gap: f64,
    /// Send-to-receive time of the newest sample, in milliseconds.
    pub latency: f64,
}

/// One of the numbers a point holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The points of every connection, oldest first.
#[derive(Default)]
pub struct History {
    traces: BTreeMap<Connection, Trace>,
}

#[derive(Default)]
struct Trace {
    /// The latest readings, back to the one the next rate is counted from.
    /// Empty when the connection was not there at the last reading.
    recent: VecDeque<Counters>,
    points: VecDeque<Point>,
}

/// What a runtime said about a connection at one reading.
struct Counters {
    pid: u32,
    taken_ns: u64,
    received: u64,
    lost: u64,
}

impl History {
    /// How many seconds of the past are kept.
    pub const LONGEST: f64 = 3600.0;
    /// How long a rate is counted over, in nanoseconds.
    const SPAN: u64 = 500_000_000;

    /// Take in one reading of the machine.
    pub fn record(&mut self, inventories: &[Inventory]) {
        let mut seen = Vec::new();
        for inventory in inventories {
            for node in &inventory.nodes {
                for input in &node.inputs {
                    let to = format!("{}/{}/{}", inventory.runtime, node.name, input.port.name);
                    // An input that is still waiting for its output has nothing to measure.
                    for source in input.sources.iter().filter(|source| source.connected) {
                        let connection = Connection { to: to.clone(), from: source.output.clone() };
                        let (received, lost) = (source.received, source.lost);
                        let now = Counters { pid: inventory.pid, taken_ns: inventory.taken_ns, received, lost };
                        let trace = self.traces.entry(connection.clone()).or_default();
                        // Counters that went back belong to a node or a program that started
                        // again, and nothing is counted across that.
                        let follows = |before: &Counters| {
                            before.pid == now.pid && before.taken_ns < now.taken_ns && before.received <= received
                        };
                        if !trace.recent.back().is_some_and(follows) {
                            trace.recent.clear();
                        }
                        // Count from the newest reading that is half a second old, or the oldest there is.
                        while trace.recent.len() > 1 && now.taken_ns - trace.recent[1].taken_ns >= Self::SPAN {
                            trace.recent.pop_front();
                        }
                        if let Some(before) = trace.recent.front() {
                            let seconds = (now.taken_ns - before.taken_ns) as f64 / 1e9;
                            trace.points.push_back(Point {
                                at: now.taken_ns as f64 / 1e9,
                                rate: (received - before.received) as f64 / seconds,
                                lost: lost.saturating_sub(before.lost) as f64 / seconds,
                                gap: source.gap_us as f64 / 1000.0,
                                latency: source.latency_us as f64 / 1000.0,
                            });
                        }
                        trace.recent.push_back(now);
                        seen.push(connection);
                    }
                }
            }
        }
        // What is too old goes, and with it a connection that is gone and has nothing left to show.
        let oldest = self.newest().unwrap_or(0.0) - Self::LONGEST;
        self.traces.retain(|connection, trace| {
            if !seen.contains(connection) {
                trace.recent.clear();
            }
            while trace.points.front().is_some_and(|point| point.at < oldest) {
                trace.points.pop_front();
            }
            !trace.recent.is_empty() || !trace.points.is_empty()
        });
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
    /// pairs of time and value, oldest first. Where the line crosses either
    /// time on its way from one point to the next, it is cut there. The
    /// points are found without looking through the whole hour.
    pub fn line(&self, connection: &Connection, stat: Stat, since: f64, until: f64) -> Vec<(f64, f64)> {
        let Some(Trace { points, .. }) = self.traces.get(connection) else { return Vec::new() };
        let first = points.partition_point(|point| point.at < since);
        let end = points.partition_point(|point| point.at <= until);
        // Where the line is at `time`, if that lies between the point at `index` and the one before it.
        let cut = |index: usize, time: f64| {
            let (before, after) = (points.get(index.checked_sub(1)?)?, points.get(index)?);
            let along = (time - before.at) / (after.at - before.at);
            let value = stat.of(before) + (stat.of(after) - stat.of(before)) * along;
            (before.at < time && time < after.at).then_some((time, value))
        };
        let in_view = points.range(first.min(end)..end).map(|point| (point.at, stat.of(point)));
        cut(first, since).into_iter().chain(in_view).chain(cut(end, until)).collect()
    }

    /// When the newest point of any connection was taken.
    pub fn newest(&self) -> Option<f64> {
        self.traces.values().filter_map(|trace| trace.points.back()).map(|point| point.at).reduce(f64::max)
    }
}
