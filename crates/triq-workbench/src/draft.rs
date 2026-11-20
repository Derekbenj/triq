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
//! Which group a node is in is a change like any other: its runtime is
//! asked for it. And a group that a runtime offers as a kind is one change,
//! however many nodes it creates.
use crate::feed::Request;
use crate::graph::{Cut, group_key, groups, parent, within};
use std::collections::BTreeMap;
use triq::{Direction, FactoryInfo, InputInfo, Inventory, Member, NodeInfo, NodeState, OutputInfo, SourceInfo};

#[derive(Debug, Default)]
pub struct Draft {
    /// In the order they were written down. No two are about the same thing.
    changes: Vec<Request>,
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

    /// The connections that the draft cuts: they are still drawn, and are to go.
    pub fn cut(&self) -> Cut {
        let going = self.wires.iter().filter(|(_, mark)| **mark == Mark::Going);
        going.map(|(wire, _)| wire.clone()).collect()
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
    /// that was to be created goes all that was drafted for it, and with a
    /// group that was, all that was drafted for its nodes.
    pub fn take_back(&mut self, change: &Request, real: &[Inventory]) {
        self.changes.retain(|written| written != change);
        if let Request::Spawn { runtime, .. } = change {
            made(change, real).iter().for_each(|(_, node)| self.forget(runtime, &node.name));
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
    /// it will have, and so are the nodes of a group that is; wires that
    /// were drawn are there; settings are as drafted, and so are which group
    /// a node is in and whether it runs. What is to go is still there, for
    /// the drawing to show as going: see [`Draft::marks`].
    pub fn over(&self, real: &[Inventory]) -> Vec<Inventory> {
        let mut drafted = real.to_vec();
        for change in self.changes(real) {
            match &change {
                Request::Spawn { runtime, .. } => {
                    let nodes = made(&change, real);
                    let Some(home) = drafted.iter_mut().find(|inventory| inventory.runtime == *runtime) else {
                        continue;
                    };
                    // A kind that nobody offers cannot be drawn. The runtime will say so when it is asked.
                    let ghosts = nodes.into_iter().filter_map(|(group, node)| {
                        let factory = home.factories.iter().find(|factory| factory.kind == node.kind)?;
                        Some(ghost(runtime, factory, &group, node))
                    });
                    let ghosts: Vec<NodeInfo> = ghosts.collect();
                    home.nodes.extend(ghosts);
                    home.nodes.sort_by(|a, b| a.name.cmp(&b.name));
                    wires(&change, real).into_iter().for_each(|(from, to)| hear(&mut drafted, from, &to));
                }
                Request::Connect { from, to } => hear(&mut drafted, from.clone(), to),
                Request::SetGroup { runtime, node, group } => {
                    if let Some(node) = node_mut(&mut drafted, runtime, node) {
                        node.group.clone_from(group);
                    }
                }
                Request::SetParams { runtime, node, params } => {
                    if let Some(node) = node_mut(&mut drafted, runtime, node) {
                        node.params.clone_from(params);
                    }
                }
                Request::Pause { runtime, node } => {
                    if let Some(node) = node_mut(&mut drafted, runtime, node) {
                        node.state = NodeState::Paused;
                    }
                }
                Request::Resume { runtime, node } => {
                    if let Some(node) = node_mut(&mut drafted, runtime, node) {
                        node.state = NodeState::Running;
                    }
                }
                Request::Disconnect { .. } | Request::Remove { .. } => {}
            }
        }
        drafted
    }

    /// What the draft does to each card and wire, for the drawing to tell them apart by.
    pub fn marks(&self, real: &[Inventory]) -> Marks {
        let mut marks = Marks::default();
        // Which group each node is in, and which it would be in.
        let (was, will) = (groups(real), groups(&self.over(real)));
        for node in was.keys().chain(will.keys()) {
            let (leaves, joins) = (was.get(node).map(String::as_str), will.get(node).map(String::as_str));
            // Every group that gains or loses the node is marked: the one it leaves, the one it
            // joins, and the groups those are inside of. A group that no node is in yet is new.
            let holds = |of: Option<&str>, group: &str| of.is_some_and(|of| within(of, group));
            let around = |of| std::iter::successors(of, |group: &&str| parent(group));
            for group in around(leaves).chain(around(joins)).filter(|group| holds(leaves, group) != holds(joins, group))
            {
                let new = !was.values().any(|real| within(real, group));
                marks.cards.insert(group_key(group), if new { Mark::New } else { Mark::Changed });
            }
        }
        for change in self.changes(real) {
            match &change {
                Request::Connect { from, to } => _ = marks.wires.insert((from.clone(), to.clone()), Mark::New),
                Request::Disconnect { from, to } => _ = marks.wires.insert((from.clone(), to.clone()), Mark::Going),
                Request::Spawn { runtime, .. } => {
                    for (_, node) in made(&change, real) {
                        marks.cards.insert(format!("{runtime}/{}", node.name), Mark::New);
                    }
                    for wire in wires(&change, real) {
                        marks.wires.insert(wire, Mark::New);
                    }
                }
                Request::Remove { runtime, node } => {
                    marks.cards.insert(format!("{runtime}/{node}"), Mark::Going);
                    // A connection ends with the node it is to: its wires go with it.
                    for wire in wired(real, runtime, node) {
                        marks.wires.entry(wire).or_insert(Mark::Going);
                    }
                }
                Request::SetParams { runtime, node, .. }
                | Request::SetGroup { runtime, node, .. }
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
    /// Which group a node is in.
    Group(&'a str, &'a str),
    /// Whether a node is there at all.
    Being(&'a str, &'a str),
}

fn about(request: &Request) -> About<'_> {
    match request {
        Request::Connect { from, to } | Request::Disconnect { from, to } => About::Wire(from, to),
        Request::SetParams { runtime, node, .. } => About::Settings(runtime, node),
        Request::Pause { runtime, node } | Request::Resume { runtime, node } => About::Running(runtime, node),
        Request::SetGroup { runtime, node, .. } => About::Group(runtime, node),
        Request::Spawn { runtime, name: node, .. } | Request::Remove { runtime, node } => About::Being(runtime, node),
    }
}

/// The runtime and the node that a change is about, unless it is about a wire.
fn on(request: &Request) -> Option<(&str, &str)> {
    match about(request) {
        About::Wire(..) => None,
        About::Settings(runtime, node)
        | About::Running(runtime, node)
        | About::Group(runtime, node)
        | About::Being(runtime, node) => Some((runtime, node)),
    }
}

/// Whether a change would make a machine that reports `real` any different.
fn needed(request: &Request, real: &[Inventory]) -> bool {
    let found = on(request).and_then(|(runtime, name)| node(real, runtime, name));
    match request {
        Request::Connect { from, to } => !hears(real, to, from),
        Request::Disconnect { from, to } => hears(real, to, from),
        // A node that is yet to be created has no settings to compare with, and no group.
        Request::SetParams { params, .. } => found.is_none_or(|node| node.params != *params),
        Request::SetGroup { group, .. } => found.is_none_or(|node| node.group != *group),
        Request::Pause { .. } => found.is_some_and(|node| node.state == NodeState::Running),
        Request::Resume { .. } => found.is_some_and(|node| node.state == NodeState::Paused),
        Request::Remove { .. } => found.is_some(),
        // Of a group, none of its nodes may be there.
        Request::Spawn { runtime, .. } => {
            made(request, real).iter().all(|(_, made)| node(real, runtime, &made.name).is_none())
        }
    }
}

/// The nodes that a request to create something creates, each with the
/// group it is in from the start: one node, in none; or, if the kind is one
/// that its runtime offers as a group, the nodes of that group, named after
/// it. No other request creates any.
fn made(request: &Request, real: &[Inventory]) -> Vec<(String, Member)> {
    let Request::Spawn { runtime, kind, name, placement, params } = request else { return Vec::new() };
    match offered(real, runtime, kind).and_then(|factory| factory.group.as_ref()) {
        Some(group) => {
            let named = |node: &Member| Member { name: node.named(name), ..node.clone() };
            group.nodes.iter().map(|node| (name.clone(), named(node))).collect()
        }
        None => {
            let (kind, name) = (kind.clone(), name.clone());
            vec![(String::new(), Member { kind, name, placement: placement.clone(), params: params.clone() })]
        }
    }
}

/// The wires that come with a group that a request creates, each from an
/// output to an input, both `runtime/node/port`.
fn wires(request: &Request, real: &[Inventory]) -> Vec<(String, String)> {
    let Request::Spawn { runtime, kind, name, .. } = request else { return Vec::new() };
    let Some(group) = offered(real, runtime, kind).and_then(|factory| factory.group.as_ref()) else {
        return Vec::new();
    };
    // A port of the group is `node/port` in its runtime. An output from elsewhere says where it is.
    let full = |port: String| if port.split('/').count() == 3 { port } else { format!("{runtime}/{port}") };
    group.wired(name).into_iter().map(|(from, to)| (full(from), full(to))).collect()
}

/// A kind that a runtime offers.
fn offered<'a>(machine: &'a [Inventory], runtime: &str, kind: &str) -> Option<&'a FactoryInfo> {
    let home = machine.iter().find(|inventory| inventory.runtime == runtime)?;
    home.factories.iter().find(|factory| factory.kind == kind)
}

/// A node as it will be when its factory has made it: its ports, and nothing through them yet.
fn ghost(runtime: &str, factory: &FactoryInfo, group: &str, node: Member) -> NodeInfo {
    let ports = |direction| factory.ports.iter().filter(move |port| port.direction == direction).cloned();
    let output = |port: triq::PortSpec| OutputInfo {
        service: format!("{runtime}/{}/{}", node.name, port.name),
        port,
        subscribers: 0,
        sent: 0,
        sent_ns: 0,
        undelivered: 0,
    };
    NodeInfo {
        kind: factory.kind.clone(),
        group: group.to_owned(),
        // The application that offers such a factory runs its nodes itself.
        placement: (!factory.local).then_some(node.placement),
        state: NodeState::Running,
        params: node.params,
        runs: 0,
        errors: 0,
        last_error: None,
        inputs: ports(Direction::Input).map(|port| InputInfo { port, sources: Vec::new() }).collect(),
        outputs: ports(Direction::Output).map(output).collect(),
        name: node.name,
    }
}

/// Every connection of the machine to or from a node, as its output and its input.
fn wired(machine: &[Inventory], runtime: &str, node: &str) -> Vec<(String, String)> {
    let its = |port: &str| parts(port).is_some_and(|[home, name, _]| (home, name) == (runtime, node));
    let mut wired = Vec::new();
    for inventory in machine {
        for hearer in &inventory.nodes {
            for input in &hearer.inputs {
                let to = format!("{}/{}/{}", inventory.runtime, hearer.name, input.port.name);
                let heard = input.sources.iter().map(|source| (source.output.clone(), to.clone()));
                wired.extend(heard.filter(|(from, to)| its(from) || its(to)));
            }
        }
    }
    wired
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

/// Have the input called `to` hear the output called `from`, as a wire that was drawn does.
fn hear(machine: &mut [Inventory], from: String, to: &str) {
    let source =
        SourceInfo { output: from, connected: true, received: 0, lost: 0, sent_ns: 0, latency_us: 0, gap_us: 0 };
    if let Some(input) = input_mut(machine, to) {
        input.sources.push(source);
    }
}

/// Whether the input called `input` is connected to the output called `output`.
fn hears(machine: &[Inventory], input: &str, output: &str) -> bool {
    let Some([runtime, name, port]) = parts(input) else { return false };
    let inputs = node(machine, runtime, name).into_iter().flat_map(|node| &node.inputs);
    inputs
        .filter(|input| input.port.name == port)
        .any(|input| input.sources.iter().any(|source| source.output == output))
}
