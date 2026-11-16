//! A small machine for the workbench tests to look at. Included with `#[path]`.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use triq::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend, Fields)]
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
        // A connection is live a moment after it was asked for. A test begins when both are.
        let live = |runtime: &Runtime, node: &str| {
            let nodes = runtime.inventory().nodes;
            let node = nodes.iter().find(|info| info.name == node).expect("the node");
            node.inputs[0].sources.iter().any(|source| source.connected)
        };
        assert!(wait_for(|| live(&robot, "smoother") && live(&desk, "viewer")), "the scene's connections are live");
        Ok(Self { robot, desk, imu, viewer })
    }

    /// What the two runtimes report, the desk first.
    pub fn inventories(&self) -> Vec<triq::Inventory> {
        vec![self.desk.inventory(), self.robot.inventory()]
    }
}

/// Pretend each runtime took its second reading this long after its first,
/// and that the newest sample of every port was sent just as it did.
pub fn later(
    before: &[triq::Inventory],
    mut after: Vec<triq::Inventory>,
    elapsed: [Duration; 2],
) -> Vec<triq::Inventory> {
    for ((after, before), elapsed) in after.iter_mut().zip(before).zip(elapsed) {
        after.taken_ns = before.taken_ns + elapsed.as_nanos() as u64;
        for node in &mut after.nodes {
            let sources = node.inputs.iter_mut().flat_map(|input| &mut input.sources);
            sources.filter(|source| source.received > 0).for_each(|source| source.sent_ns = after.taken_ns);
            node.outputs.iter_mut().filter(|output| output.sent > 0).for_each(|output| output.sent_ns = after.taken_ns);
        }
    }
    after
}
