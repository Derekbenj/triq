//! Talking to runtimes from outside: what a command-line tool, a graph editor
//! or another runtime uses. Nothing here needs to own a node.
use crate::control::{Caller, Reply, Request, announcements};
use crate::raw::{IpcNode, Listener, Svc, quiet_logs};
use crate::{Error, Inventory, Layout, Placement, Result};
use iceoryx2::prelude::{CallbackProgression, Config, NodeBuilder, Service, SignalHandlingMode};
use iceoryx2::service::ServiceDynamicDetails;
use iceoryx2::service::static_config::messaging_pattern::MessagingPattern;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// A handle for asking one runtime to change.
///
/// Every request is carried out by the runtime that owns the nodes involved,
/// under its own rules; the program asking needs no rights over them.
pub struct Remote {
    opened: Arc<Opened>,
}

/// A runtime as this program has it open.
struct Opened {
    caller: Caller,
    /// What the caller was made with. It is let go of after the caller.
    _node: ClientNode,
}

thread_local! {
    /// The runtimes this thread has opened, by name. Opening a runtime's control
    /// service costs many times what asking it something does, and a tool that
    /// shows the machine asks every runtime again and again.
    static OPENED: RefCell<BTreeMap<String, Arc<Opened>>> = const { RefCell::new(BTreeMap::new()) };
}

/// Hears every runtime on the machine say that it changed.
///
/// A tool that shows the graph creates one, then alternates between
/// [`discover`] and [`Watch::wait`]. A change made between the two is not
/// missed: it is still waiting to be heard.
pub struct Watch {
    listener: Listener,
    _node: ClientNode,
}

impl Watch {
    pub fn new() -> Result<Self> {
        let node = client_node()?;
        let listener = announcements(&node.lock().unwrap())?.listener_builder().create()?;
        Ok(Self { listener, _node: node })
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
            && held(service.dynamic_details.as_ref())
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

/// Let go of every runtime this thread has opened.
///
/// A thread keeps the runtimes it has opened, to ask them again cheaply, and
/// lets go of them when it ends. The main thread of a program does not end
/// that way. Call this before `main` returns, and the program leaves nothing
/// of itself on the machine for another one to clear away.
pub fn release() {
    OPENED.with_borrow_mut(BTreeMap::clear);
}

/// Whether any program holds a service, by what iceoryx2 reports of it.
///
/// A program that ends badly can leave a service behind that nobody holds and
/// that iceoryx2 never clears away. Such a one is not a runtime, and is passed
/// over without being opened.
fn held(service: Option<&ServiceDynamicDetails<Svc>>) -> bool {
    service.is_some_and(|service| !service.nodes.is_empty())
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
            if let Some(kept) = opened.get(runtime).filter(|kept| kept.caller.answering()) {
                return Ok(Self { opened: kept.clone() });
            }
            // Never opened by this thread, or its runtime stopped since.
            opened.remove(runtime);
            let node = client_node()?;
            let caller = Caller::open(&node.lock().unwrap(), runtime)?;
            let kept = Arc::new(Opened { caller, _node: node });
            opened.insert(runtime.to_owned(), kept.clone());
            Ok(Self { opened: kept })
        })
    }

    pub fn inventory(&self) -> Result<Inventory> {
        match self.opened.caller.ask(&Request::Inventory)? {
            Reply::Inventory(inventory) => Ok(*inventory),
            _ => Err(Error::Other("the runtime answered without an inventory".into())),
        }
    }

    /// Which numbers the messages of an output hold, and where: enough to
    /// read them from the bytes of a message without having its type.
    pub fn layout(&self, node: &str, port: &str) -> Result<Layout> {
        match self.opened.caller.ask(&Request::Layout { node: node.into(), port: port.into() })? {
            Reply::Layout(layout) => Ok(layout),
            _ => Err(Error::Other("the runtime answered without a layout".into())),
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

    /// Put a node in a group, or with an empty name in none. See
    /// [`Runtime::set_group`](crate::Runtime::set_group).
    pub fn set_group(&self, node: &str, group: &str) -> Result {
        self.done(Request::SetGroup { node: node.into(), group: group.into() }).map_err(too_old)
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
        self.opened.caller.ask(&request).map(|_| ())
    }
}

/// A runtime that was built with an earlier triq cannot read a request that
/// came later, and says so in the words of the JSON reader. This says what
/// that means.
fn too_old(error: Error) -> Error {
    match error {
        Error::Other(reason) if reason.starts_with("unknown variant") => {
            Error::Mismatch("the runtime was built with an earlier triq, which does not know this request".into())
        }
        other => other,
    }
}

/// The iceoryx2 node that a program's requests go out from.
type ClientNode = Arc<Mutex<IpcNode>>;

/// One node per process for outgoing requests. It is made on first use and
/// lasts for as long as anything made with it is held: an opened runtime, or
/// a [`Watch`]. A program that has let go of all of them has no node.
fn client_node() -> Result<ClientNode> {
    static NODE: Mutex<Weak<Mutex<IpcNode>>> = Mutex::new(Weak::new());
    let mut kept = NODE.lock().unwrap();
    if let Some(node) = kept.upgrade() {
        return Ok(node);
    }
    quiet_logs();
    let node =
        Arc::new(Mutex::new(NodeBuilder::new().signal_handling_mode(SignalHandlingMode::Disabled).create::<Svc>()?));
    *kept = Arc::downgrade(&node);
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_that_nobody_holds_is_passed_over() {
        // Left behind with nothing but its file,
        assert!(!held(None));
        // or with nobody listed as holding it.
        assert!(!held(Some(&ServiceDynamicDetails { nodes: Vec::new() })));
    }

    #[test]
    fn a_request_that_an_earlier_triq_does_not_know_is_refused_in_plain_words() {
        // What a runtime answers that was built before the request existed.
        let unread = serde_json::from_str::<Request>(r#"{"FromLater":{"node":"x"}}"#).map_err(Error::from).unwrap_err();
        assert!(matches!(too_old(unread), Error::Mismatch(reason) if reason.contains("earlier triq")));
        // Any other refusal is passed on as it is.
        assert_eq!(too_old(Error::Unknown("node".into())), Error::Unknown("node".into()));
    }
}
