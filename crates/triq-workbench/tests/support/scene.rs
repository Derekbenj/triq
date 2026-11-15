//! A small machine for the workbench tests to look at. Included with `#[path]`.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use triq::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend)]
#[repr(C)]
#[type_name("Pose")]
pub struct Pose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Ports)]
pub struct ImuPorts {
    pub pose: Output<Pose>,
}

#[derive(Ports)]
pub struct FilterPorts {
    pub raw: Input<Pose>,
    pub smooth: Output<Pose>,
}

#[derive(Ports)]
pub struct ViewerPorts {
    pub pose: Input<Pose>,
    pub image: Input<[u8]>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterParams {
    pub alpha: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self { alpha: 0.25 }
    }
}

pub struct Filter {
    alpha: f32,
}

impl Node for Filter {
    type Ports = FilterPorts;
    type Params = FilterParams;
    fn new(params: FilterParams) -> triq::Result<Self> {
        Ok(Self { alpha: params.alpha })
    }
    fn set_params(&mut self, params: FilterParams) -> triq::Result {
        self.alpha = params.alpha;
        Ok(())
    }
    fn run(&mut self, ports: &mut FilterPorts) -> triq::Result {
        while let Some(pose) = ports.raw.recv()? {
            let mut smooth = *pose;
            smooth.translation[0] *= self.alpha;
            ports.smooth.send(smooth)?;
        }
        Ok(())
    }
}

pub fn unique(tag: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("w{}-{}-{tag}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    done()
}

/// Two programs' worth of nodes: a robot with a sensor and a filter, and a desk with a viewer.
pub struct Scene {
    pub robot: Runtime,
    pub desk: Runtime,
    pub imu: ImuPorts,
    pub viewer: ViewerPorts,
}

impl Scene {
    pub fn new() -> triq::Result<Self> {
        let robot = Runtime::builder(&unique("robot")).pool("dsp", 2).start()?;
        robot.register::<Filter>("filter")?;
        let imu: ImuPorts = robot.attach("imu")?;
        robot.spawn_kind("filter", "smoother", Placement::pool("dsp"), serde_json::json!({ "alpha": 0.5 }))?;
        robot.connect("imu/pose", "smoother/raw")?;

        let desk = Runtime::new(&unique("desk"))?;
        let viewer: ViewerPorts = desk.attach("viewer")?;
        desk.connect(&format!("{}/smoother/smooth", robot.name()), "viewer/pose")?;
        // An output nobody offers yet.
        desk.connect(&format!("{}/camera/image", robot.name()), "viewer/image")?;
        Ok(Self { robot, desk, imu, viewer })
    }

    pub fn inventories(&self) -> Vec<triq::Inventory> {
        vec![self.desk.inventory(), self.robot.inventory()]
    }
}
