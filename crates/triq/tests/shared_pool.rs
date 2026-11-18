//! The optional dispatcher: a pool whose free threads take whichever node is ready.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use triq::PoolInfo;
use triq::prelude::*;

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

fn around(rt: &Runtime, node: &str) -> triq::Result<Bench> {
    let bench: Bench = rt.attach(&format!("bench-{node}"))?;
    rt.connect(&format!("bench-{node}/feed"), &format!("{node}/input"))?;
    rt.connect(&format!("{node}/output"), &format!("bench-{node}/probe"))?;
    Ok(bench)
}

/// Forwards each value after sleeping that many milliseconds.
struct Sleeper;

impl Node for Sleeper {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            std::thread::sleep(Duration::from_millis(*value));
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

/// Blocks one node for 400 ms, then measures how long two others take to answer.
fn answer_times_beside_a_blocked_node(rt: &Runtime) -> triq::Result<Vec<Duration>> {
    for node in ["a", "b", "c"] {
        rt.spawn::<Sleeper>(node, Placement::pool("work"), ())?;
    }
    let (a, b, c) = (around(rt, "a")?, around(rt, "b")?, around(rt, "c")?);
    a.feed.send(400)?;
    std::thread::sleep(Duration::from_millis(20));
    let mut times = Vec::new();
    for bench in [&b, &c] {
        let asked = Instant::now();
        bench.feed.send(0)?;
        assert_eq!(collect(&bench.probe, 1), [0]);
        times.push(asked.elapsed());
    }
    assert_eq!(collect(&a.probe, 1), [400], "the blocked node finishes too");
    Ok(times)
}

#[test]
fn in_a_shared_pool_a_blocked_node_delays_nobody() -> triq::Result {
    let rt = Runtime::builder(&unique("shared")).shared_pool("work", 2).start()?;
    assert_eq!(rt.inventory().pools, [PoolInfo { name: "work".into(), threads: 2, shared: true }]);
    let times = answer_times_beside_a_blocked_node(&rt)?;
    assert!(times.iter().all(|time| *time < Duration::from_millis(100)), "{times:?}");
    Ok(())
}

#[test]
fn in_an_ordinary_pool_a_blocked_node_delays_the_nodes_on_its_thread() -> triq::Result {
    // Three nodes on two threads: one of the two others shares the blocked thread.
    let rt = Runtime::builder(&unique("pinned")).pool("work", 2).start()?;
    let times = answer_times_beside_a_blocked_node(&rt)?;
    assert!(times.iter().any(|time| *time > Duration::from_millis(200)), "{times:?}");
    assert!(times.iter().any(|time| *time < Duration::from_millis(100)), "{times:?}");
    Ok(())
}

static INSIDE: AtomicBool = AtomicBool::new(false);
static OVERLAPS: AtomicU64 = AtomicU64::new(0);

/// Notices if two threads are ever inside `run` together.
struct Exclusive;

impl Node for Exclusive {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        if INSIDE.swap(true, Ordering::SeqCst) {
            OVERLAPS.fetch_add(1, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_micros(300));
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value)?;
        }
        INSIDE.store(false, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn a_node_in_a_shared_pool_runs_on_one_thread_at_a_time_and_in_order() -> triq::Result {
    let rt = Runtime::builder(&unique("exclusive")).shared_pool("work", 4).start()?;
    rt.spawn::<Exclusive>("node", Placement::pool("work"), ())?;
    let bench = around(&rt, "node")?;
    let mut received = Vec::new();
    for value in 1..=400 {
        bench.feed.send(value)?;
        std::thread::sleep(Duration::from_micros(100));
        while let Some(sample) = bench.probe.recv()? {
            received.push(*sample);
        }
    }
    assert!(wait_for(|| {
        while let Some(sample) = bench.probe.recv().unwrap() {
            received.push(*sample);
        }
        received.len() == 400
    }));
    assert_eq!(received, (1..=400).collect::<Vec<u64>>());
    assert_eq!(OVERLAPS.load(Ordering::SeqCst), 0);
    Ok(())
}

#[derive(Ports)]
struct TickerPorts {
    output: Output<u64>,
}

struct Ticker(u64);

impl Node for Ticker {
    type Ports = TickerPorts;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self(0))
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(Duration::from_millis(5))
    }
    fn run(&mut self, ports: &mut TickerPorts) -> triq::Result {
        self.0 += 1;
        ports.output.send(self.0)?;
        Ok(())
    }
}

#[derive(Ports)]
struct Probe {
    input: Input<u64>,
}

#[test]
fn periodic_nodes_and_removal_work_in_a_shared_pool() -> triq::Result {
    let rt = Runtime::builder(&unique("mixed")).shared_pool("work", 2).start()?;
    rt.spawn::<Ticker>("ticker", Placement::pool("work"), ())?;
    let probe: Probe = rt.attach("probe")?;
    rt.connect("ticker/output", "probe/input")?;
    assert_eq!(collect(&probe.input, 5)[..5], [1, 2, 3, 4, 5]);

    rt.remove("ticker")?;
    while probe.input.recv()?.is_some() {}
    std::thread::sleep(Duration::from_millis(30));
    assert!(probe.input.recv()?.is_none(), "a removed node no longer runs");
    rt.spawn::<Ticker>("ticker", Placement::pool("work"), ())?;
    assert_eq!(collect(&probe.input, 1)[0], 1, "and its name is free again");
    Ok(())
}

/// Forwards one value each time it runs, however many are waiting.
struct OneAtATime;

impl Node for OneAtATime {
    type Ports = Through;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        if let Some(value) = ports.input.recv()? {
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

#[test]
fn in_a_shared_pool_too_a_node_is_run_until_nothing_waits_for_it() -> triq::Result {
    let rt = Runtime::builder(&unique("sips")).shared_pool("work", 2).start()?;
    rt.spawn::<OneAtATime>("node", Placement::pool("work"), ())?;
    let bench = around(&rt, "node")?;
    rt.pause("node")?;
    for value in 0..20 {
        bench.feed.send(value)?;
    }
    rt.resume("node")?;
    assert_eq!(collect(&bench.probe, 20), (0..20).collect::<Vec<u64>>());
    // Removed while more keeps arriving for it, it is let go of all the same.
    let flood = std::thread::scope(|scope| {
        let sending = scope.spawn(|| (0..2000).try_for_each(|value| bench.feed.send(value).map(|_| ())));
        std::thread::sleep(Duration::from_millis(5));
        let removed = rt.remove("node");
        (sending.join().expect("the sender"), removed)
    });
    assert!(flood.1.is_ok(), "{:?}", flood.1);
    Ok(())
}
