//! Ports: the typed ends of a connection.
//!
//! An [`Output`] owns one iceoryx2 service. An [`Input`] subscribes to any
//! number of them. A struct of such fields, with `#[derive(Ports)]`, is the
//! whole declaration of what a node can be wired to.
mod input;
mod output;

pub use input::{Input, Sample};
pub use output::{Loan, Output};

pub(crate) use input::InputPort;
pub(crate) use output::OutputPort;

use crate::raw::IpcNode;
use crate::{MessageType, Payload, Result, runtime};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Input,
    Output,
}

/// Set with `#[port(...)]` on a field. Everything left out uses the default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortOptions {
    /// Output: how many samples each subscriber can have waiting.
    /// Defaults to 32 for fixed-size messages and 2 for slices.
    pub queue: Option<usize>,
    /// Output: how many inputs may subscribe. Defaults to 8.
    pub subscribers: Option<usize>,
    /// Output: how many samples a subscriber may hold on to at once. Defaults to 2.
    pub hold: Option<usize>,
    /// Output: wait this long for a full subscriber instead of dropping its oldest sample.
    pub reliable_ms: Option<u64>,
    /// Input: how many samples to keep waiting, at most the output's `queue`.
    pub depth: Option<usize>,
}

/// One port of a node, as advertised before and after the node exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortSpec {
    pub name: String,
    pub direction: Direction,
    pub message: MessageType,
    pub options: PortOptions,
}

impl PortSpec {
    fn new<T: Payload + ?Sized>(name: &str, direction: Direction, options: PortOptions) -> Self {
        Self { name: name.into(), direction, message: T::message_type(), options }
    }
}

/// A struct of ports. Derive it; the field names are the port names.
pub trait Ports: Sized + Send + 'static {
    /// The declaration, available without creating anything.
    fn spec() -> Vec<PortSpec>;
    /// Create the live ports of one node.
    fn bind(binder: &mut Binder) -> Result<Self>;
}

/// A field type usable inside a [`Ports`] struct: [`Input`] or [`Output`].
pub trait PortField: Sized {
    fn spec(name: &str, options: PortOptions) -> PortSpec;
    fn bind(binder: &mut Binder, name: &str, options: PortOptions) -> Result<Self>;
}

/// Creates the live ports of one node and hands the runtime a handle to each.
pub struct Binder<'a> {
    pub(crate) ipc: &'a IpcNode,
    pub(crate) runtime: &'a str,
    pub(crate) node: &'a str,
    pub(crate) guard: Arc<NodeGuard>,
    pub(crate) inputs: Vec<(PortSpec, Arc<dyn InputPort>)>,
    pub(crate) outputs: Vec<(PortSpec, Arc<dyn OutputPort>)>,
}

/// Shared by all port handles of one node. When the last handle is dropped,
/// the node disappears from its runtime.
pub(crate) struct NodeGuard {
    pub runtime: Weak<runtime::Inner>,
    pub node: String,
    pub id: u64,
}

impl Drop for NodeGuard {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.forget(&self.node, self.id);
        }
    }
}

impl<T: Payload + ?Sized> PortField for Output<T> {
    fn spec(name: &str, options: PortOptions) -> PortSpec {
        PortSpec::new::<T>(name, Direction::Output, options)
    }
    fn bind(binder: &mut Binder, name: &str, options: PortOptions) -> Result<Self> {
        let spec = Self::spec(name, options);
        let service = format!("{}/{}/{name}", binder.runtime, binder.node);
        let port = format!("{}/{name}", binder.node);
        let shared = Arc::new(output::Shared::create(binder.ipc, &service, &port, &spec.options)?);
        binder.outputs.push((spec, shared.clone()));
        Ok(Output::new(shared, binder.guard.clone()))
    }
}

impl<T: Payload + ?Sized> PortField for Input<T> {
    fn spec(name: &str, options: PortOptions) -> PortSpec {
        PortSpec::new::<T>(name, Direction::Input, options)
    }
    fn bind(binder: &mut Binder, name: &str, options: PortOptions) -> Result<Self> {
        let spec = Self::spec(name, options);
        let port = format!("{}/{name}", binder.node);
        let shared = Arc::new(input::Shared::create(&port, &spec.options)?);
        binder.inputs.push((spec, shared.clone()));
        Ok(Input::new(shared, binder.guard.clone()))
    }
}
