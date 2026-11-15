//! The body of a program that offers controllers as nodes.
//!
//! Which controller a node reads is not a connection in the graph. It is a
//! setting of the node, `{"device": id}`, chosen when the node is created and
//! changeable while it runs. The factory advertises the controllers it sees,
//! and which node has each, as its information.
use crate::XboxState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use triq::prelude::*;
use triq::{Local, LocalFactory};

/// The kind of node this host offers.
pub const FACTORY: &str = "xbox";

#[derive(Ports)]
pub struct ControllerPorts {
    pub state: Output<XboxState>,
}

/// A controller node's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllerParams {
    /// The controller to read, by the id the factory advertises; none to stay silent.
    pub device: Option<u32>,
}

/// A controller the host sees, as advertised by the factory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Changes when a controller is unplugged and plugged in again.
    pub id: u32,
    pub name: String,
    /// The node that reads it, if any.
    pub claimed_by: Option<String>,
}

/// Where controller states come from: SDL in the real program, a pretend set in tests.
pub trait Pads {
    /// The controllers plugged in right now, by id and name.
    fn devices(&mut self) -> Vec<(u32, String)>;
    /// Start reading a controller.
    fn open(&mut self, id: u32) -> triq::Result;
    fn close(&mut self, id: u32);
    /// What an open controller is doing; `None` once it is gone.
    fn read(&mut self, id: u32) -> Option<XboxState>;
}

pub struct Host<P: Pads> {
    runtime: Runtime,
    requests: LocalFactory<ControllerPorts, ControllerParams>,
    pads: P,
    controllers: BTreeMap<String, Controller>,
    /// What the factory last advertised.
    advertised: Option<Vec<Device>>,
}

struct Controller {
    ports: ControllerPorts,
    device: Option<u32>,
    paused: bool,
    /// Its controller went away: send one all-zero state so nobody keeps acting on the last one.
    neutral_due: bool,
}

impl<P: Pads> Host<P> {
    pub fn new(runtime: Runtime, pads: P) -> triq::Result<Self> {
        let requests = runtime.local_factory(FACTORY)?;
        Ok(Self { runtime, requests, pads, controllers: BTreeMap::new(), advertised: None })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn pads_mut(&mut self) -> &mut P {
        &mut self.pads
    }

    /// One turn of the program's loop: do what was asked, notice controllers
    /// that went away, publish, and advertise what is plugged in.
    pub fn step(&mut self) -> triq::Result {
        while let Some(request) = self.requests.try_next() {
            match request {
                Local::Spawn { name, params, ports } => {
                    let controller = Controller { ports, device: None, paused: false, neutral_due: false };
                    self.controllers.insert(name.clone(), controller);
                    self.assign(&name, params.device);
                }
                Local::Params { name, params } => self.assign(&name, params.device),
                Local::Pause { name } => self.set_paused(&name, true),
                Local::Resume { name } => self.set_paused(&name, false),
                // Dropping the node's ports is what ends it.
                Local::Remove { name } => {
                    if let Some(id) = self.controllers.remove(&name).and_then(|controller| controller.device) {
                        self.pads.close(id);
                    }
                }
            }
        }
        self.publish()?;
        self.advertise()
    }

    fn set_paused(&mut self, node: &str, paused: bool) {
        if let Some(controller) = self.controllers.get_mut(node) {
            controller.paused = paused;
        }
    }

    /// Point a node at a controller, or at none. A choice that cannot be
    /// honoured is reported on the node and changes nothing.
    fn assign(&mut self, node: &str, device: Option<u32>) {
        let Some(current) = self.controllers.get(node).map(|controller| controller.device) else { return };
        if current == device {
            return;
        }
        if let Some(id) = device {
            let owner = self.controllers.iter().find(|(_, other)| other.device == Some(id)).map(|(name, _)| name);
            let refused = match owner {
                Some(owner) => Some(format!("controller {id} is already assigned to {owner}")),
                None => self.pads.open(id).err().map(|error| error.to_string()),
            };
            if let Some(problem) = refused {
                let _ = self.runtime.report(node, &problem);
                return;
            }
        }
        if let Some(old) = current {
            self.pads.close(old);
        }
        let controller = self.controllers.get_mut(node).expect("looked up above");
        controller.device = device;
        controller.neutral_due = device.is_none() && current.is_some();
    }

    fn publish(&mut self) -> triq::Result {
        for (name, controller) in &mut self.controllers {
            let state = controller.device.map(|id| (id, self.pads.read(id)));
            if let Some((id, None)) = state {
                self.pads.close(id);
                controller.device = None;
                controller.neutral_due = true;
                let _ = self.runtime.report(name, &format!("controller {id} was unplugged"));
            }
            if controller.paused {
                continue;
            }
            match state {
                Some((_, Some(state))) => _ = controller.ports.state.send(state)?,
                _ if controller.neutral_due => _ = controller.ports.state.send(XboxState::default())?,
                _ => {}
            }
            controller.neutral_due = false;
        }
        Ok(())
    }

    /// Tell the world which controllers are plugged in and who has them, when that changes.
    fn advertise(&mut self) -> triq::Result {
        let claimed =
            |id: u32| self.controllers.iter().find(|(_, c)| c.device == Some(id)).map(|(name, _)| name.clone());
        let devices: Vec<Device> =
            self.pads.devices().into_iter().map(|(id, name)| Device { claimed_by: claimed(id), id, name }).collect();
        if self.advertised.as_ref() != Some(&devices) {
            self.runtime.set_factory_info(FACTORY, json!({ "devices": devices }))?;
            self.advertised = Some(devices);
        }
        Ok(())
    }
}
