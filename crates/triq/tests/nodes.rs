//! Nodes that triq runs on its own threads.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use triq::prelude::*;
use triq::{NodeState, PoolInfo};

/// The two ends a test holds itself.
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

/// Wire `bench/feed -> node/input` and `node/output -> bench/probe`.
fn around(rt: &Runtime, node: &str) -> triq::Result<Bench> {
    let bench: Bench = rt.attach(&format!("bench-{node}"))?;
    rt.connect(&format!("bench-{node}/feed"), &format!("{node}/input"))?;
    rt.connect(&format!("{node}/output"), &format!("bench-{node}/probe"))?;
    Ok(bench)
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

#[test]
fn a_dedicated_node_runs_when_its_input_has_data() -> triq::Result {
    let rt = Runtime::new(&unique("dedicated"))?;
    rt.spawn::<Doubler>("doubler", Placement::Dedicated, ())?;
    let bench = around(&rt, "doubler")?;
    for value in 1..=100 {
        bench.feed.send(value)?;
        assert_eq!(collect(&bench.probe, 1), [value * 2]);
    }
    Ok(())
}

#[derive(Ports)]
struct TickerPorts {
    output: Output<u64>,
}

#[derive(Serialize, Deserialize)]
struct TickerParams {
    period_ms: u64,
}

impl Default for TickerParams {
    fn default() -> Self {
        Self { period_ms: 100 }
    }
}

struct Ticker {
    period: Duration,
    count: u64,
}

impl Node for Ticker {
    type Ports = TickerPorts;
    type Params = TickerParams;
    fn new(params: TickerParams) -> triq::Result<Self> {
        Ok(Self { period: Duration::from_millis(params.period_ms), count: 0 })
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(self.period)
    }
    fn run(&mut self, ports: &mut TickerPorts) -> triq::Result {
        self.count += 1;
        ports.output.send(self.count)?;
        Ok(())
    }
}

#[derive(Ports)]
struct Probe {
    input: Input<u64>,
}

#[test]
fn a_node_with_a_period_runs_on_that_period() -> triq::Result {
    let rt = Runtime::new(&unique("period"))?;
    rt.spawn::<Ticker>("ticker", Placement::Dedicated, TickerParams { period_ms: 10 })?;
    let probe: Probe = rt.attach("probe")?;
    rt.connect("ticker/output", "probe/input")?;
    std::thread::sleep(Duration::from_millis(300));
    let ticks = collect(&probe.input, 1);
    assert!((15..=40).contains(&ticks.len()), "about 30 ticks in 300 ms, got {}", ticks.len());
    assert!(ticks.windows(2).all(|pair| pair[1] == pair[0] + 1), "no tick skipped: {ticks:?}");
    Ok(())
}

/// Reports which thread ran it.
struct WhichThread;

impl Node for WhichThread {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while ports.input.recv()?.is_some() {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::thread::current().id().hash(&mut hasher);
            ports.output.send(hasher.finish())?;
        }
        Ok(())
    }
}

#[test]
fn nodes_in_a_pool_share_its_threads_and_each_keeps_its_own() -> triq::Result {
    let rt = Runtime::builder(&unique("pool")).pool("dsp", 2).start()?;
    let mut threads = HashSet::new();
    for index in 0..4 {
        let node = format!("node{index}");
        rt.spawn::<WhichThread>(&node, Placement::pool("dsp"), ())?;
        let bench = around(&rt, &node)?;
        let mut mine = HashSet::new();
        for _ in 0..3 {
            bench.feed.send(0)?;
            mine.extend(collect(&bench.probe, 1));
        }
        assert_eq!(mine.len(), 1, "a pool node stays on one thread");
        threads.extend(mine);
    }
    assert_eq!(threads.len(), 2, "four nodes spread over the pool's two threads");
    Ok(())
}

#[test]
fn an_unknown_pool_is_refused() -> triq::Result {
    let rt = Runtime::new(&unique("nopool"))?;
    assert!(matches!(rt.spawn::<Doubler>("doubler", Placement::pool("missing"), ()), Err(Error::Unknown(_))));
    assert!(rt.inventory().nodes.is_empty());
    assert!(matches!(Runtime::builder(&unique("zero")).pool("empty", 0).start(), Err(Error::Mismatch(_))));
    Ok(())
}

#[test]
fn a_paused_node_does_not_run_until_resumed() -> triq::Result {
    let rt = Runtime::new(&unique("pause"))?;
    rt.spawn::<Doubler>("doubler", Placement::Dedicated, ())?;
    let bench = around(&rt, "doubler")?;
    rt.pause("doubler")?;
    assert_eq!(rt.inventory().nodes.iter().find(|node| node.name == "doubler").unwrap().state, NodeState::Paused);
    bench.feed.send(4)?;
    std::thread::sleep(Duration::from_millis(50));
    assert!(bench.probe.recv()?.is_none());
    rt.resume("doubler")?;
    assert_eq!(collect(&bench.probe, 1), [8], "what arrived meanwhile is processed on resume");
    Ok(())
}

#[test]
fn removing_a_node_frees_its_name_and_its_connections() -> triq::Result {
    let rt = Runtime::new(&unique("remove"))?;
    rt.spawn::<Doubler>("doubler", Placement::Dedicated, ())?;
    let bench = around(&rt, "doubler")?;
    assert_eq!(bench.feed.subscribers(), 1);

    rt.remove("doubler")?;
    assert!(rt.inventory().nodes.iter().all(|node| node.name != "doubler"));
    assert_eq!(bench.feed.subscribers(), 0);
    assert!(matches!(rt.remove("doubler"), Err(Error::Unknown(_))));

    rt.spawn::<Doubler>("doubler", Placement::Dedicated, ())?;
    rt.connect("bench-doubler/feed", "doubler/input")?;
    bench.feed.send(21)?;
    assert_eq!(collect(&bench.probe, 1), [42], "the probe's connection outlived the node it points at");
    Ok(())
}

/// Refuses odd numbers.
struct Fussy;

impl Node for Fussy {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            if *value % 2 == 1 {
                return Err(Error::Other(format!("{} is odd", *value)));
            }
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

#[test]
fn a_failing_run_is_reported_and_the_node_keeps_running() -> triq::Result {
    let rt = Runtime::new(&unique("fussy"))?;
    rt.spawn::<Fussy>("fussy", Placement::Dedicated, ())?;
    let bench = around(&rt, "fussy")?;
    bench.feed.send(3)?;
    let info = || rt.inventory().nodes.into_iter().find(|node| node.name == "fussy").unwrap();
    assert!(wait_for(|| info().errors == 1));
    assert_eq!(info().last_error.as_deref(), Some("3 is odd"));
    assert_eq!(info().state, NodeState::Running);
    bench.feed.send(4)?;
    assert_eq!(collect(&bench.probe, 1), [4]);
    Ok(())
}

struct Explosive;

impl Node for Explosive {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        if ports.input.recv()?.is_some() {
            panic!("boom");
        }
        Ok(())
    }
}

#[test]
fn a_panicking_node_stops_alone() -> triq::Result {
    let rt = Runtime::builder(&unique("panic")).pool("one", 1).start()?;
    rt.spawn::<Explosive>("explosive", Placement::pool("one"), ())?;
    rt.spawn::<Doubler>("doubler", Placement::pool("one"), ())?;
    let trigger = around(&rt, "explosive")?;
    let bench = around(&rt, "doubler")?;
    trigger.feed.send(1)?;
    let state = || rt.inventory().nodes.into_iter().find(|node| node.name == "explosive").unwrap().state;
    assert!(wait_for(|| state() == NodeState::Stopped("boom".into())), "{:?}", state());
    bench.feed.send(5)?;
    assert_eq!(collect(&bench.probe, 1), [10], "the node sharing its thread still runs");
    Ok(())
}

#[derive(Serialize, Deserialize)]
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
fn params_configure_a_node_and_can_change_while_it_runs() -> triq::Result {
    let rt = Runtime::new(&unique("params"))?;
    rt.spawn::<Gain>("gain", Placement::Dedicated, GainParams { factor: 2 })?;
    let bench = around(&rt, "gain")?;
    bench.feed.send(5)?;
    assert_eq!(collect(&bench.probe, 1), [10]);

    rt.set_params("gain", json!({ "factor": 3 }))?;
    bench.feed.send(5)?;
    assert_eq!(collect(&bench.probe, 1), [15]);
    let info = rt.inventory().nodes.into_iter().find(|node| node.name == "gain").unwrap();
    assert_eq!(info.params, json!({ "factor": 3 }));

    assert!(rt.set_params("gain", json!({ "factor": "three" })).is_err());
    rt.spawn::<Doubler>("fixed", Placement::Dedicated, ())?;
    assert!(matches!(rt.set_params("fixed", json!(null)), Err(Error::Mismatch(_))));
    Ok(())
}

/// Settings with a fraction, as most have.
#[derive(Debug, Serialize, Deserialize)]
struct FadeParams {
    level: f32,
}

impl Default for FadeParams {
    fn default() -> Self {
        Self { level: 0.2 }
    }
}

struct Fade;

impl Node for Fade {
    type Ports = Through;
    type Params = FadeParams;
    fn new(_: FadeParams) -> triq::Result<Self> {
        Ok(Fade)
    }
    fn set_params(&mut self, _: FadeParams) -> triq::Result {
        Ok(())
    }
    fn run(&mut self, _: &mut Through) -> triq::Result {
        Ok(())
    }
}

#[test]
fn settings_read_as_they_were_written() -> triq::Result {
    // An `f32` of 0.2 is not exactly 0.2, and must not be shown as 0.20000000298023224.
    let rt = Runtime::new(&unique("fractions"))?;
    rt.register::<Fade>("fade")?;
    assert_eq!(rt.inventory().factories[0].params, json!({ "level": 0.2 }));

    rt.spawn::<Fade>("typed", Placement::Dedicated, FadeParams { level: 0.7 })?;
    rt.spawn_kind("fade", "asked", Placement::Dedicated, json!({ "level": 0.1 }))?;
    let shown = |name: &str| rt.inventory().nodes.into_iter().find(|node| node.name == name).unwrap().params;
    assert_eq!((shown("typed"), shown("asked")), (json!({ "level": 0.7 }), json!({ "level": 0.1 })));
    rt.set_params("typed", json!({ "level": 0.3 }))?;
    assert_eq!(shown("typed"), json!({ "level": 0.3 }));
    Ok(())
}

#[test]
fn a_registered_factory_spawns_nodes_by_kind() -> triq::Result {
    let rt = Runtime::new(&unique("factory"))?;
    rt.register::<Gain>("gain")?;
    assert!(matches!(rt.register::<Gain>("gain"), Err(Error::Name(_))));

    let inventory = rt.inventory();
    let factory = &inventory.factories[0];
    assert_eq!(factory.kind, "gain");
    assert_eq!(factory.ports, Through::spec());
    assert_eq!(factory.params, json!({ "factor": 1 }), "the defaults are advertised");
    assert!(inventory.nodes.is_empty(), "registering creates nothing");

    rt.spawn_kind("gain", "gain1", Placement::Dedicated, json!({ "factor": 4 }))?;
    let bench = around(&rt, "gain1")?;
    bench.feed.send(2)?;
    assert_eq!(collect(&bench.probe, 1), [8]);
    assert_eq!(rt.inventory().nodes.iter().find(|node| node.name == "gain1").unwrap().kind, "gain");

    assert!(matches!(rt.spawn_kind("nope", "x", Placement::Dedicated, json!(null)), Err(Error::Unknown(_))));
    assert!(rt.spawn_kind("gain", "bad", Placement::Dedicated, json!({ "factor": [] })).is_err());
    assert!(rt.inventory().nodes.iter().all(|node| node.name != "bad"));
    Ok(())
}

#[test]
fn an_inventory_says_when_it_was_taken() -> triq::Result {
    // Counters read twice give a rate only with the time between the readings,
    // and only the runtime knows when it read them.
    let rt = Runtime::new(&unique("taken"))?;
    let first = rt.inventory();
    std::thread::sleep(Duration::from_millis(20));
    let between = Duration::from_nanos(rt.inventory().taken_ns - first.taken_ns);
    assert!((Duration::from_millis(20)..Duration::from_millis(500)).contains(&between), "{between:?}");
    Ok(())
}

#[test]
fn the_inventory_describes_pools_and_nodes() -> triq::Result {
    let rt = Runtime::builder(&unique("describe")).pool("dsp", 2).start()?;
    rt.spawn::<Doubler>("doubler", Placement::pool("dsp"), ())?;
    let bench = around(&rt, "doubler")?;
    bench.feed.send(1)?;
    collect(&bench.probe, 1);

    let inventory = rt.inventory();
    assert_eq!(inventory.pools, [PoolInfo { name: "dsp".into(), threads: 2, shared: false }]);
    let node = inventory.nodes.iter().find(|node| node.name == "doubler").unwrap();
    assert_eq!(node.placement, Some(Placement::pool("dsp")));
    assert_eq!(node.state, NodeState::Running);
    assert!(node.runs >= 1);
    let app = inventory.nodes.iter().find(|node| node.name == "bench-doubler").unwrap();
    assert_eq!(app.placement, None, "the application runs this one itself");
    Ok(())
}

#[test]
fn a_chain_across_placements_delivers_in_order() -> triq::Result {
    let rt = Runtime::builder(&unique("chain")).pool("dsp", 2).start()?;
    rt.spawn::<Doubler>("first", Placement::pool("dsp"), ())?;
    rt.spawn::<Doubler>("second", Placement::Dedicated, ())?;
    rt.spawn::<Doubler>("third", Placement::pool("dsp"), ())?;
    let bench: Bench = rt.attach("bench")?;
    rt.connect("bench/feed", "first/input")?;
    rt.connect("first/output", "second/input")?;
    rt.connect("second/output", "third/input")?;
    rt.connect("third/output", "bench/probe")?;
    let sent: Vec<u64> = (1..=500).collect();
    let mut received = Vec::new();
    for chunk in sent.chunks(16) {
        for value in chunk {
            bench.feed.send(*value)?;
        }
        // Stay within the queues: this test is about order, not overload.
        let so_far = received.len();
        assert!(wait_for(|| {
            while let Some(sample) = bench.probe.recv().unwrap() {
                received.push(*sample);
            }
            received.len() == so_far + chunk.len()
        }));
    }
    assert_eq!(received, sent.iter().map(|value| value * 8).collect::<Vec<_>>());
    Ok(())
}

/// Forwards input at once, and sends a zero on every period.
struct Heartbeat;

impl Node for Heartbeat {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn trigger(&self) -> Trigger {
        Trigger::InputOrEvery(Duration::from_millis(50))
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value)?;
        }
        Ok(())
    }
    fn tick(&mut self, ports: &mut Through) -> triq::Result {
        ports.output.send(0)?;
        Ok(())
    }
}

#[test]
fn a_node_can_run_both_on_input_and_on_a_period() -> triq::Result {
    let rt = Runtime::new(&unique("both"))?;
    rt.spawn::<Heartbeat>("heartbeat", Placement::Dedicated, ())?;
    let bench = around(&rt, "heartbeat")?;
    let started = std::time::Instant::now();
    let mut seen = Vec::new();
    for value in 1..=5 {
        bench.feed.send(value)?;
        let asked = std::time::Instant::now();
        assert!(wait_for(|| {
            seen.extend(collect(&bench.probe, 0));
            seen.contains(&value)
        }));
        assert!(asked.elapsed() < Duration::from_millis(25), "input does not wait for the next period");
        std::thread::sleep(Duration::from_millis(60));
    }
    seen.extend(collect(&bench.probe, 0));
    let beats = seen.iter().filter(|value| **value == 0).count();
    let periods = (started.elapsed().as_millis() / 50) as usize;
    assert!(beats >= periods - 2 && beats <= periods + 1, "{beats} heartbeats in {periods} periods");
    seen.retain(|value| *value != 0);
    assert_eq!(seen, [1, 2, 3, 4, 5], "each input was forwarded once, by `run`");
    Ok(())
}

/// Passes numbers on, and leaves the newest where it was told to at birth.
struct Recorder {
    name: String,
    seen: Seen,
}

/// The newest number each recorder passed on, by the recorder's name.
type Seen = Arc<Mutex<BTreeMap<String, u64>>>;

impl Node for Recorder {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self { name: String::new(), seen: Seen::default() })
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            self.seen.lock().unwrap().insert(self.name.clone(), *value);
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

#[test]
fn a_factory_gives_its_nodes_what_settings_cannot_carry() -> triq::Result {
    let rt = Runtime::builder(&unique("given")).pool("work", 2).start()?;
    // Somewhere the application can look. Every node of the kind is handed it at birth, with its own name.
    let seen = Seen::default();
    rt.register_with("recorder", {
        let seen = seen.clone();
        move |name, ()| Ok(Recorder { name: name.to_owned(), seen: seen.clone() })
    })?;
    let factory = &rt.inventory().factories[0];
    assert_eq!((factory.kind.as_str(), factory.local, &factory.ports), ("recorder", false, &Through::spec()));

    // The runtime runs them like any other node, wherever they are placed.
    rt.spawn_kind("recorder", "first", Placement::pool("work"), json!(null))?;
    rt.spawn_kind("recorder", "second", Placement::Dedicated, json!(null))?;
    let (first, second) = (around(&rt, "first")?, around(&rt, "second")?);
    first.feed.send(3)?;
    second.feed.send(4)?;
    assert_eq!((collect(&first.probe, 1), collect(&second.probe, 1)), (vec![3], vec![4]));
    assert_eq!(*seen.lock().unwrap(), BTreeMap::from([("first".to_owned(), 3), ("second".to_owned(), 4)]));

    // A refusal to make one is what the program that asked for it is told.
    rt.register_with::<Recorder>("never", |_, ()| Err(Error::Other("not today".into())))?;
    let refused = rt.spawn_kind("never", "third", Placement::Dedicated, json!(null));
    assert!(matches!(refused, Err(Error::Other(reason)) if reason == "not today"));
    assert!(rt.inventory().nodes.iter().all(|node| node.name != "third"));
    Ok(())
}

/// Forwards one value each time it runs, however many are waiting, after working that many milliseconds on it.
struct OneAtATime {
    work_ms: u64,
}

impl Node for OneAtATime {
    type Ports = Through;
    type Params = u64;
    fn new(work_ms: u64) -> triq::Result<Self> {
        Ok(Self { work_ms })
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        if let Some(value) = ports.input.recv()? {
            std::thread::sleep(Duration::from_millis(self.work_ms));
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

/// Send `count` values to a node while it cannot run.
fn pile_up(rt: &Runtime, node: &str, bench: &Bench, count: u64) -> triq::Result {
    rt.pause(node)?;
    for value in 0..count {
        bench.feed.send(value)?;
    }
    rt.resume(node)
}

#[test]
fn a_node_that_takes_one_message_a_run_is_run_until_none_waits() -> triq::Result {
    let rt = Runtime::new(&unique("one"))?;
    rt.spawn::<OneAtATime>("node", Placement::Dedicated, 0)?;
    let bench = around(&rt, "node")?;
    // Twenty messages wait for the node. Data arriving is one reason to run, however much of it
    // there is: the node is not told twenty times. So it is run again while it leaves some waiting.
    pile_up(&rt, "node", &bench, 20)?;
    assert_eq!(collect(&bench.probe, 20), (0..20).collect::<Vec<u64>>());
    Ok(())
}

/// Reads nothing.
struct Deaf;

impl Node for Deaf {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, _: &mut Through) -> triq::Result {
        Ok(())
    }
}

#[test]
fn a_node_that_takes_nothing_is_not_run_over_and_over() -> triq::Result {
    let rt = Runtime::new(&unique("deaf"))?;
    rt.spawn::<Deaf>("node", Placement::Dedicated, ())?;
    let bench = around(&rt, "node")?;
    for value in 0..5 {
        bench.feed.send(value)?;
    }
    std::thread::sleep(Duration::from_millis(200));
    // Five messages wait for it still. It was run for their arriving, and then left alone.
    let runs = rt.inventory().nodes.iter().find(|node| node.name == "node").expect("the node").runs;
    assert!((1..=6).contains(&runs), "it ran {runs} times");
    Ok(())
}

#[test]
fn a_node_that_is_behind_does_not_keep_its_thread_from_the_others() -> triq::Result {
    // Two nodes on the one thread of a pool.
    let rt = Runtime::builder(&unique("fair")).pool("one", 1).start()?;
    rt.spawn::<OneAtATime>("busy", Placement::pool("one"), 2)?;
    rt.spawn::<OneAtATime>("other", Placement::pool("one"), 0)?;
    let (busy, other) = (around(&rt, "busy")?, around(&rt, "other")?);
    // Thirty messages wait for the busy one: sixty milliseconds of work, a message at a time.
    pile_up(&rt, "busy", &busy, 30)?;
    // The other has its turn in between, not after all of that.
    let asked = std::time::Instant::now();
    other.feed.send(7)?;
    assert_eq!(collect(&other.probe, 1), [7]);
    let waited = asked.elapsed();
    assert!(waited < Duration::from_millis(30), "the other node waited {waited:?}");
    assert_eq!(collect(&busy.probe, 30).len(), 30, "and the busy one gets through all of its own");
    Ok(())
}
