//! Changing some of a node's settings and leaving the rest, where `set_params` replaces them all.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use triq::prelude::*;
use triq::{Local, Remote};

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

/// Settings of several sorts: numbers, a list, and settings within settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ScaleParams {
    factor: u64,
    offset: u64,
    steps: Vec<u64>,
    limits: Limits,
    /// A number that the text of it must be read exactly to give back.
    fine: f64,
    pace: Pace,
}

/// A setting that is one of a few names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Pace {
    #[default]
    Slow,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Limits {
    low: u64,
    high: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { low: 0, high: 100 }
    }
}

/// A number that a careless reader of its text gives back one step of rounding away.
const FINE: f64 = 1.0676952664346183e-6;

impl Default for ScaleParams {
    fn default() -> Self {
        Self { factor: 1, offset: 0, steps: vec![1, 2, 3], limits: Limits::default(), fine: FINE, pace: Pace::Slow }
    }
}

/// Sends what it hears, scaled and offset and held to its limits. Told 0, it sends the bits of
/// its `fine` setting, so that a test sees the number the node itself has.
struct Scale(ScaleParams);

impl Node for Scale {
    type Ports = Through;
    type Params = ScaleParams;
    fn new(params: ScaleParams) -> triq::Result<Self> {
        Ok(Self(params))
    }
    fn set_params(&mut self, params: ScaleParams) -> triq::Result {
        if params.limits.low > params.limits.high {
            return Err(Error::Mismatch("low is above high".into()));
        }
        self.0 = params;
        Ok(())
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            let Limits { low, high } = self.0.limits;
            let scaled = (*value * self.0.factor + self.0.offset).clamp(low, high);
            ports.output.send(if *value == 0 { self.0.fine.to_bits() } else { scaled })?;
        }
        Ok(())
    }
}

/// A runtime with one `Scale` node, `scale`, made with `settings`, and both of its ends.
fn scale(tag: &str, settings: serde_json::Value) -> triq::Result<(Runtime, Bench)> {
    let rt = Runtime::new(&unique(tag))?;
    rt.register::<Scale>("scale")?;
    rt.spawn_kind("scale", "scale", Placement::Dedicated, settings)?;
    let bench: Bench = rt.attach("bench")?;
    rt.connect("bench/feed", "scale/input")?;
    rt.connect("scale/output", "bench/probe")?;
    Ok((rt, bench))
}

/// What the node makes of `value`.
fn made(bench: &Bench, value: u64) -> u64 {
    bench.feed.send(value).expect("send");
    collect(&bench.probe, 1)[0]
}

fn shown(rt: &Runtime) -> serde_json::Value {
    rt.inventory().nodes.into_iter().find(|node| node.name == "scale").unwrap().params
}

#[test]
fn the_settings_named_change_and_the_rest_stay_as_they_are() -> triq::Result {
    let (rt, bench) = scale("named", json!({ "factor": 2, "offset": 1 }))?;
    assert_eq!(made(&bench, 5), 11);

    // Only the offset is named. The factor stays 2: it does not go back to its default of 1.
    rt.change_params("scale", json!({ "offset": 7 }))?;
    assert_eq!(made(&bench, 5), 17);
    assert_eq!(shown(&rt)["factor"], 2);
    assert_eq!(shown(&rt)["offset"], 7);

    // Replacing them all is still what `set_params` does: what is left out is its default.
    rt.set_params("scale", json!({ "offset": 7 }))?;
    assert_eq!(made(&bench, 5), 12);
    Ok(())
}

#[test]
fn settings_within_settings_change_one_by_one_and_a_list_changes_whole() -> triq::Result {
    let (rt, bench) = scale("within", json!({ "factor": 10, "limits": { "low": 3, "high": 40 } }))?;
    assert_eq!(made(&bench, 9), 40);

    // Only the high limit is named: the low one stays 3.
    rt.change_params("scale", json!({ "limits": { "high": 60 } }))?;
    assert_eq!(shown(&rt)["limits"], json!({ "low": 3, "high": 60 }));
    assert_eq!(made(&bench, 9), 60);

    // A list has no names in it to change one by one: the new list takes the place of the old.
    rt.change_params("scale", json!({ "steps": [9] }))?;
    assert_eq!(shown(&rt)["steps"], json!([9]));
    assert_eq!(shown(&rt)["limits"], json!({ "low": 3, "high": 60 }), "and nothing else changed");
    Ok(())
}

#[test]
fn a_setting_named_as_null_goes_back_to_its_default() -> triq::Result {
    let (rt, bench) = scale("null", json!({ "factor": 4, "offset": 2, "limits": { "low": 5, "high": 50 } }))?;
    rt.change_params("scale", json!({ "factor": null, "limits": { "low": null } }))?;
    assert_eq!(shown(&rt)["factor"], 1, "the default");
    assert_eq!(shown(&rt)["limits"], json!({ "low": 0, "high": 50 }));
    assert_eq!(shown(&rt)["offset"], 2, "what was not named stays");
    assert_eq!(made(&bench, 5), 7);
    Ok(())
}

#[test]
fn a_tool_works_out_what_a_change_would_make_of_settings_before_it_asks() {
    // What `change_params` makes of a node's settings, for a tool that shows it first.
    let has = json!({ "factor": 2, "steps": [1, 2], "limits": { "low": 3, "high": 40 } });
    let changes = json!({ "factor": 5, "steps": [9], "limits": { "high": null, "middle": 7 }, "new": true });
    assert_eq!(
        triq::merged(has.clone(), changes),
        json!({ "factor": 5, "steps": [9], "limits": { "low": 3, "middle": 7 }, "new": true })
    );
    assert_eq!(triq::merged(has.clone(), json!({})), has, "no change named, none made");
    // Settings that are not named ones, or a change that is not, have nothing to name: the change is all of them.
    assert_eq!(triq::merged(json!(4), json!(7)), json!(7));
    assert_eq!(triq::merged(has, json!([1])), json!([1]));
    assert_eq!(triq::merged(json!(4), json!({ "factor": 5, "gone": null })), json!({ "factor": 5 }));
}

#[test]
fn a_change_that_is_refused_changes_nothing() -> triq::Result {
    let (rt, bench) = scale("refused", json!({ "factor": 2, "offset": 1 }))?;
    let before = shown(&rt);
    // Not a number at all; a setting the node has none of the kind of; and one its own check refuses.
    assert!(rt.change_params("scale", json!({ "factor": "two" })).is_err());
    assert!(rt.change_params("scale", json!({ "limits": 5 })).is_err());
    assert!(matches!(rt.change_params("scale", json!({ "limits": { "low": 500 } })), Err(Error::Mismatch(_))));
    assert_eq!(shown(&rt), before);
    assert_eq!(made(&bench, 5), 11);

    // As with `set_params`: a node that is not there, and one whose settings cannot change.
    assert!(matches!(rt.change_params("nobody", json!({ "factor": 3 })), Err(Error::Unknown(_))));
    assert!(matches!(rt.change_params("bench", json!({ "factor": 3 })), Err(Error::Mismatch(_))));
    Ok(())
}

#[test]
fn a_number_that_was_not_named_is_still_exactly_the_number_it_was() -> triq::Result {
    // The runtime keeps a node's settings as it shows them, which is as text reads. A number
    // read back from its text one step of rounding away would be a setting that changed by
    // itself whenever another one was changed.
    let (rt, bench) = scale("exact", json!({ "factor": 2 }))?;
    assert_eq!(f64::from_bits(made(&bench, 0)), FINE);
    assert_eq!(shown(&rt)["fine"], json!(FINE), "shown as the number it is");

    rt.change_params("scale", json!({ "offset": 3 }))?;
    assert_eq!(f64::from_bits(made(&bench, 0)), FINE, "the node still has the number it had, to the last bit");

    // And all of what the runtime shows, sent back as it is shown, changes nothing either.
    rt.set_params("scale", shown(&rt))?;
    assert_eq!(f64::from_bits(made(&bench, 0)), FINE);
    assert_eq!(made(&bench, 5), 13);
    Ok(())
}

#[test]
fn changes_made_at_the_same_moment_are_all_made() -> triq::Result {
    // Each thread changes a setting of its own. A change worked out from the settings as they
    // were before another change, and made after it, would undo that other change.
    let (rt, _bench) = scale("together", json!({}))?;
    let names = ["factor", "offset", "steps"];
    for round in 1..=200u64 {
        let started = std::sync::Barrier::new(names.len());
        std::thread::scope(|threads| {
            for name in names {
                let (rt, started) = (&rt, &started);
                threads.spawn(move || {
                    started.wait();
                    let value = if name == "steps" { json!([round]) } else { json!(round) };
                    rt.change_params("scale", json!({ name: value })).expect("change");
                });
            }
        });
        let now = shown(&rt);
        assert_eq!((&now["factor"], &now["offset"], &now["steps"]), (&json!(round), &json!(round), &json!([round])));
    }
    Ok(())
}

#[test]
fn a_change_and_new_settings_at_the_same_moment_are_made_one_after_the_other() -> triq::Result {
    // Whichever is first, the factor is the new settings': a change worked out before them and
    // made after them would put back the factor of the round before.
    let (rt, _bench) = scale("either", json!({}))?;
    for round in 1..=200u64 {
        let started = std::sync::Barrier::new(2);
        std::thread::scope(|threads| {
            let (rt, started) = (&rt, &started);
            threads.spawn(move || {
                started.wait();
                rt.set_params("scale", json!({ "factor": round })).expect("set");
            });
            threads.spawn(move || {
                started.wait();
                rt.change_params("scale", json!({ "offset": round })).expect("change");
            });
        });
        let now = shown(&rt);
        assert_eq!(now["factor"], round);
        assert!(now["offset"] == round || now["offset"] == 0, "the change, or the default that came after it");
    }
    Ok(())
}

#[derive(Ports)]
struct Orders {
    input: Input<u64>,
    output: Output<u64>,
}

/// A node that changes another node's settings as it runs: `scale`'s offset becomes what it hears.
struct Foreman(std::sync::Weak<Runtime>);

impl Node for Foreman {
    type Ports = Orders;
    type Params = u64;
    fn new(_: u64) -> triq::Result<Self> {
        unreachable!("made by the test")
    }
    fn set_params(&mut self, _: u64) -> triq::Result {
        Ok(())
    }
    fn run(&mut self, ports: &mut Orders) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            if let Some(rt) = self.0.upgrade() {
                rt.change_params("scale", json!({ "offset": *value }))?;
            }
            ports.output.send(*value)?;
        }
        Ok(())
    }
}

#[test]
fn a_node_changes_another_nodes_settings_while_its_own_are_changed() -> triq::Result {
    // The foreman is given new settings while it runs, and its run changes the settings of
    // `scale`. Were one node's settings held while another's are changed, each would wait for
    // the other for ever.
    let (rt, _bench) = scale("foreman", json!({}))?;
    let rt = std::sync::Arc::new(rt);
    let weak = std::sync::Arc::downgrade(&rt);
    rt.register_with::<Foreman>("foreman", move |_, _| Ok(Foreman(weak.clone())))?;
    rt.spawn_kind("foreman", "foreman", Placement::Dedicated, json!(0))?;
    let orders: Bench = rt.attach("orders")?;
    rt.connect("orders/feed", "foreman/input")?;
    rt.connect("foreman/output", "orders/probe")?;

    // Not a scoped thread: if it does wait for ever, the test is to fail, not to wait with it.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (setter, stop) = (rt.clone(), done.clone());
    std::thread::spawn(move || {
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            setter.set_params("foreman", json!(1)).expect("set");
        }
    });
    for order in 1..=500u64 {
        orders.feed.send(order)?;
        assert_eq!(collect(&orders.probe, 1), [order], "the foreman ran, and did not wait for ever");
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(shown(&rt)["offset"], 500);
    Ok(())
}

#[derive(Ports)]
struct LampPorts {
    #[allow(dead_code, reason = "a node needs a port; this one's is never sent on")]
    light: Output<u64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct LampParams {
    level: u32,
    colour: String,
}

impl Default for LampParams {
    fn default() -> Self {
        Self { level: 10, colour: "white".into() }
    }
}

#[test]
fn a_node_the_application_runs_is_told_all_of_its_settings_with_the_change_in_them() -> triq::Result {
    let rt = Runtime::new(&unique("lamp"))?;
    let lamps = rt.local_factory::<LampPorts, LampParams>("lamp")?;
    rt.spawn_kind("lamp", "desk", Placement::Dedicated, json!({ "level": 60, "colour": "amber" }))?;
    // The node lasts for as long as the application holds its ports.
    let Some(Local::Spawn { ports: _ports, .. }) = lamps.try_next() else { panic!("a spawn request") };

    rt.change_params("desk", json!({ "level": 80 }))?;
    let Some(Local::Params { name, params }) = lamps.try_next() else { panic!("the new settings") };
    assert_eq!((name.as_str(), params), ("desk", LampParams { level: 80, colour: "amber".into() }));
    assert_eq!(rt.inventory().nodes[0].params, json!({ "level": 80, "colour": "amber" }));

    // What is refused never reaches the application.
    assert!(rt.change_params("desk", json!({ "level": "bright" })).is_err());
    assert!(lamps.try_next().is_none());
    Ok(())
}

#[test]
fn another_program_changes_some_of_a_nodes_settings() -> triq::Result {
    let (rt, bench) = scale("remote", json!({ "factor": 2, "offset": 1 }))?;
    let remote = Remote::open(rt.name())?;
    remote.change_params("scale", json!({ "factor": 3 }))?;
    assert_eq!(made(&bench, 5), 16);
    assert_eq!(remote.inventory()?.nodes.iter().find(|node| node.name == "scale").unwrap().params["offset"], 1);
    assert!(remote.change_params("scale", json!({ "factor": "three" })).is_err());
    assert!(matches!(remote.change_params("nobody", json!({})), Err(Error::Unknown(_))));
    Ok(())
}

#[test]
fn a_name_the_node_does_not_know_is_refused_in_the_nodes_own_words_from_another_program_too() -> triq::Result {
    // A runtime built with an earlier triq does not know the request itself, and the caller is
    // told so. A node that does not know a setting's value says so in much the same words: that
    // is the node's refusal, and must reach the caller as it is.
    let (rt, _bench) = scale("pace", json!({}))?;
    let remote = Remote::open(rt.name())?;
    remote.change_params("scale", json!({ "pace": "fast" }))?;
    assert_eq!(shown(&rt)["pace"], "fast");
    for refused in [
        rt.change_params("scale", json!({ "pace": "turbo" })),
        remote.change_params("scale", json!({ "pace": "turbo" })),
    ] {
        let said = refused.expect_err("no such pace").to_string();
        assert!(said.contains("turbo") && !said.contains("earlier triq"), "{said}");
    }
    assert_eq!(shown(&rt)["pace"], "fast");
    Ok(())
}

#[derive(Ports)]
struct Hears {
    #[allow(dead_code, reason = "a node needs a port; nothing is sent to this one")]
    input: Input<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Three {
    a: u64,
    b: u64,
    c: u64,
}

/// What a test holds a node's `set_params` at: the node says it is there, and waits to be let on.
struct Gate {
    there: std::sync::mpsc::Sender<()>,
    on: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

/// A node that takes any settings: at once, or, with a gate, when the test lets it. Told an `a`
/// of 13, it panics.
struct Takes(Option<Gate>);

impl Node for Takes {
    type Ports = Hears;
    type Params = Three;
    fn new(_: Three) -> triq::Result<Self> {
        Ok(Self(None))
    }
    fn set_params(&mut self, params: Three) -> triq::Result {
        assert_ne!(params.a, 13, "thirteen");
        if let Some(gate) = &self.0 {
            gate.there.send(()).expect("the test is waiting");
            gate.on.lock().unwrap().recv().expect("the test lets it on");
        }
        Ok(())
    }
    fn run(&mut self, _: &mut Hears) -> triq::Result {
        Ok(())
    }
}

#[test]
fn a_change_to_a_node_that_is_gone_is_not_made_to_the_node_that_has_its_name_now() -> triq::Result {
    // A node is being given settings, and takes its time. Meanwhile it is removed, and another
    // is made with its name and settings of its own. What the first was given is not the
    // second's: not shown as its settings, and not what its next change is worked out from.
    let rt = std::sync::Arc::new(Runtime::new(&unique("renamed"))?);
    let (there, arrived) = std::sync::mpsc::channel();
    let (let_on, on) = std::sync::mpsc::channel();
    // The first node made has the gate, and those after it none.
    let gate = std::sync::Mutex::new(Some(Gate { there, on: std::sync::Mutex::new(on) }));
    rt.register_with::<Takes>("takes", move |_, _| Ok(Takes(gate.lock().unwrap().take())))?;
    rt.spawn_kind("takes", "node", Placement::Dedicated, json!({ "a": 1, "b": 1, "c": 1 }))?;
    let has = |rt: &Runtime| rt.inventory().nodes.into_iter().find(|node| node.name == "node").unwrap().params;

    let changing = {
        let rt = rt.clone();
        std::thread::spawn(move || rt.change_params("node", json!({ "a": 5 })))
    };
    arrived.recv_timeout(std::time::Duration::from_secs(5)).expect("the first node is being given its settings");
    rt.remove("node")?;
    rt.spawn_kind("takes", "node", Placement::Dedicated, json!({ "a": 0, "b": 9, "c": 0 }))?;
    let_on.send(()).expect("the first node is waiting");
    let _ = changing.join().expect("the change ends");

    assert_eq!(has(&rt), json!({ "a": 0, "b": 9, "c": 0 }), "the second node's own settings");
    rt.change_params("node", json!({ "c": 7 }))?;
    assert_eq!(has(&rt), json!({ "a": 0, "b": 9, "c": 7 }));
    Ok(())
}

#[test]
fn a_node_that_panics_at_its_settings_can_be_given_others_after() -> triq::Result {
    let rt = Runtime::new(&unique("panics"))?;
    rt.register::<Takes>("takes")?;
    rt.spawn_kind("takes", "node", Placement::Dedicated, json!({ "a": 1 }))?;
    let panicked =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.change_params("node", json!({ "a": 13 }))));
    assert!(panicked.is_err(), "the panic reaches whoever asked");
    rt.change_params("node", json!({ "a": 2, "b": 4 }))?;
    assert_eq!(rt.inventory().nodes[0].params, json!({ "a": 2, "b": 4, "c": 0 }));
    Ok(())
}
