//! A tool is told when anything on the machine changes, instead of asking over and over.
//! This file holds one test so that no other test's runtimes are heard.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use triq::Watch;
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

#[derive(Default, Serialize, Deserialize)]
struct GainParams {
    factor: u64,
}

struct Gain(u64);

impl Node for Gain {
    type Ports = Through;
    type Params = GainParams;
    fn new(params: GainParams) -> triq::Result<Self> {
        Ok(Self(params.factor))
    }
    fn set_params(&mut self, params: GainParams) -> triq::Result {
        self.0 = params.factor;
        Ok(())
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value * self.0)?;
        }
        Ok(())
    }
}

/// True if a change is announced within a second; then swallow what follows closely.
fn heard(watch: &Watch) -> bool {
    let heard = watch.wait(Duration::from_secs(1)).unwrap();
    while watch.wait(Duration::from_millis(5)).unwrap() {}
    heard
}

#[test]
fn a_watcher_hears_every_change_and_nothing_else() -> triq::Result {
    let watch = Watch::new()?;
    while watch.wait(Duration::from_millis(5))? {}

    let name = unique("watched");
    let rt = Runtime::new(&name)?;
    assert!(heard(&watch), "a runtime starting is announced");

    let revision = std::cell::Cell::new(rt.inventory().revision);
    let check = |what: &str| {
        assert!(heard(&watch), "{what} is announced");
        let now = rt.inventory().revision;
        assert!(now > revision.get(), "{what} moves the runtime's revision on");
        revision.set(now);
    };
    rt.register::<Gain>("gain")?;
    check("a factory");
    rt.spawn_kind("gain", "gain1", Placement::Dedicated, json!({ "factor": 2 }))?;
    check("a node");
    let bench: Bench = rt.attach("bench")?;
    check("a node the application holds");
    rt.connect("bench/feed", "gain1/input")?;
    check("a connection");
    rt.connect("gain1/output", "bench/probe")?;
    check("another connection");
    rt.set_params("gain1", json!({ "factor": 3 }))?;
    check("new settings");
    rt.pause("gain1")?;
    check("a pause");
    rt.resume("gain1")?;
    check("a resume");
    rt.set_factory_info("gain", json!("hello"))?;
    check("factory information");
    rt.disconnect("bench/feed", "gain1/input")?;
    check("a disconnection");
    let before = revision.get();
    rt.remove("gain1")?;
    check("a removal");
    // The probe heard the node that is gone. It lets go of it a moment later, which is one more
    // change: it may come so soon after the removal that the two are heard as one.
    assert!(wait_for(|| rt.inventory().nodes[0].inputs[0].sources.is_empty()), "the probe lets go");
    while watch.wait(Duration::from_millis(5))? {}
    assert!(rt.inventory().revision >= before + 2, "letting go moves the runtime's revision on again");
    revision.set(rt.inventory().revision);

    // Messages are not changes.
    rt.connect("bench/feed", "bench/probe")?;
    check("a loop");
    for value in 0..20 {
        bench.feed.send(value)?;
    }
    assert_eq!(collect(&bench.probe, 20).len(), 20);
    assert!(!watch.wait(Duration::from_millis(50))?, "data flowing is not announced");
    assert_eq!(rt.inventory().revision, revision.get());

    drop(bench);
    check("an application dropping its ports");

    drop(rt);
    assert!(heard(&watch), "a runtime stopping is announced");
    assert!(triq::discover()?.iter().all(|inventory| inventory.runtime != name), "and by then it is gone");
    Ok(())
}
