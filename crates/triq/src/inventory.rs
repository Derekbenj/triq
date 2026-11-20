//! What a runtime reports about itself. Plain data, sent between programs as JSON.
use crate::{Placement, PortSpec, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Settings as the inventory shows them. Going through text keeps an `f32` of
/// 0.2 reading 0.2; converted directly it would read 0.20000000298023224.
pub(crate) fn shown<A: Serialize>(params: &A) -> Result<Value> {
    Ok(serde_json::from_str(&serde_json::to_string(params)?)?)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub runtime: String,
    pub pid: u32,
    /// Moves on whenever the runtime's nodes, factories, connections or settings
    /// change. Counters moving is not a change.
    pub revision: u64,
    /// When the runtime read its counters for this report, in nanoseconds
    /// since 1970 by its own clock. Two reports and the time between them
    /// give rates. Zero from a runtime too old to say.
    #[serde(default)]
    pub taken_ns: u64,
    pub pools: Vec<PoolInfo>,
    /// The kinds of node this runtime can be asked to create. Sorted by kind.
    pub factories: Vec<FactoryInfo>,
    /// Sorted by name.
    pub nodes: Vec<NodeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolInfo {
    pub name: String,
    pub threads: usize,
    /// True when any free thread of the pool may run a node; false when each node keeps one thread.
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactoryInfo {
    pub kind: String,
    /// The ports every node of this kind will have.
    pub ports: Vec<PortSpec>,
    /// The default settings, as a starting point for a form.
    pub params: Value,
    /// True when the application creates and runs these nodes itself;
    /// a placement in a spawn request is then ignored.
    pub local: bool,
    /// Whatever else the application wants to show, such as the devices it sees.
    pub info: Value,
    /// For a kind that creates a group: its nodes and the wires between them.
    /// `None` for a kind of node, and from a runtime too old to offer groups.
    #[serde(default)]
    pub group: Option<Group>,
}

/// Several nodes that are created together and wired to each other: what a
/// runtime offers as one kind with
/// [`Runtime::register_group`](crate::Runtime::register_group). It is
/// written as the calls that would make it by hand are:
///
/// ```
/// use serde_json::json;
/// use triq::{Group, Placement};
///
/// let stack = Group::new()
///     .spawn("estimator", "estimator", Placement::Dedicated, json!(null))
///     .spawn("controller", "controller", Placement::pool("control"), json!({ "gain": 2.0 }))
///     .connect("estimator/state", "controller/state");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub nodes: Vec<Member>,
    /// Each from an output to an input, both `node/port` by the names the
    /// nodes have in the group. The output may also be one that is not of
    /// the group: `runtime/node/port`. It has to be there when the group is
    /// asked for, or the group is not made.
    pub wires: Vec<(String, String)>,
}

/// One node of a [`Group`]: what [`Runtime::spawn_kind`](crate::Runtime::spawn_kind) is given to create it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub kind: String,
    /// What the node is called in the group. In a group that was created
    /// under the name `drone`, the node `estimator` is `drone.estimator`.
    pub name: String,
    pub placement: Placement,
    pub params: Value,
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node, of a kind the runtime has registered.
    pub fn spawn(mut self, kind: &str, name: &str, placement: Placement, params: Value) -> Self {
        self.nodes.push(Member { kind: kind.into(), name: name.into(), placement, params });
        self
    }

    /// Add a wire from an output to an input of a node of the group.
    pub fn connect(mut self, from: &str, to: &str) -> Self {
        self.wires.push((from.into(), to.into()));
        self
    }

    /// The wires of a group that was created under the name `name`, by what
    /// its nodes are called then: `name.node/port`. An output that is not
    /// of the group is as it was written.
    pub fn wired(&self, name: &str) -> Vec<(String, String)> {
        let port = |port: &String| if port.split('/').count() == 3 { port.clone() } else { format!("{name}.{port}") };
        self.wires.iter().map(|(from, to)| (port(from), port(to))).collect()
    }
}

impl Member {
    /// What the node is called in a group that was created under the name `group`.
    pub fn named(&self, group: &str) -> String {
        format!("{group}.{}", self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    /// The factory it came from; empty for nodes created directly.
    pub kind: String,
    /// The group it is in, as `outer/inner` for a group inside another: see
    /// [`Runtime::set_group`](crate::Runtime::set_group). Empty for a node
    /// that is in none, and from a runtime too old to say.
    #[serde(default)]
    pub group: String,
    /// `None` when the application runs the node itself.
    pub placement: Option<Placement>,
    pub state: NodeState,
    pub params: Value,
    pub runs: u64,
    /// How many runs returned an error, and the most recent one.
    pub errors: u64,
    pub last_error: Option<String>,
    pub inputs: Vec<InputInfo>,
    pub outputs: Vec<OutputInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Running,
    Paused,
    /// The node panicked, with this message, and will not run again.
    Stopped(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputInfo {
    pub port: PortSpec,
    /// Every output this input hears: the edges of the graph.
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// `runtime/node/port` of the output.
    pub output: String,
    /// The node that sends is there. False only for the moment between that
    /// node going and the input letting go of the output.
    pub connected: bool,
    pub received: u64,
    /// Samples overwritten in the queue before the input read them.
    pub lost: u64,
    /// When the most recent sample was sent, in nanoseconds since 1970 by the
    /// sender's clock. Two reports give an exact rate: the samples received
    /// between them, over the time between these. Zero until one arrives,
    /// and from a runtime too old to say.
    #[serde(default)]
    pub sent_ns: u64,
    /// Send-to-receive time of the most recent sample. Added to `sent_ns`, it
    /// is when the input had the sample.
    pub latency_us: u64,
    /// The longest time from one sample to the next, by when they were sent,
    /// over the last half second to second: read at least twice a second, no
    /// gap is missed. A hiccup of the sender shows here, and so does a hole
    /// left by lost samples. Zero until two samples have arrived in that
    /// time, and from a runtime too old to say.
    #[serde(default)]
    pub gap_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub port: PortSpec,
    /// `runtime/node/port`: the name inputs connect to.
    pub service: String,
    pub subscribers: usize,
    pub sent: u64,
    /// When the most recent sample was sent, in nanoseconds since 1970 by this
    /// program's clock. Zero until one is, and from a runtime too old to say.
    #[serde(default)]
    pub sent_ns: u64,
    /// Sends a reliable output gave up on.
    pub undelivered: u64,
}
