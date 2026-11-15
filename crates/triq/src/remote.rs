//! Talking to runtimes from outside: what a command-line tool, a graph editor
//! or another runtime uses. Nothing here needs to own a node.
use crate::control::{Caller, Reply, Request, announcements};
use crate::raw::{IpcNode, Listener, Svc, quiet_logs};
use crate::{Error, Inventory, Placement, Result};
use iceoryx2::prelude::{CallbackProgression, Config, NodeBuilder, Service, SignalHandlingMode};
use iceoryx2::service::static_config::messaging_pattern::MessagingPattern;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// A handle for asking one runtime to change.
///
/// Every request is carried out by the runtime that owns the nodes involved,
/// under its own rules; the program asking needs no rights over them.
pub struct Remote {
    caller: Arc<Caller>,
}

thread_local! {
    /// The runtimes this thread has opened, by name. Opening a runtime's control
    /// service costs many times what asking it something does, and a tool that
    /// shows the machine asks every runtime again and again.
    static OPENED: RefCell<BTreeMap<String, Arc<Caller>>> = const { RefCell::new(BTreeMap::new()) };
}

/// Hears every runtime on the machine say that it changed.
///
/// A tool that shows the graph creates one, then alternates between
/// [`discover`] and [`Watch::wait`]. A change made between the two is not
/// missed: it is still waiting to be heard.
pub struct Watch {
    listener: Listener,
}

impl Watch {
    pub fn new() -> Result<Self> {
        let listener = announcements(&client_node()?.lock().unwrap())?.listener_builder().create()?;
        Ok(Self { listener })
    }

    /// Sleep until a runtime starts, stops or changes, or until `timeout` passes.
    /// True if a change was heard. A runtime that is killed says nothing, so a
    /// tool should still look again now and then.
    pub fn wait(&self, timeout: Duration) -> Result<bool> {
        let mut heard = false;
        self.listener.timed_wait(|_| heard = true, timeout)?;
        Ok(heard)
    }
}

/// The inventory of every runtime running on this machine.
pub fn discover() -> Result<Vec<Inventory>> {
    quiet_logs();
    let mut runtimes = Vec::new();
    Svc::list(Config::global_config(), |service| {
        let details = &service.static_details;
        if let (Some(runtime), MessagingPattern::RequestResponse(_)) =
            (details.name().as_str().strip_suffix("/_control"), details.messaging_pattern())
        {
            runtimes.push(runtime.to_owned());
        }
        CallbackProgression::Continue
    })?;
    runtimes.sort();
    // What this thread still holds open of runtimes that are gone is let go.
    OPENED.with_borrow_mut(|opened| opened.retain(|runtime, _| runtimes.contains(runtime)));
    // A runtime that exits while we ask is simply not listed.
    Ok(runtimes.iter().filter_map(|runtime| Remote::open(runtime).and_then(|remote| remote.inventory()).ok()).collect())
}

/// Make the input `to` receive from the output `from`. Both are `runtime/node/port`.
/// The request goes to the runtime that owns the input.
pub fn connect(from: &str, to: &str) -> Result {
    let (runtime, input) = split(to)?;
    Remote::open(runtime)?.connect(from, input)
}

pub fn disconnect(from: &str, to: &str) -> Result {
    let (runtime, input) = split(to)?;
    Remote::open(runtime)?.disconnect(from, input)
}

/// `runtime/node/port` as the runtime and the rest.
fn split(name: &str) -> Result<(&str, &str)> {
    match name.split_once('/') {
        Some((runtime, rest)) if rest.split('/').count() == 2 => Ok((runtime, rest)),
        _ => Err(Error::Name(format!("{name:?} is not `runtime/node/port`"))),
    }
}

impl Remote {
    /// Fails with [`Error::Unknown`] if no runtime of that name is running.
    pub fn open(runtime: &str) -> Result<Self> {
        OPENED.with_borrow_mut(|opened| {
            if let Some(caller) = opened.get(runtime).filter(|caller| caller.answering()) {
                return Ok(Self { caller: caller.clone() });
            }
            // Never opened by this thread, or its runtime stopped since.
            opened.remove(runtime);
            let caller = Arc::new(Caller::open(&client_node()?.lock().unwrap(), runtime)?);
            opened.insert(runtime.to_owned(), caller.clone());
            Ok(Self { caller })
        })
    }

    pub fn inventory(&self) -> Result<Inventory> {
        match self.caller.ask(&Request::Inventory)? {
            Reply::Inventory(inventory) => Ok(*inventory),
            Reply::Done => Err(Error::Other("the runtime answered without an inventory".into())),
        }
    }

    /// Create a node of a kind the runtime has registered.
    pub fn spawn(&self, kind: &str, name: &str, placement: Placement, params: Value) -> Result {
        self.done(Request::Spawn { kind: kind.into(), name: name.into(), placement, params })
    }

    pub fn remove(&self, node: &str) -> Result {
        self.done(Request::Remove { node: node.into() })
    }

    /// `to` is `node/port` in this runtime; `from` is `runtime/node/port`, or
    /// `node/port` for an output of this same runtime.
    pub fn connect(&self, from: &str, to: &str) -> Result {
        self.done(Request::Connect { from: from.into(), to: to.into() })
    }

    pub fn disconnect(&self, from: &str, to: &str) -> Result {
        self.done(Request::Disconnect { from: from.into(), to: to.into() })
    }

    pub fn set_params(&self, node: &str, params: Value) -> Result {
        self.done(Request::SetParams { node: node.into(), params })
    }

    pub fn pause(&self, node: &str) -> Result {
        self.done(Request::Pause { node: node.into() })
    }

    pub fn resume(&self, node: &str) -> Result {
        self.done(Request::Resume { node: node.into() })
    }

    /// Ask the runtime's program to exit: its `Runtime::wait` returns.
    pub fn shutdown(&self) -> Result {
        match self.done(Request::Shutdown) {
            // The runtime exiting is what was asked for; its answer can be lost on the way out.
            Err(Error::Unknown(_)) => Ok(()),
            other => other,
        }
    }

    fn done(&self, request: Request) -> Result {
        self.caller.ask(&request).map(|_| ())
    }
}

/// One iceoryx2 node per process for outgoing requests, created on first use.
fn client_node() -> Result<&'static Mutex<IpcNode>> {
    static NODE: OnceLock<Mutex<IpcNode>> = OnceLock::new();
    if let Some(node) = NODE.get() {
        return Ok(node);
    }
    quiet_logs();
    let node = NodeBuilder::new().signal_handling_mode(SignalHandlingMode::Disabled).create::<Svc>()?;
    Ok(NODE.get_or_init(|| Mutex::new(node)))
}
