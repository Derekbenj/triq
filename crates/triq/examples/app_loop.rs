//! A program that keeps its own main loop, as a renderer or a device driver does.
//!
//! triq never takes the main thread. The application offers a kind of node,
//! receives the ports of each one that is asked for, and uses them from its
//! own loop. The things behind a node may be tied to that thread (`Rc` here).
//!
//! ```text
//! cargo run --example app_loop
//! ```
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::rc::Rc;
use std::time::Duration;
use triq::prelude::*;
use triq::{Local, Remote};

#[derive(Ports)]
struct CameraPorts {
    /// Large frames: keep only the newest two per viewer.
    #[port(queue = 2)]
    image: Output<[u8]>,
    exposure: Input<f32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CameraParams {
    width: usize,
    height: usize,
}

struct Camera {
    name: String,
    params: CameraParams,
    ports: CameraPorts,
    /// Stands for a device handle that must stay on this thread.
    device: Rc<str>,
    exposure: f32,
}

#[derive(Ports)]
struct Viewer {
    image: Input<[u8]>,
}

fn main() -> triq::Result {
    let rt = Runtime::new("app")?;
    let requests = rt.local_factory::<CameraPorts, CameraParams>("camera")?;
    rt.set_factory_info("camera", json!({ "sensors": ["front", "rear"] }))?;
    let viewer: Viewer = rt.attach("viewer")?;

    // Any program can do this part; `triq spawn app camera front --params '{"width":64,"height":48}'`
    // and `triq connect app/front/image app/viewer/image` are the same requests.
    let remote = Remote::open("app")?;
    remote.spawn("camera", "front", Placement::Dedicated, json!({ "width": 64, "height": 48 }))?;
    remote.connect("front/image", "viewer/image")?;

    let mut cameras: Vec<Camera> = Vec::new();
    for frame in 0..30u8 {
        // The application's loop. First, whatever other programs asked for.
        while let Some(request) = requests.try_next() {
            match request {
                Local::Spawn { name, params, ports } => {
                    println!("camera {name} created: {params:?}");
                    cameras.push(Camera { device: Rc::from(name.as_str()), name, params, ports, exposure: 1.0 });
                }
                Local::Params { name, params } => {
                    if let Some(camera) = cameras.iter_mut().find(|camera| camera.name == name) {
                        camera.params = params;
                    }
                }
                Local::Remove { name } => cameras.retain(|camera| camera.name != name),
                Local::Pause { .. } | Local::Resume { .. } => {}
            }
        }
        // Then the application's own work.
        for camera in &mut cameras {
            if let Some(exposure) = camera.ports.exposure.latest()? {
                camera.exposure = *exposure;
            }
            if camera.ports.image.subscribers() == 0 {
                continue; // nobody is looking: skip the rendering
            }
            // Render straight into shared memory.
            let mut image = camera.ports.image.loan(camera.params.width * camera.params.height)?;
            image.fill((f32::from(frame) * camera.exposure) as u8);
            image.send()?;
        }
        if let Some(image) = viewer.image.latest()? {
            let device = &cameras[0].device;
            println!("frame {frame}: {} bytes of {} from {} ({device})", image.len(), image[0], image.source());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}
