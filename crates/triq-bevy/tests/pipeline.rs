//! The whole chain across two programs: a pretend controller in this one, and
//! the real Bevy program, without a window, as the other.
use serde_json::json;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq::{NodeState, Remote};
use triq_bevy::{CAMERA, Frame, Pose, RIG};
use triq_xbox::XboxState;

/// What this test holds: a controller to push, and what comes back from the rig and a camera.
#[derive(Ports)]
struct Pilot {
    controller: Output<XboxState>,
    pose: Input<Pose>,
    image: Input<[u8], Frame>,
}

/// A camera this wide has rows that the GPU pads, so the padding must be taken out on the way.
const WIDTH: u32 = 200;
const HEIGHT: u32 = 150;

/// A child process that is killed when the test ends, however it ends.
struct Program(Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Poll until `found` returns something. Starting a renderer takes a while, so this waits up to 30 seconds.
fn wait_for<T>(what: &str, mut found: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn latest_pose(pilot: &Pilot) -> Option<Pose> {
    pilot.pose.latest().expect("pose").map(|pose| *pose)
}

/// The next picture from the camera: what it says about itself, and its pixels.
fn next_frame(pilot: &Pilot) -> (Frame, Vec<u8>) {
    wait_for("a camera frame", || pilot.image.recv().expect("image").map(|frame| (*frame.header(), frame.to_vec())))
}

#[test]
fn a_controller_drives_the_rig_and_a_camera_on_it_sends_pictures() -> triq::Result {
    let bevy = format!("bevy{}", std::process::id());
    let here = format!("pilot{}", std::process::id());
    let mut program = Program(
        Command::new(env!("CARGO_BIN_EXE_triq-bevy")).args([&bevy, "--headless"]).stdout(Stdio::null()).spawn()?,
    );
    let rt = Runtime::new(&here)?;
    let pilot: Pilot = rt.attach("pad")?;

    // The program offers rigs and cameras, and has none until asked.
    let remote = wait_for("the Bevy program", || {
        let remote = Remote::open(&bevy).ok()?;
        (remote.inventory().ok()?.factories.len() == 2).then_some(remote)
    });
    let offered = remote.inventory()?;
    let kinds: Vec<&str> = offered.factories.iter().map(|factory| factory.kind.as_str()).collect();
    assert_eq!(kinds, [CAMERA, RIG]);
    assert!(offered.factories.iter().all(|factory| factory.local));
    assert!(offered.nodes.is_empty());

    remote.spawn(RIG, "rig", Placement::Dedicated, json!({}))?;
    remote.spawn(CAMERA, "left", Placement::Dedicated, json!({ "width": WIDTH, "height": HEIGHT, "hz": 60.0 }))?;
    triq::connect(&format!("{here}/pad/controller"), &format!("{bevy}/rig/controller"))?;
    triq::connect(&format!("{bevy}/rig/pose"), &format!("{here}/pad/pose"))?;
    triq::connect(&format!("{bevy}/rig/mounts"), &format!("{bevy}/left/mounts"))?;
    triq::connect(&format!("{bevy}/left/image"), &format!("{here}/pad/image"))?;

    // With nobody at the controller the rig stays where it starts.
    let start = wait_for("the rig's first pose", || latest_pose(&pilot));
    assert_eq!(start.translation, [0.0, 1.5, 6.0]);

    // The camera's pictures arrive with their size beside the pixels. Its very
    // first one is only the background, drawn before the renderer was ready for a new camera.
    let (first, before) = wait_for("a picture of the scene", || {
        Some(next_frame(&pilot)).filter(|(_, pixels)| pixels.chunks(4).any(|pixel| pixel != &pixels[..4]))
    });
    assert_eq!((first.width, first.height), (WIDTH, HEIGHT));
    assert_eq!(before.len(), (WIDTH * HEIGHT * 4) as usize);

    // Push the left stick forward until the rig is a metre further on.
    let forward = XboxState { connected: true, left_y: i16::MIN, ..Default::default() };
    let moved = wait_for("the rig to move", || {
        pilot.controller.send(forward).expect("send");
        latest_pose(&pilot).filter(|pose| pose.translation[2] < 5.0)
    });
    assert_eq!((moved.translation[0], moved.translation[1]), (0.0, 1.5), "it moved straight ahead");

    // The controller goes quiet without letting go of the stick. The rig must not run on.
    std::thread::sleep(Duration::from_millis(400));
    let stopped = latest_pose(&pilot).expect("pose");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(latest_pose(&pilot).expect("pose"), stopped, "the rig stops when its controller goes quiet");

    // The camera rode along: later pictures are numbered on, and show something else.
    let (later, after) =
        wait_for("a picture from the new place", || Some(next_frame(&pilot)).filter(|(_, pixels)| *pixels != before));
    assert!(later.number > first.number);
    let picture = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bevy-camera.png");
    image::save_buffer(&picture, &after, later.width, later.height, image::ExtendedColorType::Rgba8)?;

    // Settings change while the nodes run: a new starting pose puts the rig
    // there, and a camera takes a new size. (Settings left out are the defaults.)
    remote.set_params("rig", json!({ "translation": [1.0, 2.0, 3.0] }))?;
    wait_for("the rig to be put elsewhere", || latest_pose(&pilot).filter(|pose| pose.translation == [1.0, 2.0, 3.0]));
    remote.set_params("left", json!({ "width": 120, "height": 90, "hz": 60.0 }))?;
    let (small, pixels) =
        wait_for("a smaller picture", || Some(next_frame(&pilot)).filter(|(frame, _)| frame.width == 120));
    assert_eq!((small.height, pixels.len()), (90, 120 * 90 * 4));
    assert!(small.number > later.number, "the same camera, still counting");

    // A paused camera sends nothing; removed nodes are gone.
    remote.pause("left")?;
    std::thread::sleep(Duration::from_millis(200));
    while pilot.image.recv()?.is_some() {}
    std::thread::sleep(Duration::from_millis(200));
    assert!(pilot.image.recv()?.is_none(), "a paused camera sends nothing");
    assert_eq!(remote.inventory()?.nodes.iter().find(|node| node.name == "left").unwrap().state, NodeState::Paused);
    remote.remove("left")?;
    remote.remove("rig")?;
    assert!(remote.inventory()?.nodes.is_empty());

    // Asked to shut down, the program ends by itself.
    remote.shutdown()?;
    let ended = wait_for("the Bevy program to end", || program.0.try_wait().expect("wait"));
    assert!(ended.success());
    Ok(())
}

#[test]
fn without_a_window_the_program_keeps_the_pace_it_is_told() -> triq::Result {
    let bevy = format!("pace{}", std::process::id());
    let here = format!("watch{}", std::process::id());
    // A hundred frames a second: more than the sixty it used to be held to.
    let arguments = [bevy.as_str(), "--headless", "--fps", "100"];
    let _program =
        Program(Command::new(env!("CARGO_BIN_EXE_triq-bevy")).args(arguments).stdout(Stdio::null()).spawn()?);
    let rt = Runtime::new(&here)?;
    let pilot: Pilot = rt.attach("pad")?;
    let remote = wait_for("the Bevy program", || {
        let remote = Remote::open(&bevy).ok()?;
        (remote.inventory().ok()?.factories.len() == 2).then_some(remote)
    });
    // A camera this wide has rows the GPU does not pad: its frames go out as they come back.
    let (width, height) = (256, 144);
    remote.spawn(CAMERA, "fast", Placement::Dedicated, json!({ "width": width, "height": height, "hz": 100.0 }))?;
    triq::connect(&format!("{bevy}/fast/image"), &format!("{here}/pad/image"))?;
    let (first, pixels) = wait_for("a picture of the scene", || {
        Some(next_frame(&pilot)).filter(|(_, pixels)| pixels.chunks(4).any(|pixel| pixel != &pixels[..4]))
    });
    assert_eq!((first.width, first.height, pixels.len()), (width, height, (width * height * 4) as usize));

    // Two seconds of pictures, each stamped with the time it was sent and numbered by the camera.
    let (mut sent, mut numbers) = (Vec::new(), Vec::new());
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        while let Some(frame) = pilot.image.recv()? {
            sent.push(frame.stamp().sent_ns);
            numbers.push(frame.header().number);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(numbers.windows(2).all(|pair| pair[1] == pair[0] + 1), "every picture, in the order it was taken");
    // More than the sixty a second it used to be held to, and no more than asked for. On a machine
    // with nothing else to do it is a hundred; this test also runs beside other programs.
    let rate = (sent.len() - 1) as f64 / ((sent[sent.len() - 1] - sent[0]) as f64 / 1e9);
    assert!((66.0..=103.0).contains(&rate), "{rate:.1} pictures a second");
    Ok(())
}
