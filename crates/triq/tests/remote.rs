//! One program asking another runtime to change: discovery, spawning and wiring
//! by a third party that owns none of the nodes.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use triq::prelude::*;
use triq::{Local, NodeState, Remote};

#[derive(Ports)]
struct Bench {
    feed: Output<u64>,
    probe: Input<u64>,
}

#[derive(Ports)]
struct Through {
    input: Input<u64>,
    output: Output<u64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GainParams {
    factor: u64,
}

impl Default for GainParams {
    fn default() -> Self {
        Self { factor: 1 }
    }
}

struct Gain {
    factor: u64,
}

impl Node for Gain {
    type Ports = Through;
    type Params = GainParams;
    fn new(params: GainParams) -> triq::Result<Self> {
        Ok(Self { factor: params.factor })
    }
    fn set_params(&mut self, params: GainParams) -> triq::Result {
        self.factor = params.factor;
        Ok(())
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value * self.factor)?;
        }
        Ok(())
    }
}

#[test]
fn discover_finds_running_runtimes_and_what_they_offer() -> triq::Result {
    let rt = Runtime::builder(&unique("discover")).pool("dsp", 2).start()?;
    rt.register::<Gain>("gain")?;
    let _bench: Bench = rt.attach("bench")?;

    let found = triq::discover()?;
    let inventory = found.iter().find(|inventory| inventory.runtime == rt.name()).expect("the runtime is listed");
    assert_eq!(*inventory, rt.inventory(), "a remote view is the runtime's own view");
    assert_eq!(inventory.factories[0].kind, "gain");
    assert_eq!(inventory.pools[0].name, "dsp");
    assert_eq!(inventory.nodes[0].name, "bench");
    Ok(())
}

#[test]
fn a_third_party_spawns_and_wires_nodes_it_does_not_own() -> triq::Result {
    let worker = Runtime::new(&unique("worker"))?;
    worker.register::<Gain>("gain")?;
    let app = Runtime::new(&unique("app"))?;
    let bench: Bench = app.attach("bench")?;
    let (worker_name, app_name) = (worker.name().to_owned(), app.name().to_owned());

    // From here on, only names are used: this could be any other process.
    let remote = Remote::open(&worker_name)?;
    remote.spawn("gain", "gain1", Placement::Dedicated, json!({ "factor": 3 }))?;
    triq::connect(&format!("{app_name}/bench/feed"), &format!("{worker_name}/gain1/input"))?;
    triq::connect(&format!("{worker_name}/gain1/output"), &format!("{app_name}/bench/probe"))?;
    bench.feed.send(2)?;
    assert_eq!(collect(&bench.probe, 1), [6]);

    remote.set_params("gain1", json!({ "factor": 5 }))?;
    bench.feed.send(2)?;
    assert_eq!(collect(&bench.probe, 1), [10]);

    remote.pause("gain1")?;
    assert_eq!(remote.inventory()?.nodes[0].state, NodeState::Paused);
    remote.resume("gain1")?;

    triq::disconnect(&format!("{app_name}/bench/feed"), &format!("{worker_name}/gain1/input"))?;
    assert_eq!(bench.feed.send(2)?, 0);

    remote.remove("gain1")?;
    assert!(remote.inventory()?.nodes.is_empty());
    Ok(())
}

#[test]
fn a_refused_request_fails_with_the_runtimes_own_error() -> triq::Result {
    let rt = Runtime::new(&unique("refuse"))?;
    rt.register::<Gain>("gain")?;
    let _poses: Poses = rt.attach("poses")?;
    let remote = Remote::open(rt.name())?;

    assert!(matches!(remote.spawn("nope", "x", Placement::Dedicated, json!(null)), Err(Error::Unknown(_))));
    assert!(matches!(remote.spawn("gain", "x", Placement::pool("missing"), json!(null)), Err(Error::Unknown(_))));
    assert!(matches!(remote.remove("ghost"), Err(Error::Unknown(_))));
    remote.spawn("gain", "gain1", Placement::Dedicated, json!(null))?;
    assert!(matches!(remote.spawn("gain", "gain1", Placement::Dedicated, json!(null)), Err(Error::Name(_))));
    assert!(matches!(remote.connect("poses/out", "gain1/input"), Err(Error::Mismatch(_))));
    assert!(matches!(Remote::open(&unique("nobody")), Err(Error::Unknown(_))));
    Ok(())
}

#[derive(Ports)]
#[allow(dead_code)]
struct Poses {
    out: Output<Pose>,
}

#[derive(Ports)]
struct Source {
    out: Output<u64>,
}

#[derive(Ports)]
struct Sink {
    input: Input<u64>,
}

#[test]
fn an_input_follows_an_output_through_its_runtimes_restarts() -> triq::Result {
    let consumer = Runtime::new(&unique("consumer"))?;
    let sink: Sink = consumer.attach("sink")?;
    let producer_name = unique("producer");
    consumer.connect(&format!("{producer_name}/source/out"), "sink/input")?;

    for round in 1..=3 {
        // The producing runtime does not exist yet, or was shut down in the previous round.
        let producer = Runtime::new(&producer_name)?;
        let source: Source = producer.attach("source")?;
        assert!(wait_for(|| source.out.subscribers() == 1), "round {round}: the waiting input links by itself");
        source.out.send(round)?;
        assert_eq!(collect(&sink.input, 1), [round]);
    }
    Ok(())
}

#[derive(Ports)]
struct CameraPorts {
    trigger: Input<u64>,
    frames: Output<u64>,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct CameraParams {
    fps: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LensParams {
    zoom: f32,
}

impl Default for LensParams {
    fn default() -> Self {
        Self { zoom: 1.1 }
    }
}

#[test]
fn a_local_factory_shows_settings_as_they_were_written() -> triq::Result {
    let rt = Runtime::new(&unique("lens"))?;
    let _lenses = rt.local_factory::<CameraPorts, LensParams>("lens")?;
    assert_eq!(rt.inventory().factories[0].params, json!({ "zoom": 1.1 }));
    rt.spawn_kind("lens", "wide", Placement::Dedicated, json!({ "zoom": 0.6 }))?;
    assert_eq!(rt.inventory().nodes[0].params, json!({ "zoom": 0.6 }));
    rt.set_params("wide", json!({ "zoom": 0.7 }))?;
    assert_eq!(rt.inventory().nodes[0].params, json!({ "zoom": 0.7 }));
    Ok(())
}

#[test]
fn a_local_factory_hands_ports_to_the_application() -> triq::Result {
    let rt = Runtime::new(&unique("local"))?;
    let cameras = rt.local_factory::<CameraPorts, CameraParams>("camera")?;
    rt.set_factory_info("camera", json!({ "devices": ["front", "rear"] }))?;
    let bench: Bench = rt.attach("bench")?;
    let remote = Remote::open(rt.name())?;

    let factory = &remote.inventory()?.factories[0];
    assert_eq!((factory.kind.as_str(), factory.local), ("camera", true));
    assert_eq!(factory.ports, CameraPorts::spec());
    assert_eq!(factory.info, json!({ "devices": ["front", "rear"] }));
    assert!(cameras.try_next().is_none());

    // The request returns at once; the application picks the node up when it gets to it.
    remote.spawn("camera", "left", Placement::Dedicated, json!({ "fps": 30 }))?;
    remote.connect("bench/feed", "left/trigger")?;
    remote.connect("left/frames", "bench/probe")?;
    bench.feed.send(7)?;

    let Some(Local::Spawn { name, params, ports }) = cameras.try_next() else { panic!("a spawn request") };
    assert_eq!((name.as_str(), params), ("left", CameraParams { fps: 30 }));
    assert_eq!(*ports.trigger.recv()?.expect("sent before the application had the ports"), 7);
    ports.frames.send(1)?;
    assert_eq!(collect(&bench.probe, 1), [1]);
    let node = &remote.inventory()?.nodes[1];
    assert_eq!((node.name.as_str(), node.kind.as_str(), &node.placement), ("left", "camera", &None));

    remote.set_params("left", json!({ "fps": 60 }))?;
    let Some(Local::Params { name, params }) = cameras.try_next() else { panic!("a params request") };
    assert_eq!((name.as_str(), params), ("left", CameraParams { fps: 60 }));

    // Pausing is passed on too; what it means is up to the application.
    remote.pause("left")?;
    assert!(matches!(cameras.try_next(), Some(Local::Pause { name }) if name == "left"));
    assert_eq!(remote.inventory()?.nodes[1].state, NodeState::Paused);
    remote.resume("left")?;
    assert!(matches!(cameras.try_next(), Some(Local::Resume { name }) if name == "left"));
    assert_eq!(remote.inventory()?.nodes[1].state, NodeState::Running);
    assert!(matches!(remote.pause("bench"), Err(Error::Mismatch(_))), "nobody was asked to honour a pause there");

    // The application says when something is wrong with a node it runs.
    rt.report("left", "sensor unplugged")?;
    let node = &remote.inventory()?.nodes[1];
    assert_eq!((node.errors, node.last_error.as_deref()), (1, Some("sensor unplugged")));
    // Said again, the same problem is counted but is not news: an application
    // may report it every time round its loop.
    let revision = remote.inventory()?.revision;
    rt.report("left", "sensor unplugged")?;
    assert_eq!((remote.inventory()?.nodes[1].errors, remote.inventory()?.revision), (2, revision));
    rt.report("left", "sensor on fire")?;
    assert!(remote.inventory()?.revision > revision);

    remote.remove("left")?;
    let Some(Local::Remove { name }) = cameras.try_next() else { panic!("a remove request") };
    assert_eq!(name, "left");
    assert_eq!(remote.inventory()?.nodes.len(), 1);

    drop(cameras);
    assert!(remote.inventory()?.factories.is_empty(), "the factory goes with its handle");

    // A program with its own loop looks at this instead of sleeping in `wait`.
    assert!(!rt.stopping());
    remote.shutdown()?;
    assert!(rt.stopping());
    Ok(())
}

#[test]
fn a_large_inventory_is_not_cut_off() -> triq::Result {
    let rt = Runtime::new(&unique("large"))?;
    let _nodes: Vec<Bench> = (0..150).map(|i| rt.attach(&format!("node{i}"))).collect::<triq::Result<_>>()?;
    let inventory = Remote::open(rt.name())?.inventory()?;
    assert_eq!(inventory.nodes.len(), 150);
    assert!(serde_json::to_vec(&inventory).unwrap().len() > 64 * 1024, "larger than zegra's control limit");
    Ok(())
}

#[test]
fn several_clients_are_served_at_once() -> triq::Result {
    let rt = Runtime::new(&unique("clients"))?;
    let name = rt.name().to_owned();
    let clients: Vec<_> = (0..4)
        .map(|_| {
            let name = name.clone();
            std::thread::spawn(move || -> triq::Result {
                let remote = Remote::open(&name)?;
                for _ in 0..25 {
                    assert_eq!(remote.inventory()?.runtime, name);
                }
                Ok(())
            })
        })
        .collect();
    for client in clients {
        client.join().unwrap()?;
    }
    Ok(())
}

#[test]
fn a_shutdown_request_ends_wait() -> triq::Result {
    let rt = Runtime::new(&unique("shutdown"))?;
    let name = rt.name().to_owned();
    let waiting = std::thread::spawn(move || rt.wait());
    std::thread::sleep(Duration::from_millis(20));
    assert!(!waiting.is_finished());
    Remote::open(&name)?.shutdown()?;
    assert!(wait_for(|| waiting.is_finished()));
    Ok(())
}
