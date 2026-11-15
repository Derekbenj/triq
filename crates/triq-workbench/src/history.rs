//! What every connection did over time: a point for each time the machine was read.
//!
//! Nothing here is measured by the workbench. A runtime reports counters for
//! each of its connections; two readings of them give what happened between.
use std::collections::{BTreeMap, VecDeque};
use triq::Inventory;

/// An input and one output it hears. Both are `runtime/node/port`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Connection {
    pub to: String,
    pub from: String,
}

/// What a connection did between two readings of its counters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// When the second reading was taken: seconds since 1970, by the clock of the runtime that owns the input.
    pub at: f64,
    /// Samples received per second.
    pub rate: f64,
    /// Samples lost per second: sent, and gone before the input read them.
    pub lost: f64,
    /// The longest time from one sample to the next over the last second or two, in milliseconds.
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
    /// The reading the next point is measured from. `None` when the
    /// connection was not there at the last reading.
    last: Option<Counters>,
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
                        // Counters that went back belong to a node or a program that started again.
                        let before = trace.last.as_ref().filter(|before| {
                            before.pid == now.pid && before.taken_ns < now.taken_ns && before.received <= received
                        });
                        if let Some(before) = before {
                            let seconds = (now.taken_ns - before.taken_ns) as f64 / 1e9;
                            trace.points.push_back(Point {
                                at: now.taken_ns as f64 / 1e9,
                                rate: (received - before.received) as f64 / seconds,
                                lost: lost.saturating_sub(before.lost) as f64 / seconds,
                                gap: source.gap_us as f64 / 1000.0,
                                latency: source.latency_us as f64 / 1000.0,
                            });
                        }
                        trace.last = Some(now);
                        seen.push(connection);
                    }
                }
            }
        }
        // What is too old goes, and with it a connection that is gone and has nothing left to show.
        let oldest = self.newest().unwrap_or(0.0) - Self::LONGEST;
        self.traces.retain(|connection, trace| {
            if !seen.contains(connection) {
                trace.last = None;
            }
            trace.points.retain(|point| point.at >= oldest);
            trace.last.is_some() || !trace.points.is_empty()
        });
    }

    /// The connections that were there at the last reading, sorted by input.
    pub fn connections(&self) -> impl Iterator<Item = &Connection> {
        self.traces.iter().filter(|(_, trace)| trace.last.is_some()).map(|(connection, _)| connection)
    }

    /// What a connection did, oldest first.
    pub fn points(&self, connection: &Connection) -> impl Iterator<Item = &Point> {
        self.traces.get(connection).into_iter().flat_map(|trace| &trace.points)
    }

    /// When the newest point of any connection was taken.
    pub fn newest(&self) -> Option<f64> {
        self.traces.values().filter_map(|trace| trace.points.back()).map(|point| point.at).reduce(f64::max)
    }
}
