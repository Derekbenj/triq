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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    /// The factory it came from; empty for nodes created directly.
    pub kind: String,
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
    /// Every output this input wants to hear from: the edges of the graph.
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// `runtime/node/port` of the output.
    pub output: String,
    /// The output exists and has a live sender.
    pub connected: bool,
    /// Why subscribing failed, if it did for a reason other than the output not existing yet.
    pub problem: Option<String>,
    pub received: u64,
    /// Samples overwritten in the queue before the input read them.
    pub lost: u64,
    /// Send-to-receive time of the most recent sample.
    pub latency_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub port: PortSpec,
    /// `runtime/node/port`: the name inputs connect to.
    pub service: String,
    pub subscribers: usize,
    pub sent: u64,
    /// Sends a reliable output gave up on.
    pub undelivered: u64,
}
