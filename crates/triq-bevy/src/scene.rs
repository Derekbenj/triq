//! The Bevy program: a small world, a rig that a controller drives through
//! it, and cameras that ride on the rig and send what they see.
//!
//! Bevy keeps the main thread, as it must. The rig and the cameras are nodes
//! that this program runs itself: it offers two factories, takes the ports of
//! each node that is asked for, and uses them from ordinary Bevy systems.
use crate::{CameraParams, CameraPorts, Frame, Mounts, Pacing, Pose, RigParams, RigPorts, RigState};
use crate::{next_frame, unpad};
use bevy::app::{AppExit, PluginsState};
use bevy::camera::RenderTarget;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::ecs::batching::BatchingStrategy;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::window::ExitCondition;
use bevy::winit::{UpdateMode, WinitPlugin, WinitSettings};
use std::time::{Duration, Instant};
use triq::{LocalFactory, Runtime};
use triq_xbox::XboxState;

/// The kinds of node this program offers.
pub const RIG: &str = "rig";
pub const CAMERA: &str = "camera";

/// How often a rig moves and says where it is.
const RIG_HZ: f64 = 120.0;

/// What other programs ask of this one, picked up once per frame.
/// It stays on the main thread, with Bevy.
struct Requests {
    rigs: LocalFactory<RigPorts, RigParams>,
    cameras: LocalFactory<CameraPorts, CameraParams>,
    runtime: Runtime,
}

/// A rig node: an entity that a controller moves around.
#[derive(Component)]
struct Rig {
    name: String,
    ports: RigPorts,
    params: RigParams,
    state: RigState,
    paused: bool,
    /// The last thing heard from the controller, and when.
    heard: Option<(XboxState, Instant)>,
}

/// A camera node: an entity that renders to an image and sends it.
#[derive(Component)]
struct CameraNode {
    name: String,
    ports: CameraPorts,
    params: CameraParams,
    paused: bool,
    sent: u64,
    /// Which frames it sends.
    pacing: Pacing,
    /// Frames the GPU has handed back that are still to be sent, oldest first.
    waiting: Vec<Vec<u8>>,
    /// Something that went wrong while sending, until it has been reported.
    problem: Option<String>,
    /// Its picture in the window, if there is a window.
    preview: Option<Entity>,
}

/// The largest camera picture, in pixels each way. Any graphics card can render this.
const LARGEST: u32 = 4096;

/// The row of camera previews at the bottom of the window.
#[derive(Resource)]
struct Previews(Entity);

#[derive(Resource)]
struct Headless(bool);

/// How far apart frames come, evened out over the last second or so. One
/// frame's own time says little: after a long frame comes a short one.
#[derive(Resource)]
struct FrameTime(Duration);

fn time_frames(time: Res<Time<Real>>, mut frame: ResMut<FrameTime>) {
    frame.0 = frame.0.mul_f32(0.95) + time.delta().mul_f32(0.05);
}

/// Run the program without a window, one frame every `frame`.
fn keep_time(mut app: App, frame: Duration) -> AppExit {
    // As Bevy's own loops do first: let the plugins finish setting up.
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    let mut due = Instant::now();
    loop {
        app.update();
        if let Some(exit) = app.should_exit() {
            return exit;
        }
        due = next_frame(due, frame, Instant::now());
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
    }
}

/// Build the program. `name` is its runtime name; `headless` runs it without a window.
///
/// `fps` is how many frames a second it runs when it keeps time itself:
/// without a window, or with its window not in front. A camera cannot send
/// more pictures than there are frames. With its window in front the program
/// follows the display.
pub fn app(name: &str, headless: bool, fps: f64) -> triq::Result<App> {
    let frame = Duration::from_secs_f64(1.0 / fps.clamp(1.0, 1000.0));
    let runtime = Runtime::new(name)?;
    let requests = Requests { rigs: runtime.local_factory(RIG)?, cameras: runtime.local_factory(CAMERA)?, runtime };

    let window = Window { title: "triq · Bevy rig and cameras".into(), resolution: (1280, 800).into(), ..default() };
    let plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: (!headless).then_some(window),
        exit_condition: if headless { ExitCondition::DontExit } else { ExitCondition::OnAllClosed },
        ..default()
    });
    let mut app = App::new();
    if headless {
        // Without a window nothing paces the loop, so it is paced here.
        app.add_plugins(plugins.disable::<WinitPlugin>()).set_runner(move |app| keep_time(app, frame));
    } else {
        // Bevy would slow to sixty frames a second whenever its window is not in front.
        let behind = UpdateMode::reactive_low_power(frame);
        app.add_plugins(plugins)
            .insert_resource(WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: behind });
    }
    app.insert_non_send(requests)
        .insert_resource(Headless(headless))
        .insert_resource(FrameTime(frame))
        .add_systems(First, (time_frames, send_frames))
        .insert_resource(Time::<Fixed>::from_hz(RIG_HZ))
        .insert_resource(ClearColor(Color::srgb(0.12, 0.16, 0.23)))
        .add_systems(Startup, build_world)
        .add_systems(PreUpdate, serve_requests)
        .add_systems(FixedUpdate, move_rigs)
        .add_systems(Update, place_cameras)
        .add_systems(Last, (report_problems, stop_when_asked));
    Ok(app)
}

/// Do what other programs asked for since the last frame.
fn serve_requests(world: &mut World) {
    let requests = world.non_send::<Requests>();
    let rigs: Vec<_> = std::iter::from_fn(|| requests.rigs.try_next()).collect();
    let cameras: Vec<_> = std::iter::from_fn(|| requests.cameras.try_next()).collect();
    for request in rigs {
        match request {
            triq::Local::Spawn { name, params, ports } => spawn_rig(world, name, params, ports),
            // A new starting pose puts the rig there. The other settings are read at every step.
            triq::Local::Params { name, params } => with_rig(world, &name, |rig| {
                let start = |params: &RigParams| (params.translation, params.yaw, params.pitch);
                if start(&params) != start(&rig.params) {
                    rig.state = RigState::new(&params);
                }
                rig.params = params;
            }),
            triq::Local::Pause { name } => with_rig(world, &name, |rig| rig.paused = true),
            triq::Local::Resume { name } => with_rig(world, &name, |rig| rig.paused = false),
            // Despawning the entity drops the node's ports, which is what ends the node.
            triq::Local::Remove { name } => {
                if let Some(entity) = rig_named(world, &name) {
                    world.despawn(entity);
                }
            }
        }
    }
    for request in cameras {
        match request {
            triq::Local::Spawn { name, params, ports } => spawn_camera(world, name, params, ports),
            triq::Local::Params { name, params } => change_camera(world, &name, params),
            triq::Local::Pause { name } => with_camera(world, &name, |camera| camera.paused = true),
            triq::Local::Resume { name } => with_camera(world, &name, |camera| camera.paused = false),
            triq::Local::Remove { name } => {
                if let Some(entity) = camera_named(world, &name) {
                    if let Some(preview) = world.get::<CameraNode>(entity).and_then(|camera| camera.preview) {
                        world.despawn(preview);
                    }
                    world.despawn(entity);
                }
            }
        }
    }
}

fn rig_named(world: &mut World, name: &str) -> Option<Entity> {
    world.query::<(Entity, &Rig)>().iter(world).find(|(_, rig)| rig.name == name).map(|(entity, _)| entity)
}

fn camera_named(world: &mut World, name: &str) -> Option<Entity> {
    world.query::<(Entity, &CameraNode)>().iter(world).find(|(_, camera)| camera.name == name).map(|(entity, _)| entity)
}

fn with_rig(world: &mut World, name: &str, change: impl FnOnce(&mut Rig)) {
    if let Some(mut rig) = rig_named(world, name).and_then(|entity| world.get_mut::<Rig>(entity)) {
        change(&mut rig);
    }
}

fn with_camera(world: &mut World, name: &str, change: impl FnOnce(&mut CameraNode)) {
    if let Some(mut camera) = camera_named(world, name).and_then(|entity| world.get_mut::<CameraNode>(entity)) {
        change(&mut camera);
    }
}

/// A rig is drawn as an orange slab, so it can be seen moving.
fn spawn_rig(world: &mut World, name: String, params: RigParams, ports: RigPorts) {
    let state = RigState::new(&params);
    let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(0.7, 0.12, 0.3));
    let material = world.resource_mut::<Assets<StandardMaterial>>().add(Color::srgb(0.98, 0.5, 0.1));
    let place = placed(state.pose());
    println!("rig {name} created");
    world.spawn((
        Rig { name, ports, params, state, paused: false, heard: None },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        place,
    ));
}

/// A size any graphics card can render.
fn sized(params: CameraParams) -> CameraParams {
    CameraParams { width: params.width.clamp(1, LARGEST), height: params.height.clamp(1, LARGEST), ..params }
}

/// A camera starts where a rig starts, and stays there until a rig is connected.
fn spawn_camera(world: &mut World, name: String, params: CameraParams, ports: CameraPorts) {
    let params = sized(params);
    let panel = world.get_resource::<Previews>().map(|previews| previews.0);
    let preview = panel.map(|panel| world.spawn(ChildOf(panel)).id());
    println!("camera {name} created: {} x {}", params.width, params.height);
    let camera = CameraNode {
        name,
        ports,
        params,
        paused: false,
        sent: 0,
        pacing: Pacing::new(Instant::now()),
        waiting: Vec::new(),
        problem: None,
        preview,
    };
    let entity = world
        .spawn((
            camera,
            Camera3d::default(),
            Camera { order: -1, ..default() },
            // The pixels are sent as rendered: no tone mapping, dithering or smoothing.
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            placed(RigState::new(&RigParams::default()).pose()),
        ))
        .observe(take_frame)
        .id();
    equip(world, entity);
}

/// The mount point and the rate are read at every frame. A new size or
/// field of view needs a new lens and picture.
fn change_camera(world: &mut World, name: &str, params: CameraParams) {
    let Some(entity) = camera_named(world, name) else { return };
    let Some(mut camera) = world.get_mut::<CameraNode>(entity) else { return };
    let params = sized(params);
    let optics = |params: &CameraParams| (params.width, params.height, params.fov_degrees);
    let same = optics(&params) == optics(&camera.params);
    camera.params = params;
    if !same {
        equip(world, entity);
    }
}

/// Give a camera the lens its settings ask for and a picture of its own to
/// render into. Bevy reads that picture back from the GPU after every frame
/// and hands it to `take_frame`.
fn equip(world: &mut World, entity: Entity) {
    let Some(camera) = world.get::<CameraNode>(entity) else { return };
    let (width, height, preview) = (camera.params.width, camera.params.height, camera.preview);
    let lens = PerspectiveProjection {
        fov: camera.params.fov_degrees.to_radians(),
        aspect_ratio: width as f32 / height as f32,
        ..default()
    };
    let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = world.resource_mut::<Assets<Image>>().add(image);
    world.entity_mut(entity).insert((
        Projection::Perspective(lens),
        RenderTarget::Image(image.clone().into()),
        Readback::texture(image.clone()),
    ));
    if let Some(preview) = preview {
        let size = Node { width: px(240), height: px(240.0 * height as f32 / width as f32), ..default() };
        world.entity_mut(preview).insert((ImageNode::new(image), size));
    }
}

/// Called with each frame Bevy has read back from the GPU. That is at the one
/// moment in a frame when both halves of Bevy stand still, and with one camera
/// after another, so the frame is only taken here. `send_frames` sends it.
fn take_frame(mut read: On<ReadbackComplete>, frame: Res<FrameTime>, mut cameras: Query<&mut CameraNode>) {
    let Ok(mut camera) = cameras.get_mut(read.entity) else { return };
    // Nobody looking means nothing to do.
    if camera.paused || camera.ports.image.subscribers() == 0 {
        return;
    }
    // Of the frames the renderer hands back, as many a second as the camera was asked for.
    let hz = camera.params.hz;
    if camera.pacing.take(Instant::now(), hz, frame.0) {
        camera.waiting.push(std::mem::take(&mut read.event_mut().data));
    }
}

/// Send the frames the cameras took, all cameras at once: every frame is
/// megabytes to copy into shared memory, and the cameras need not wait for
/// each other.
fn send_frames(mut cameras: Query<&mut CameraNode>) {
    cameras.par_iter_mut().batching_strategy(BatchingStrategy::fixed(1)).for_each(|mut camera| {
        let camera = &mut *camera;
        let (width, height) = (camera.params.width, camera.params.height);
        for mut pixels in camera.waiting.drain(..) {
            // A frame of another size was rendered before the camera's size changed.
            if !unpad(&mut pixels, width, height) {
                continue;
            }
            // The pixels are copied once, with their size beside them.
            let number = camera.sent + 1;
            match camera.ports.image.send_slice_with(&pixels, Frame { width, height, number }) {
                Ok(_) => camera.sent = number,
                Err(error) => camera.problem = Some(error.to_string()),
            }
        }
    });
}

/// Move every rig by what its controller says, and tell the world where it is.
fn move_rigs(time: Res<Time>, requests: NonSend<Requests>, mut rigs: Query<(&mut Rig, &mut Transform)>) {
    for (mut rig, mut transform) in &mut rigs {
        let rig = &mut *rig;
        if let Ok(Some(state)) = rig.ports.controller.latest() {
            rig.heard = Some((*state, Instant::now()));
        }
        if rig.paused {
            continue;
        }
        // A controller that has gone quiet must not keep the rig moving.
        let patience = Duration::from_millis(rig.params.timeout_ms);
        let controller = rig.heard.filter(|(_, at)| at.elapsed() <= patience).map(|(state, _)| state);
        rig.state.step(controller.as_ref(), &rig.params, time.delta());
        let pose = rig.state.pose();
        *transform = placed(pose);
        let mounts = Mounts { rig: pose, count: rig.params.mounts, spacing: rig.params.spacing };
        if let Err(error) = rig.ports.pose.send(pose).and(rig.ports.mounts.send(mounts)) {
            let _ = requests.runtime.report(&rig.name, &error.to_string());
        }
    }
}

/// Put every camera on the mount point of the rig it is connected to.
fn place_cameras(mut cameras: Query<(&CameraNode, &mut Transform)>) {
    for (camera, mut transform) in &mut cameras {
        if let Ok(Some(mounts)) = camera.ports.mounts.latest()
            && let Some(point) = mounts.point(camera.params.mount)
        {
            *transform = placed(point);
        }
    }
}

/// Frames are sent from wherever Bevy finishes reading them. What goes wrong
/// there is passed on here, so that it shows on the node in every tool.
fn report_problems(requests: NonSend<Requests>, mut cameras: Query<&mut CameraNode>) {
    for mut camera in &mut cameras {
        if let Some(problem) = camera.problem.take() {
            let _ = requests.runtime.report(&camera.name, &problem);
        }
    }
}

fn stop_when_asked(requests: NonSend<Requests>, mut exit: MessageWriter<AppExit>) {
    if requests.runtime.stopping() {
        exit.write(AppExit::Success);
    }
}

fn placed(pose: Pose) -> Transform {
    Transform {
        translation: Vec3::from_array(pose.translation),
        rotation: Quat::from_array(pose.rotation),
        ..default()
    }
}

/// A checkered floor and a few coloured landmarks, so that moving, turning
/// and the distance between two cameras are easy to see.
fn build_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    headless: Res<Headless>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let dark = materials.add(Color::srgb(0.11, 0.15, 0.19));
    let light = materials.add(Color::srgb(0.27, 0.33, 0.38));
    for x in -8..8 {
        for z in -8..8 {
            let tile = if (x + z) % 2 == 0 { dark.clone() } else { light.clone() };
            let place =
                Transform::from_xyz(x as f32 + 0.5, -0.04, z as f32 + 0.5).with_scale(Vec3::new(1.0, 0.08, 1.0));
            commands.spawn((Mesh3d(cube.clone()), MeshMaterial3d(tile), place));
        }
    }
    let landmarks = [
        (Color::srgb(0.95, 0.16, 0.12), [-2.3, 0.8, 0.0], [1.0, 1.6, 1.0]),
        (Color::srgb(0.12, 0.7, 0.95), [0.0, 0.5, -0.8], [1.0, 1.0, 1.0]),
        (Color::srgb(0.98, 0.74, 0.08), [2.3, 1.2, -2.0], [0.8, 2.4, 0.8]),
        (Color::srgb(0.3, 0.85, 0.28), [4.5, 0.7, 1.0], [1.4, 1.4, 1.4]),
        (Color::srgb(0.72, 0.22, 0.9), [-4.0, 1.5, -4.0], [0.6, 3.0, 0.6]),
        // A gate: two posts and a crossbar.
        (Color::srgb(0.9, 0.9, 0.85), [-3.0, 2.0, -7.0], [0.2, 4.0, 0.2]),
        (Color::srgb(0.9, 0.9, 0.85), [3.0, 2.0, -7.0], [0.2, 4.0, 0.2]),
        (Color::srgb(0.9, 0.9, 0.85), [0.0, 4.0, -7.0], [6.2, 0.2, 0.2]),
    ];
    for (color, at, size) in landmarks {
        let place = Transform::from_translation(Vec3::from_array(at)).with_scale(Vec3::from_array(size));
        commands.spawn((Mesh3d(cube.clone()), MeshMaterial3d(materials.add(color)), place));
    }
    commands.spawn((
        DirectionalLight { illuminance: 12_000.0, ..default() },
        Transform::from_xyz(4.0, 9.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if headless.0 {
        return;
    }
    // The window shows the whole scene from above, and each camera's picture along the bottom.
    let overview = commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(12.0, 10.0, 16.0).looking_at(Vec3::new(0.0, 0.5, -1.0), Vec3::Y),
        ))
        .id();
    commands.spawn((
        Text::new("Create rig and camera nodes from the workbench, then wire controller -> rig -> camera."),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        Node { position_type: PositionType::Absolute, left: px(20), top: px(18), ..default() },
        UiTargetCamera(overview),
    ));
    let row = Node {
        position_type: PositionType::Absolute,
        left: px(20),
        bottom: px(20),
        flex_direction: FlexDirection::Row,
        column_gap: px(12),
        ..default()
    };
    let previews = commands.spawn((row, UiTargetCamera(overview))).id();
    commands.insert_resource(Previews(previews));
}
