//! A pool whose threads never sleep. A message to a node in it costs no thread wake-up, and each
//! of its threads keeps a processor core busy whether or not there is work.
#[path = "support/helpers.rs"]
mod helpers;
use cpu_time::ThreadTime;
use helpers::*;
use serde_json::json;
use std::time::Duration;
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

#[derive(Ports)]
struct Says {
    output: Output<u64>,
}

#[derive(Ports)]
struct Hears {
    probe: Input<u64>,
}

fn around(rt: &Runtime, node: &str) -> triq::Result<Bench> {
    let bench: Bench = rt.attach(&format!("bench-{node}"))?;
    rt.connect(&format!("bench-{node}/feed"), &format!("{node}/input"))?;
    rt.connect(&format!("{node}/output"), &format!("bench-{node}/probe"))?;
    Ok(bench)
}

fn hearing(rt: &Runtime, node: &str) -> triq::Result<Hears> {
    let hears: Hears = rt.attach(&format!("hears-{node}"))?;
    rt.connect(&format!("{node}/output"), &format!("hears-{node}/probe"))?;
    Ok(hears)
}

/// Sends on twice each value it is sent.
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

/// Every 20 ms, says how many microseconds its thread has had a processor for.
struct Clocked;

impl Node for Clocked {
    type Ports = Says;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self)
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(Duration::from_millis(20))
    }
    fn run(&mut self, _: &mut Says) -> triq::Result {
        Ok(())
    }
    fn tick(&mut self, ports: &mut Says, _: usize) -> triq::Result {
        ports.output.send(ThreadTime::now().as_duration().as_micros() as u64)?;
        Ok(())
    }
}

#[test]
fn a_runtime_says_which_of_its_pools_poll() -> triq::Result {
    let builder = Runtime::builder(&unique("kinds"));
    let rt = builder.pool("kept", 2).shared_pool("shared", 3).polling_pool("fast", 1).start()?;
    let pool = |name: &str, threads, shared, polling| PoolInfo { name: name.into(), threads, shared, polling };
    assert_eq!(
        rt.inventory().pools,
        [pool("fast", 1, false, true), pool("kept", 2, false, false), pool("shared", 3, true, false)]
    );
    // The tool's list says so too.
    let listed = std::process::Command::new(env!("CARGO_BIN_EXE_triq")).arg("list").output()?;
    let listed = String::from_utf8_lossy(&listed.stdout).into_owned();
    for pool in ["pool fast: 1 threads, polling\n", "pool kept: 2 threads\n", "pool shared: 3 threads, shared\n"] {
        assert!(listed.contains(pool), "{pool:?} is not in:\n{listed}");
    }
    // A runtime too old to say has no pool that polls.
    let old: PoolInfo = serde_json::from_value(json!({ "name": "dsp", "threads": 2, "shared": false }))?;
    assert_eq!(old, pool("dsp", 2, false, false));
    Ok(())
}

#[test]
fn a_polling_pool_needs_a_thread_and_a_name_of_its_own() {
    let none = Runtime::builder(&unique("none")).polling_pool("fast", 0).start();
    assert!(matches!(none, Err(Error::Mismatch(_))), "{:?}", none.err());
    let twice = Runtime::builder(&unique("twice")).pool("fast", 1).polling_pool("fast", 1).start();
    assert!(matches!(twice, Err(Error::Name(_))), "{:?}", twice.err());
}

#[test]
fn nodes_in_a_polling_pool_run_as_in_any_pool_and_several_share_a_thread() -> triq::Result {
    // Three nodes on two threads: two of them share one.
    let rt = Runtime::builder(&unique("polls")).polling_pool("fast", 2).start()?;
    let names = ["a", "b", "c"];
    for node in names {
        rt.spawn::<Doubler>(node, Placement::pool("fast"), ())?;
    }
    let benches = names.iter().map(|node| around(&rt, node)).collect::<triq::Result<Vec<Bench>>>()?;
    // Each answers what it is sent, all of it and in order.
    for round in 0..50u64 {
        for (index, bench) in benches.iter().enumerate() {
            bench.feed.send(round * 10 + index as u64)?;
        }
        for (index, bench) in benches.iter().enumerate() {
            assert_eq!(
                collect(&bench.probe, 1),
                [(round * 10 + index as u64) * 2],
                "{} in round {round}",
                names[index]
            );
        }
    }
    let placed = |node: &str| rt.inventory().nodes.into_iter().find(|info| info.name == node).unwrap().placement;
    assert_eq!(placed("b"), Some(Placement::pool("fast")));

    // Paused, a node says nothing; resumed, it answers what waited for it. The others go on.
    rt.pause("a")?;
    benches[0].feed.send(7)?;
    benches[1].feed.send(8)?;
    assert_eq!(collect(&benches[1].probe, 1), [16]);
    std::thread::sleep(Duration::from_millis(50));
    assert!(benches[0].probe.recv()?.is_none(), "a paused node ran");
    rt.resume("a")?;
    assert_eq!(collect(&benches[0].probe, 1), [14]);

    // Removed, a node is gone, and the node that shared its thread goes on.
    rt.remove("a")?;
    assert!(rt.inventory().nodes.iter().all(|node| node.name != "a"));
    for (index, bench) in benches.iter().enumerate().skip(1) {
        bench.feed.send(100)?;
        assert_eq!(collect(&bench.probe, 1), [200], "{}", names[index]);
    }

    // A node made later and wired later is run all the same.
    rt.spawn::<Doubler>("d", Placement::pool("fast"), ())?;
    let late = around(&rt, "d")?;
    late.feed.send(21)?;
    assert_eq!(collect(&late.probe, 1), [42]);
    Ok(())
}

#[test]
fn the_threads_of_a_polling_pool_are_busy_all_the_time_and_those_of_another_pool_only_while_they_work() -> triq::Result
{
    let rt = Runtime::builder(&unique("busy")).polling_pool("fast", 1).pool("kept", 1).start()?;
    rt.spawn::<Clocked>("polls", Placement::pool("fast"), ())?;
    rt.spawn::<Clocked>("sleeps", Placement::pool("kept"), ())?;
    let (polls, sleeps) = (hearing(&rt, "polls")?, hearing(&rt, "sleeps")?);
    // Sixteen ticks are fifteen periods of 20 ms: 300 ms. A node in a polling pool is ticked on
    // its period as any other is.
    let busy_for = |hears: &Hears| {
        let said = collect(&hears.probe, 16);
        assert_eq!(said.len(), 16, "it ticked {} times in two seconds", said.len());
        Duration::from_micros(said[15] - said[0])
    };
    let (polling, sleeping) = (busy_for(&polls), busy_for(&sleeps));
    // The thread that polls had a processor for most of those 300 ms: a third of them at the
    // least, on a machine with much else to do. The one that sleeps had one for the few
    // microseconds each tick takes.
    assert!(polling >= Duration::from_millis(100), "the polling thread was busy for {polling:?} of 300 ms");
    assert!(sleeping <= Duration::from_millis(30), "the sleeping thread was busy for {sleeping:?} of 300 ms");
    Ok(())
}
