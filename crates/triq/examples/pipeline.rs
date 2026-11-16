//! One process, three nodes: a source on a timer, a filter that runs when data
//! arrives, and the application itself reading the result.
//!
//! ```text
//! cargo run --example pipeline
//! ```
//!
//! While it runs, `cargo run --bin triq -- list` shows the same graph from outside.
use serde::{Deserialize, Serialize};
use std::time::Duration;
use triq::prelude::*;

/// A message: plain `#[repr(C)]` data. It is written into shared memory once
/// and read there by every receiver.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Pose")]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Ports)]
struct ImuPorts {
    pose: Output<Pose>,
}

/// Produces a pose every 10 ms.
struct Imu {
    time: f32,
}

impl Node for Imu {
    type Ports = ImuPorts;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self { time: 0.0 })
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(Duration::from_millis(10))
    }
    fn run(&mut self, ports: &mut ImuPorts) -> triq::Result {
        self.time += 0.01;
        let noise = (self.time * 977.0).sin() * 0.2;
        ports.pose.send(Pose { translation: [self.time.sin() + noise, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0] })?;
        Ok(())
    }
}

#[derive(Ports)]
struct FilterPorts {
    raw: Input<Pose>,
    smooth: Output<Pose>,
}

#[derive(Serialize, Deserialize)]
struct FilterParams {
    alpha: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self { alpha: 0.2 }
    }
}

/// Smooths the translation it receives.
struct Filter {
    alpha: f32,
    state: Option<[f32; 3]>,
}

impl Node for Filter {
    type Ports = FilterPorts;
    type Params = FilterParams;
    fn new(params: FilterParams) -> triq::Result<Self> {
        Ok(Self { alpha: params.alpha, state: None })
    }
    fn set_params(&mut self, params: FilterParams) -> triq::Result {
        self.alpha = params.alpha;
        Ok(())
    }
    fn run(&mut self, ports: &mut FilterPorts) -> triq::Result {
        while let Some(pose) = ports.raw.recv()? {
            let previous = self.state.unwrap_or(pose.translation);
            let blended =
                std::array::from_fn(|axis| previous[axis] + self.alpha * (pose.translation[axis] - previous[axis]));
            self.state = Some(blended);
            ports.smooth.send(Pose { translation: blended, ..*pose })?;
        }
        Ok(())
    }
}

/// The application's own end of the graph.
#[derive(Ports)]
struct Monitor {
    pose: Input<Pose>,
}

fn main() -> triq::Result {
    let rt = Runtime::builder("pipeline").pool("dsp", 2).start()?;
    rt.spawn::<Imu>("imu", Placement::Dedicated, ())?;
    rt.spawn::<Filter>("filter", Placement::pool("dsp"), FilterParams { alpha: 0.1 })?;
    let monitor: Monitor = rt.attach("monitor")?;
    rt.connect("imu/pose", "filter/raw")?;
    rt.connect("filter/smooth", "monitor/pose")?;

    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(pose) = monitor.pose.latest()? {
            println!("x = {:+.3}  (sample {}, {:?} old)", pose.translation[0], pose.stamp().seq, pose.age());
        }
    }
    Ok(())
}
