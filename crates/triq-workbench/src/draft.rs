//! Changes that were made on the drawing and are held back.
//!
//! While the user is drafting, what they do is written down here instead of
//! being asked of a runtime. The drawing shows the machine as it would be
//! with the changes made, and applying the draft asks for them, in the order
//! they were written down.
//!
//! A draft holds what it takes to get from the machine to the drawing, and no
//! more: one change for each thing that differs. A wire that is drawn and
//! taken away again leaves nothing behind. Nothing here talks to a runtime or
//! to the screen.
//!
//! Which group a node is in is drafted too. That is not asked of any
//! runtime: groups are how the workbench draws the machine. But while
//! drafting they are held back with the rest, and made when the draft is applied.
use crate::feed::Request;
use crate::graph::{Groups, group_key};
use serde_json::Value;
use std::collections::BTreeMap;
use triq::{Direction, FactoryInfo, InputInfo, Inventory, NodeInfo, NodeState, OutputInfo, Placement, SourceInfo};

#[derive(Debug, Default)]
pub struct Draft {
    /// In the order they were written down. No two are about the same thing.
    changes: Vec<Request>,
    /// The group that a node is to be in, by `runtime/node`: `None` for none.
    groups: BTreeMap<String, Option<String>>,
}

/// What a draft does to something on the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// It is not on the machine yet.
    New,
    /// It is on the machine, and is to be different.
    Changed,
    /// It is on the machine, and is to go.
    Going,
}

/// What a draft does to the cards and the wires of the drawing.
#[derive(Debug, Default)]
pub struct Marks {
    /// By `runtime/node`.
    cards: BTreeMap<String, Mark>,
    /// By the output and the input, each `runtime/node/port`.
    wires: BTreeMap<(String, String), Mark>,
}

impl Marks {
    pub fn card(&self, key: &str) -> Option<Mark> {
        self.cards.get(key).copied()
    }

    pub fn wire(&self, from: &str, to: &str) -> Option<Mark> {
        let wire = self.wires.iter().find(|((output, input), _)| output == from && input == to);
        wire.map(|(_, mark)| *mark)
    }

    /// Whether a wire that the draft draws or cuts ends at this port.
    pub fn port(&self, name: &str) -> bool {
        self.wires.keys().any(|(from, to)| from == name || to == name)
    }
}

impl Draft {
    /// Write a change down, as the last. `real` is what the runtimes report.
    ///
    /// A change that was written before about the same thing gives way to it,
    /// and a change that would leave the machine as it is is not written at
    /// all. So a wire that is drawn and then taken away leaves nothing to do,
    /// and neither does a wire of the machine that is cut and drawn again.
    pub fn record(&mut self, request: Request, real: &[Inventory]) {
        // A node that was only drafted is not removed by any runtime: the draft of it is taken back.
        if let Request::Remove { runtime, node } = &request
            && self.creates(runtime, node)
        {
            return self.forget(runtime, node);
        }
        self.changes.retain(|written| about(written) != about(&request));
        if needed(&request, real) {
            self.changes.push(request);
        }
    }

    /// Take a change back, as if it had not been written down. With a node
    /// that was to be created goes all that was drafted for it.
    pub fn take_back(&mut self, change: &Request) {
        match change {
            Request::Spawn { runtime, name, .. } => self.forget(runtime, name),
            _ => self.changes.retain(|written| written != change),
        }
    }

    /// What is left to do on a machine that reports `real`, in the order it
    /// was written down. A change that the machine has come to by other
    /// means is not among them.
    pub fn changes(&self, real: &[Inventory]) -> Vec<Request> {
        self.changes.iter().filter(|change| needed(change, real)).cloned().collect()
    }

    /// The machine as it would be with the changes made, for the drawing. A
    /// node that is to be created is there, with the ports its factory says
    /// it will have; wires that were drawn are there; settings are as
    /// drafted, and so is whether a node runs. What is to go is still there,
    /// for the drawing to show as going: see [`Draft::marks`].
    pub fn over(&self, real: &[Inventory]) -> Vec<Inventory> {
        let mut drafted = real.to_vec();
        for change in self.changes(real) {
            match change {
                Request::Spawn { runtime, kind, name, placement, params } => {
                    let Some(home) = drafted.iter_mut().find(|inventory| inventory.runtime == runtime) else {
                        continue;
                    };
                    // A kind that nobody offers cannot be drawn. The runtime will say so when it is asked.
                    let Some(factory) = home.factories.iter().find(|factory| factory.kind == kind) else { continue };
                    home.nodes.push(ghost(&runtime, factory, name, placement, params));
                    home.nodes.sort_by(|a, b| a.name.cmp(&b.name));
                }
                Request::Connect { from, to } => {
                    let source = SourceInfo {
                        output: from,
                        connected: true,
                        problem: None,
                        received: 0,
                        lost: 0,
                        sent_ns: 0,
                        latency_us: 0,
                        gap_us: 0,
                    };
                    if let Some(input) = input_mut(&mut drafted, &to) {
                        input.sources.push(source);
                    }
                }
                Request::SetParams { runtime, node, params } => {
                    if let Some(node) = node_mut(&mut drafted, &runtime, &node) {
                        node.params = params;
                    }
                }
                Request::Pause { runtime, node } => {
                    if let Some(node) = node_mut(&mut drafted, &runtime, &node) {
                        node.state = NodeState::Paused;
                    }
                }
                Request::Resume { runtime, node } => {
                    if let Some(node) = node_mut(&mut drafted, &runtime, &node) {
                        node.state = NodeState::Running;
                    }
                }
                Request::Disconnect { .. } | Request::Remove { .. } => {}
            }
        }
        drafted
    }

    /// Put a node, by `runtime/node`, in a group, or with `None` take it out
    /// of the group it is in. `real` is which group each node is in: a node
    /// that is put back where it is leaves nothing to do.
    pub fn group(&mut self, node: String, group: Option<String>, real: &Groups) {
        if real.get(&node) == group.as_ref() {
            self.groups.remove(&node);
        } else {
            self.groups.insert(node, group);
        }
    }

    /// The nodes that are to change group, each with the group it is to be in.
    pub fn regrouped(&self, real: &Groups) -> Vec<(String, Option<String>)> {
        let differs = self.groups.iter().filter(|(node, group)| real.get(*node) != group.as_ref());
        differs.map(|(node, group)| (node.clone(), group.clone())).collect()
    }

    /// Which group each node would be in.
    pub fn grouping(&self, real: &Groups) -> Groups {
        let mut drafted = real.clone();
        for (node, group) in self.regrouped(real) {
            match group {
                Some(group) => _ = drafted.insert(node, group),
                None => _ = drafted.remove(&node),
            }
        }
        drafted
    }

    /// Leave a node in the group it is in, after all.
    pub fn leave(&mut self, node: &str) {
        self.groups.remove(node);
    }

    /// Call a group by another name, wherever a node is to be put in it.
    pub fn rename(&mut self, group: &str, name: &str) {
        for drafted in self.groups.values_mut().flatten().filter(|drafted| *drafted == group) {
            *drafted = name.to_owned();
        }
    }

    /// What the draft does to each card and wire, for the drawing to tell
    /// them apart by. `groups` is which group each node is in.
    pub fn marks(&self, real: &[Inventory], groups: &Groups) -> Marks {
        let mut marks = Marks::default();
        for (node, group) in self.regrouped(groups) {
            // A group that no node is in yet is new. One that gains or loses a node is changed.
            let new = |group: &String| !groups.values().any(|real| real == group);
            if let Some(group) = &group {
                marks.cards.insert(group_key(group), if new(group) { Mark::New } else { Mark::Changed });
            }
            if let Some(left) = groups.get(&node) {
                marks.cards.entry(group_key(left)).or_insert(Mark::Changed);
            }
            marks.cards.insert(node, Mark::Changed);
        }
        for change in self.changes(real) {
            match change {
                Request::Connect { from, to } => _ = marks.wires.insert((from, to), Mark::New),
                Request::Disconnect { from, to } => _ = marks.wires.insert((from, to), Mark::Going),
                Request::Spawn { runtime, name, .. } => _ = marks.cards.insert(format!("{runtime}/{name}"), Mark::New),
                Request::Remove { runtime, node } => _ = marks.cards.insert(format!("{runtime}/{node}"), Mark::Going),
                Request::SetParams { runtime, node, .. }
                | Request::Pause { runtime, node }
                | Request::Resume { runtime, node } => {
                    // A card that is new, or is to go, says that before anything else.
                    marks.cards.entry(format!("{runtime}/{node}")).or_insert(Mark::Changed);
                }
            }
        }
        marks
    }

    /// Whether the draft is to create that node.
    fn creates(&self, runtime: &str, node: &str) -> bool {
        self.changes.iter().any(|change| matches!(change, Request::Spawn { .. }) && on(change) == Some((runtime, node)))
    }

    /// Drop every change that is about a node, or about a wire to or from it.
    fn forget(&mut self, runtime: &str, node: &str) {
        let its_port = |port: &String| parts(port).is_some_and(|[home, name, _]| (home, name) == (runtime, node));
        self.changes.retain(|change| match change {
            Request::Connect { from, to } | Request::Disconnect { from, to } => !its_port(from) && !its_port(to),
            _ => on(change) != Some((runtime, node)),
        });
    }
}

/// What a change is about. A draft holds one change about each thing: the one written last.
#[derive(PartialEq)]
enum About<'a> {
    /// A wire, from an output to an input: whether it is there.
    Wire(&'a str, &'a str),
    /// The settings of a node, by runtime and name.
    Settings(&'a str, &'a str),
    /// Whether a node runs or is paused.
    Running(&'a str, &'a str),
    /// Whether a node is there at all.
    Being(&'a str, &'a str),
}

fn about(request: &Request) -> About<'_> {
    match request {
        Request::Connect { from, to } | Request::Disconnect { from, to } => About::Wire(from, to),
        Request::SetParams { runtime, node, .. } => About::Settings(runtime, node),
        Request::Pause { runtime, node } | Request::Resume { runtime, node } => About::Running(runtime, node),
        Request::Spawn { runtime, name: node, .. } | Request::Remove { runtime, node } => About::Being(runtime, node),
    }
}

/// The runtime and the node that a change is about, unless it is about a wire.
fn on(request: &Request) -> Option<(&str, &str)> {
    match about(request) {
        About::Wire(..) => None,
        About::Settings(runtime, node) | About::Running(runtime, node) | About::Being(runtime, node) => {
            Some((runtime, node))
        }
    }
}

/// Whether a change would make a machine that reports `real` any different.
fn needed(request: &Request, real: &[Inventory]) -> bool {
    let found = on(request).and_then(|(runtime, name)| node(real, runtime, name));
    match request {
        Request::Connect { from, to } => !hears(real, to, from),
        Request::Disconnect { from, to } => hears(real, to, from),
        // A node that is yet to be created has no settings to compare with.
        Request::SetParams { params, .. } => found.is_none_or(|node| node.params != *params),
        Request::Pause { .. } => found.is_some_and(|node| node.state == NodeState::Running),
        Request::Resume { .. } => found.is_some_and(|node| node.state == NodeState::Paused),
        Request::Remove { .. } => found.is_some(),
        Request::Spawn { .. } => found.is_none(),
    }
}

/// A node as it will be when its factory has made it: its ports, and nothing through them yet.
fn ghost(runtime: &str, factory: &FactoryInfo, name: String, placement: Placement, params: Value) -> NodeInfo {
    let ports = |direction| factory.ports.iter().filter(move |port| port.direction == direction).cloned();
    let output = |port: triq::PortSpec| OutputInfo {
        service: format!("{runtime}/{name}/{}", port.name),
        port,
        subscribers: 0,
        sent: 0,
        sent_ns: 0,
        undelivered: 0,
    };
    NodeInfo {
        kind: factory.kind.clone(),
        // The application that offers such a factory runs its nodes itself.
        placement: (!factory.local).then_some(placement),
        state: NodeState::Running,
        params,
        runs: 0,
        errors: 0,
        last_error: None,
        inputs: ports(Direction::Input).map(|port| InputInfo { port, sources: Vec::new() }).collect(),
        outputs: ports(Direction::Output).map(output).collect(),
        name,
    }
}

/// `runtime/node/port` as its three parts.
fn parts(port: &str) -> Option<[&str; 3]> {
    let mut parts = port.split('/');
    Some([parts.next()?, parts.next()?, parts.next()?])
}

fn node<'a>(machine: &'a [Inventory], runtime: &str, name: &str) -> Option<&'a NodeInfo> {
    let home = machine.iter().find(|inventory| inventory.runtime == runtime)?;
    home.nodes.iter().find(|node| node.name == name)
}

fn node_mut<'a>(machine: &'a mut [Inventory], runtime: &str, name: &str) -> Option<&'a mut NodeInfo> {
    let home = machine.iter_mut().find(|inventory| inventory.runtime == runtime)?;
    home.nodes.iter_mut().find(|node| node.name == name)
}

/// The input called `runtime/node/port`.
fn input_mut<'a>(machine: &'a mut [Inventory], name: &str) -> Option<&'a mut InputInfo> {
    let [runtime, node, port] = parts(name)?;
    node_mut(machine, runtime, node)?.inputs.iter_mut().find(|input| input.port.name == port)
}

/// Whether the input called `input` is connected to the output called `output`.
fn hears(machine: &[Inventory], input: &str, output: &str) -> bool {
    let Some([runtime, name, port]) = parts(input) else { return false };
    let inputs = node(machine, runtime, name).into_iter().flat_map(|node| &node.inputs);
    inputs
        .filter(|input| input.port.name == port)
        .any(|input| input.sources.iter().any(|source| source.output == output))
}
