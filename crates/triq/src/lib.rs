//! Nodes, factories and runtimes on top of iceoryx2.
//!
//! A **node** is your code plus a struct of typed ports. A **runtime** owns the
//! nodes of one process, runs them on the threads you choose, and answers
//! requests from other programs: list what exists, create a node from a
//! **factory**, connect an output to an input.
//!
//! triq adds no transport of its own. An [`Output`] is one iceoryx2
//! publish-subscribe service, an [`Input`] is a set of subscribers, and the
//! thread that runs a node sleeps on those subscribers directly. Messages are
//! plain `#[repr(C)]` data in shared memory: written once, read in place.
//!
//! ```no_run
//! use triq::prelude::*;
//!
//! #[derive(Ports)]
//! struct Through {
//!     input: Input<u64>,
//!     output: Output<u64>,
//! }
//!
//! struct Doubler;
//!
//! impl Node for Doubler {
//!     type Ports = Through;
//!     type Params = ();
//!     fn new(_: ()) -> triq::Result<Self> {
//!         Ok(Self)
//!     }
//!     fn run(&mut self, ports: &mut Through) -> triq::Result {
//!         while let Some(value) = ports.input.recv()? {
//!             ports.output.send(*value * 2)?;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> triq::Result {
//!     let rt = Runtime::new("demo")?;
//!     rt.register::<Doubler>("doubler")?; // any program may now ask for one
//!     rt.spawn::<Doubler>("twice", Placement::Dedicated, ())?;
//!     rt.wait();
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

// Lets `#[derive(Ports)]` expansions, which name `::triq`, resolve inside this crate too.
extern crate self as triq;

mod control;
mod error;
mod executor;
mod inventory;
mod message;
mod node;
mod port;
mod raw;
mod remote;
mod runtime;

pub use error::{Error, Result};
pub use inventory::{FactoryInfo, InputInfo, Inventory, NodeInfo, NodeState, OutputInfo, PoolInfo, SourceInfo};
pub use message::{Headed, Header, MessageType, Payload, Stamp};
pub use node::{Node, Placement, Trigger};
pub use port::{Binder, Direction, Input, Loan, Output, PortField, PortOptions, PortSpec, Ports, Sample};
pub use remote::{Remote, Watch, connect, disconnect, discover};
pub use runtime::{Builder, Local, LocalFactory, Runtime};
pub use triq_macros::Ports;

/// `use triq::prelude::*;` brings in everything a node needs.
pub mod prelude {
    // `Ports` and `ZeroCopySend` each name a trait and its derive macro.
    pub use crate::{Error, Input, Node, Output, Placement, Ports, Result, Runtime, Sample, Trigger};
    pub use iceoryx2::prelude::ZeroCopySend;
}
