//! A program that offers nodes for others to create and wire.
//!
//! Start two of them, then use the command-line tool from a third terminal:
//!
//! ```text
//! cargo run --example station -- left
//! cargo run --example station -- right
//!
//! cargo run --bin triq -- list
//! cargo run --bin triq -- spawn left counter ticks --params '{"period_ms":500}'
//! cargo run --bin triq -- spawn right doubler twice --pool work
//! cargo run --bin triq -- spawn right logger log
//! cargo run --bin triq -- connect left/ticks/output right/twice/input
//! cargo run --bin triq -- connect right/twice/output right/log/input
//! ```
//!
//! Stop and restart `left`, spawn `ticks` again, and `right` carries on by itself.
use serde::{Deserialize, Serialize};
use std::time::Duration;
use triq::prelude::*;

#[derive(Ports)]
struct SourcePorts {
    output: Output<u64>,
}

#[derive(Ports)]
struct Through {
    input: Input<u64>,
    output: Output<u64>,
}

#[derive(Ports)]
struct SinkPorts {
    input: Input<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct CounterParams {
    period_ms: u64,
}

impl Default for CounterParams {
    fn default() -> Self {
        Self { period_ms: 100 }
    }
}

/// Counts up on a fixed period.
struct Counter {
    period: Duration,
    count: u64,
}

impl Node for Counter {
    type Ports = SourcePorts;
    type Params = CounterParams;
    fn new(params: CounterParams) -> triq::Result<Self> {
        Ok(Self { period: Duration::from_millis(params.period_ms), count: 0 })
    }
    fn set_params(&mut self, params: CounterParams) -> triq::Result {
        self.period = Duration::from_millis(params.period_ms);
        Ok(())
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(self.period)
    }
    fn run(&mut self, ports: &mut SourcePorts) -> triq::Result {
        self.count += 1;
        ports.output.send(self.count)?;
        Ok(())
    }
}

struct Doubler;

impl Node for Doubler {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value * 2)?;
        }
        Ok(())
    }
}

/// Prints what it receives, and where it came from.
struct Logger;

impl Node for Logger {
    type Ports = SinkPorts;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut SinkPorts) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            println!("{} from {} after {:?}", *value, value.source(), value.age());
        }
        Ok(())
    }
}

fn main() -> triq::Result {
    let name = std::env::args().nth(1).unwrap_or_else(|| "station".into());
    let rt = Runtime::builder(&name).pool("work", 2).start()?;
    rt.register::<Counter>("counter")?;
    rt.register::<Doubler>("doubler")?;
    rt.register::<Logger>("logger")?;
    eprintln!("{name}: offering counter, doubler and logger. Try `triq list`.");
    rt.wait();
    Ok(())
}
