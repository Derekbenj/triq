//! Nodes that are called on more periods than one.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;

#[derive(Ports)]
struct StationPorts {
    heard: Input<u64>,
    echo: Output<u64>,
    fast: Output<u64>,
    slow: Output<u64>,
}

/// The periods of a station, and whether data that arrives wakes it.
#[derive(Serialize, Deserialize)]
struct Rates {
    first_ms: u64,
    second_ms: u64,
    listens: bool,
}

impl Default for Rates {
    fn default() -> Self {
        Self { first_ms: 10, second_ms: 50, listens: false }
    }
}

/// Sends what it hears on at once, a count on its first period, and another on its second.
struct Station {
    rates: Rates,
    counts: [u64; 2],
}

impl Node for Station {
    type Ports = StationPorts;
    type Params = Rates;
    fn new(rates: Rates) -> triq::Result<Self> {
        Ok(Self { rates, counts: [0; 2] })
    }
    fn set_params(&mut self, rates: Rates) -> triq::Result {
        self.rates = rates;
        Ok(())
    }
    fn trigger(&self) -> Trigger {
        let (first, second) = (Duration::from_millis(self.rates.first_ms), Duration::from_millis(self.rates.second_ms));
        let first = if self.rates.listens { Trigger::InputOrEvery(first) } else { Trigger::Every(first) };
        first.and_every(second)
    }
    fn run(&mut self, ports: &mut StationPorts) -> triq::Result {
        while let Some(value) = ports.heard.recv()? {
            ports.echo.send(*value)?;
        }
        Ok(())
    }
    fn tick(&mut self, ports: &mut StationPorts, period: usize) -> triq::Result {
        self.counts[period] += 1;
        match period {
            0 => ports.fast.send(self.counts[0])?,
            _ => ports.slow.send(self.counts[1])?,
        };
        Ok(())
    }
}

/// The other end of every port of a station, held by a test.
#[derive(Ports)]
struct Bench {
    feed: Output<u64>,
    echo: Input<u64>,
    fast: Input<u64>,
    slow: Input<u64>,
}

fn around(rt: &Runtime, node: &str) -> triq::Result<Bench> {
    let bench: Bench = rt.attach(&format!("bench-{node}"))?;
    rt.connect(&format!("bench-{node}/feed"), &format!("{node}/heard"))?;
    for port in ["echo", "fast", "slow"] {
        rt.connect(&format!("{node}/{port}"), &format!("bench-{node}/{port}"))?;
    }
    Ok(bench)
}

/// True if each count is one more than the one before it: no call was left out, and none made twice.
fn in_step(counts: &[u64]) -> bool {
    counts.windows(2).all(|pair| pair[1] == pair[0] + 1)
}

#[test]
fn a_node_with_two_periods_is_called_on_each_and_told_which() -> triq::Result {
    let rt = Runtime::builder(&unique("periods")).pool("own", 1).shared_pool("any", 2).start()?;
    // On a thread of its own, on a thread of a pool, and on whichever thread of a pool is free.
    let placements = [Placement::Dedicated, Placement::pool("own"), Placement::pool("any")];
    let mut benches = Vec::new();
    for (number, placement) in placements.into_iter().enumerate() {
        let node = format!("station{number}");
        rt.spawn::<Station>(&node, placement, Rates::default())?;
        benches.push(around(&rt, &node)?);
    }
    benches.iter().try_for_each(|bench| bench.feed.send(7).map(drop))?;
    std::thread::sleep(Duration::from_millis(500));
    for bench in &benches {
        let (fast, slow) = (collect(&bench.fast, 1), collect(&bench.slow, 1));
        assert!((30..=60).contains(&fast.len()), "about 50 of 10 ms in 500 ms, got {}", fast.len());
        assert!((6..=12).contains(&slow.len()), "about 10 of 50 ms in 500 ms, got {}", slow.len());
        assert!(in_step(&fast) && in_step(&slow), "{fast:?} {slow:?}");
        // A node that is only called on its periods is not woken by what arrives.
        assert!(bench.echo.recv()?.is_none());
    }
    Ok(())
}

#[test]
fn a_node_hears_its_inputs_at_once_and_has_two_periods_besides() -> triq::Result {
    let rt = Runtime::new(&unique("hears"))?;
    rt.spawn::<Station>("station", Placement::Dedicated, Rates { first_ms: 30, second_ms: 90, listens: true })?;
    let bench = around(&rt, "station")?;
    let started = Instant::now();
    for value in 1..=5 {
        bench.feed.send(value)?;
        let asked = Instant::now();
        assert_eq!(collect(&bench.echo, 1), [value], "each is sent on once, by `run`");
        assert!(asked.elapsed() < Duration::from_millis(25), "what arrives does not wait for a period");
        std::thread::sleep(Duration::from_millis(70));
    }
    let (fast, slow) = (collect(&bench.fast, 1), collect(&bench.slow, 1));
    let passed = started.elapsed().as_millis() as usize;
    let about = |count: usize, period: usize| count + 2 >= passed / period && count <= passed / period + 1;
    assert!(about(fast.len(), 30) && about(slow.len(), 90), "{} and {} calls in {passed} ms", fast.len(), slow.len());
    assert!(in_step(&fast) && in_step(&slow), "{fast:?} {slow:?}");
    Ok(())
}

#[test]
fn the_periods_of_a_node_change_with_its_settings() -> triq::Result {
    let rt = Runtime::new(&unique("retimed"))?;
    rt.spawn::<Station>("station", Placement::Dedicated, Rates::default())?;
    let bench = around(&rt, "station")?;
    assert!(wait_for(|| bench.slow.recv().unwrap().is_some()), "it runs");

    // The other way round: the first period is now the long one.
    rt.set_params("station", json!({ "first_ms": 50, "second_ms": 10, "listens": false }))?;
    std::thread::sleep(Duration::from_millis(60));
    let _ = (collect(&bench.fast, 0), collect(&bench.slow, 0));
    std::thread::sleep(Duration::from_millis(300));
    let (first, second) = (collect(&bench.fast, 1), collect(&bench.slow, 1));
    assert!((3..=8).contains(&first.len()), "about 6 of 50 ms in 300 ms, got {}", first.len());
    assert!((18..=36).contains(&second.len()), "about 30 of 10 ms in 300 ms, got {}", second.len());
    Ok(())
}

#[derive(Ports)]
struct Counted {
    count: Output<u64>,
}

/// Two periods of the same length. On the first it takes its time and fails, and on the second it counts.
struct Uneven(u64);

impl Node for Uneven {
    type Ports = Counted;
    type Params = ();
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self(0))
    }
    fn trigger(&self) -> Trigger {
        Trigger::Every(Duration::from_millis(5)).and_every(Duration::from_millis(5))
    }
    fn run(&mut self, _: &mut Counted) -> triq::Result {
        Ok(())
    }
    fn tick(&mut self, ports: &mut Counted, period: usize) -> triq::Result {
        if period == 0 {
            // Longer than the periods: by the time it is over, both have elapsed again.
            std::thread::sleep(Duration::from_millis(20));
            return Err(Error::Other("the first period fails".into()));
        }
        self.0 += 1;
        ports.count.send(self.0)?;
        Ok(())
    }
}

#[derive(Ports)]
struct Counter {
    count: Input<u64>,
}

#[test]
fn a_period_that_fails_does_not_cost_another_its_call() -> triq::Result {
    let rt = Runtime::new(&unique("uneven"))?;
    rt.spawn::<Uneven>("uneven", Placement::Dedicated, ())?;
    let counter: Counter = rt.attach("counter")?;
    rt.connect("uneven/count", "counter/count")?;
    std::thread::sleep(Duration::from_millis(300));
    let counts = collect(&counter.count, 1);
    assert!(counts.len() >= 8 && in_step(&counts), "about 15 calls of 20 ms in 300 ms, got {counts:?}");
    let info = rt.inventory().nodes.into_iter().find(|node| node.name == "uneven").unwrap();
    assert!(info.errors >= 8, "{} errors", info.errors);
    assert_eq!(info.last_error.as_deref(), Some("the first period fails"));
    Ok(())
}

/// As many periods of 5 ms as its settings say. It takes longer than that over every call, so
/// all of them have elapsed again by the time it is over, and it says so when it is called for
/// a period that it does not have.
struct Fewer(usize);

impl Node for Fewer {
    type Ports = Counted;
    type Params = usize;
    fn new(periods: usize) -> triq::Result<Self> {
        Ok(Self(periods))
    }
    fn set_params(&mut self, periods: usize) -> triq::Result {
        self.0 = periods;
        Ok(())
    }
    fn trigger(&self) -> Trigger {
        Trigger::Several { input: false, periods: vec![Duration::from_millis(5); self.0] }
    }
    fn run(&mut self, _: &mut Counted) -> triq::Result {
        Ok(())
    }
    fn tick(&mut self, ports: &mut Counted, period: usize) -> triq::Result {
        if period >= self.0 {
            ports.count.send(period as u64)?;
        }
        std::thread::sleep(Duration::from_millis(15));
        Ok(())
    }
}

#[test]
fn a_node_is_not_called_for_a_period_that_its_new_settings_took_away() -> triq::Result {
    let rt = Runtime::new(&unique("fewer"))?;
    rt.spawn::<Fewer>("fewer", Placement::Dedicated, 3)?;
    let counter: Counter = rt.attach("counter")?;
    rt.connect("fewer/count", "counter/count")?;
    let runs = || rt.inventory().nodes.into_iter().find(|node| node.name == "fewer").unwrap().runs;
    // The settings change while it is in a call, with the three periods elapsed behind it.
    for periods in [3, 1, 2, 1] {
        std::thread::sleep(Duration::from_millis(60));
        rt.set_params("fewer", json!(periods))?;
    }
    let before = runs();
    std::thread::sleep(Duration::from_millis(100));
    assert!(runs() > before, "it is still called for the period it has");
    assert_eq!(collect(&counter.count, 0), [0; 0], "and was never called for one it had no more");
    Ok(())
}

/// As many periods as its settings say.
struct Many(usize);

impl Node for Many {
    type Ports = Counted;
    type Params = usize;
    fn new(periods: usize) -> triq::Result<Self> {
        Ok(Self(periods))
    }
    fn trigger(&self) -> Trigger {
        // Written out: what `and_every` makes.
        Trigger::Several { input: false, periods: vec![Duration::from_secs(1); self.0] }
    }
    fn run(&mut self, _: &mut Counted) -> triq::Result {
        Ok(())
    }
}

#[test]
fn a_node_with_more_periods_than_there_is_room_for_is_refused() -> triq::Result {
    let rt = Runtime::new(&unique("many"))?;
    let refused = rt.spawn::<Many>("many", Placement::Dedicated, 64);
    assert!(matches!(&refused, Err(Error::Mismatch(reason)) if reason.contains("63 periods")), "{refused:?}");
    assert!(rt.inventory().nodes.is_empty(), "nothing of it is left behind");
    rt.spawn::<Many>("many", Placement::Dedicated, 63)?;
    Ok(())
}
