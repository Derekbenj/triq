//! The controller host's loop, with pretend controllers in place of SDL.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use triq::prelude::*;
use triq::{NodeInfo, NodeState, Remote};
use triq_xbox::{FACTORY, Host, Pads, XboxState, buttons};

/// Controllers that tests plug in, move and unplug.
#[derive(Clone, Default)]
struct FakePads(Arc<Mutex<Fake>>);

#[derive(Default)]
struct Fake {
    plugged: BTreeMap<u32, (String, XboxState)>,
    open: Vec<u32>,
}

impl FakePads {
    fn plug(&self, id: u32, name: &str) {
        let state = XboxState { connected: true, ..Default::default() };
        self.0.lock().unwrap().plugged.insert(id, (name.into(), state));
    }
    fn unplug(&self, id: u32) {
        self.0.lock().unwrap().plugged.remove(&id);
    }
    fn hold(&self, id: u32, state: XboxState) {
        self.0.lock().unwrap().plugged.get_mut(&id).unwrap().1 = state;
    }
    fn open(&self) -> Vec<u32> {
        self.0.lock().unwrap().open.clone()
    }
}

impl Pads for FakePads {
    fn devices(&mut self) -> Vec<(u32, String)> {
        self.0.lock().unwrap().plugged.iter().map(|(id, (name, _))| (*id, name.clone())).collect()
    }
    fn open(&mut self, id: u32) -> triq::Result {
        let mut fake = self.0.lock().unwrap();
        if !fake.plugged.contains_key(&id) {
            return Err(Error::Unknown(format!("controller {id}: it is not plugged in")));
        }
        fake.open.push(id);
        Ok(())
    }
    fn close(&mut self, id: u32) {
        self.0.lock().unwrap().open.retain(|open| *open != id);
    }
    fn read(&mut self, id: u32) -> Option<XboxState> {
        self.0.lock().unwrap().plugged.get(&id).map(|(_, state)| *state)
    }
}

#[derive(Ports)]
struct Probe {
    state: Input<XboxState>,
}

fn unique(tag: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("x{}-{}-{tag}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

struct Bench {
    pads: FakePads,
    host: Host<FakePads>,
    remote: Remote,
    probe: Probe,
    runtime: String,
}

impl Bench {
    fn new(tag: &str) -> triq::Result<Self> {
        let pads = FakePads::default();
        let runtime = unique(tag);
        let rt = Runtime::new(&runtime)?;
        let probe: Probe = rt.attach("probe")?;
        let host = Host::new(rt, pads.clone())?;
        Ok(Self { pads, host, remote: Remote::open(&runtime)?, probe, runtime })
    }

    /// What the factory tells the world about the controllers it sees.
    fn devices(&self) -> triq::Result<Value> {
        Ok(self.remote.inventory()?.factories[0].info["devices"].clone())
    }

    fn node(&self, name: &str) -> triq::Result<NodeInfo> {
        Ok(self.remote.inventory()?.nodes.into_iter().find(|node| node.name == name).expect("the node"))
    }

    /// Create a controller node and listen to it.
    fn controller(&mut self, name: &str, params: Value) -> triq::Result {
        self.remote.spawn(FACTORY, name, Placement::Dedicated, params)?;
        self.remote.connect(&format!("{name}/state"), "probe/state")?;
        self.host.step()
    }

    /// Everything the probe has received since the last call.
    fn heard(&self) -> triq::Result<Vec<XboxState>> {
        let mut heard = Vec::new();
        while let Some(state) = self.probe.state.recv()? {
            heard.push(*state);
        }
        Ok(heard)
    }
}

#[test]
fn the_factory_advertises_the_controllers_it_sees() -> triq::Result {
    let mut bench = Bench::new("detect")?;
    bench.host.step()?;
    let factory = &bench.remote.inventory()?.factories[0];
    assert_eq!((factory.kind.as_str(), factory.local), (FACTORY, true));
    assert_eq!(factory.params, json!({ "device": null }));
    assert_eq!(bench.devices()?, json!([]));

    bench.pads.plug(7, "Xbox Wireless Controller");
    bench.pads.plug(9, "Xbox Elite");
    bench.host.step()?;
    assert_eq!(
        bench.devices()?,
        json!([
            { "id": 7, "name": "Xbox Wireless Controller", "claimed_by": null },
            { "id": 9, "name": "Xbox Elite", "claimed_by": null },
        ])
    );
    bench.pads.unplug(9);
    bench.host.step()?;
    assert_eq!(bench.devices()?.as_array().unwrap().len(), 1);
    Ok(())
}

#[test]
fn a_node_publishes_the_controller_it_was_given() -> triq::Result {
    let mut bench = Bench::new("publish")?;
    bench.pads.plug(7, "Xbox Wireless Controller");
    bench.controller("pad1", json!({ "device": 7 }))?;
    assert_eq!(bench.devices()?[0]["claimed_by"], "pad1");
    bench.heard()?;

    let held =
        XboxState { connected: true, buttons: buttons::A | buttons::LEFT_BUMPER, left_x: 1200, ..Default::default() };
    bench.pads.hold(7, held);
    bench.host.step()?;
    assert_eq!(bench.heard()?, [held]);
    assert!(held.pressed(buttons::A) && !held.pressed(buttons::B));
    assert_eq!(held.pressed_names(), ["A", "LB"]);

    // A node created without a controller says nothing until it is given one.
    bench.pads.plug(9, "Xbox Elite");
    bench.controller("pad2", json!(null))?;
    bench.heard()?;
    bench.host.step()?;
    assert_eq!(bench.heard()?.len(), 1, "only pad1 speaks");
    bench.remote.set_params("pad2", json!({ "device": 9 }))?;
    bench.host.step()?;
    assert_eq!(bench.heard()?.len(), 2);
    assert_eq!(bench.devices()?[1]["claimed_by"], "pad2");
    Ok(())
}

#[test]
fn a_controller_belongs_to_one_node_and_bad_choices_are_reported() -> triq::Result {
    let mut bench = Bench::new("claims")?;
    bench.pads.plug(7, "Xbox Wireless Controller");
    bench.controller("pad1", json!({ "device": 7 }))?;

    bench.controller("pad2", json!({ "device": 7 }))?;
    assert_eq!(bench.node("pad2")?.last_error.as_deref(), Some("controller 7 is already assigned to pad1"));
    assert_eq!(bench.devices()?[0]["claimed_by"], "pad1");

    bench.remote.set_params("pad2", json!({ "device": 42 }))?;
    bench.host.step()?;
    assert!(bench.node("pad2")?.last_error.unwrap().contains("42"));
    assert_eq!(bench.pads.open(), [7], "nothing was opened for pad2");
    Ok(())
}

#[test]
fn losing_a_controller_sends_one_neutral_state_and_frees_the_node() -> triq::Result {
    let mut bench = Bench::new("unplug")?;
    bench.pads.plug(7, "Xbox Wireless Controller");
    bench.controller("pad1", json!({ "device": 7 }))?;
    bench.pads.hold(7, XboxState { connected: true, left_y: -32768, ..Default::default() });
    bench.host.step()?;
    bench.heard()?;

    // Unplugged: whoever listens must not keep steering on the last value.
    bench.pads.unplug(7);
    bench.host.step()?;
    assert_eq!(bench.heard()?, [XboxState::default()]);
    bench.host.step()?;
    assert!(bench.heard()?.is_empty(), "and then silence");
    assert!(bench.node("pad1")?.last_error.unwrap().contains("unplugged"));
    assert!(bench.pads.open().is_empty());

    // It comes back under a new id; the node is pointed at it.
    bench.pads.plug(8, "Xbox Wireless Controller");
    bench.remote.set_params("pad1", json!({ "device": 8 }))?;
    bench.host.step()?;
    assert_eq!(bench.heard()?.len(), 1);

    // Letting go of it on purpose does the same.
    bench.remote.set_params("pad1", json!({ "device": null }))?;
    bench.host.step()?;
    assert_eq!(bench.heard()?, [XboxState::default()]);
    assert_eq!(bench.devices()?[0]["claimed_by"], Value::Null);
    Ok(())
}

#[test]
fn pausing_silences_a_node_and_removing_it_releases_its_controller() -> triq::Result {
    let mut bench = Bench::new("pause")?;
    bench.pads.plug(7, "Xbox Wireless Controller");
    bench.controller("pad1", json!({ "device": 7 }))?;
    bench.heard()?;

    bench.remote.pause("pad1")?;
    assert_eq!(bench.node("pad1")?.state, NodeState::Paused);
    bench.host.step()?;
    bench.host.step()?;
    assert!(bench.heard()?.is_empty());
    bench.remote.resume("pad1")?;
    bench.host.step()?;
    assert_eq!(bench.heard()?.len(), 1);

    bench.remote.remove("pad1")?;
    bench.host.step()?;
    assert!(bench.pads.open().is_empty());
    assert_eq!(bench.devices()?[0]["claimed_by"], Value::Null);
    assert!(bench.remote.inventory()?.nodes.iter().all(|node| node.name != "pad1"));
    assert!(bench.runtime.starts_with('x'));
    Ok(())
}
