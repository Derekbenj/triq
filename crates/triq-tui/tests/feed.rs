//! The app's own look at the machine and what it asks of it, against a runtime of the test's.
use serde_json::json;
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq::{NodeState, Placement};
use triq_tui::feed::{Ask, Feed};

#[derive(Ports)]
struct Through {
    input: Input<u64>,
    output: Output<u64>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Gain {
    gain: u64,
    offset: u64,
}

/// Sends on each value it is sent, times its gain, plus its offset.
struct Amplifier(Gain);

impl Node for Amplifier {
    type Ports = Through;
    type Params = Gain;
    fn new(settings: Gain) -> triq::Result<Self> {
        Ok(Self(settings))
    }
    fn set_params(&mut self, settings: Gain) -> triq::Result {
        self.0 = settings;
        Ok(())
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value * self.0.gain + self.0.offset)?;
        }
        Ok(())
    }
}

/// Wait until `done` holds; false when three seconds pass first.
fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

#[test]
fn the_feed_sees_the_machine_and_what_is_asked_through_it_is_done() -> triq::Result {
    let name = format!("tui{}-feed", std::process::id());
    let runtime = Runtime::builder(&name).pool("work", 1).start()?;
    runtime.register::<Amplifier>("amplifier")?;
    let feed = Feed::start();
    let seen = |feed: &Feed| feed.machine().into_iter().find(|inventory| inventory.runtime == name);
    let node = |feed: &Feed, node: &str| seen(feed)?.nodes.into_iter().find(|each| each.name == node);
    assert!(wait_for(|| seen(&feed).is_some()), "the runtime is seen");
    assert_eq!(seen(&feed).unwrap().factories[0].kind, "amplifier");

    // Two nodes are asked for, and wired one to the next.
    for (node, placement) in [("first", Placement::Dedicated), ("second", Placement::pool("work"))] {
        let params = json!({ "gain": 2 });
        feed.ask(Ask::Spawn { runtime: name.clone(), kind: "amplifier".into(), name: node.into(), placement, params });
    }
    assert!(wait_for(|| node(&feed, "first").is_some() && node(&feed, "second").is_some()), "both are made");
    let (from, to) = (format!("{name}/first/output"), format!("{name}/second/input"));
    feed.ask(Ask::Connect { from: from.clone(), to: to.clone() });
    let heard = from.clone();
    let wired = move |feed: &Feed| {
        node(feed, "second").is_some_and(|node| node.inputs[0].sources.iter().any(|source| source.output == heard))
    };
    assert!(wait_for(|| wired(&feed)), "they are wired");

    // One setting is changed, and the other left; the node is paused and started again.
    feed.ask(Ask::Change { runtime: name.clone(), node: "first".into(), changes: json!({ "offset": 7 }) });
    assert!(wait_for(|| node(&feed, "first").unwrap().params == json!({ "gain": 2, "offset": 7 })));
    feed.ask(Ask::Pause { runtime: name.clone(), node: "first".into() });
    assert!(wait_for(|| node(&feed, "first").unwrap().state == NodeState::Paused));
    feed.ask(Ask::Resume { runtime: name.clone(), node: "first".into() });
    assert!(wait_for(|| node(&feed, "first").unwrap().state == NodeState::Running));

    // What cannot be done is said, once, in words that say what was asked.
    assert_eq!(feed.said(), None);
    feed.ask(Ask::Pause { runtime: name.clone(), node: "nobody".into() });
    let mut said = None;
    assert!(wait_for(|| {
        said = said.take().or_else(|| feed.said());
        said.is_some()
    }));
    let said = said.unwrap();
    assert!(said.contains(&format!("pause {name}/nobody")) && feed.said().is_none(), "{said}");

    // The wire is cut, a node removed, and the runtime told to stop.
    feed.ask(Ask::Disconnect { from, to });
    assert!(wait_for(|| !wired(&feed)));
    feed.ask(Ask::Remove { runtime: name.clone(), node: "second".into() });
    assert!(wait_for(|| node(&feed, "second").is_none()));
    feed.ask(Ask::Shutdown { runtime: name.clone() });
    assert!(wait_for(|| runtime.stopping()), "the runtime was asked to stop");
    Ok(())
}
