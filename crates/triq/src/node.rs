//! A node: your code, plus the ports it talks through.
use crate::port::InputPort;
use crate::raw::Listener;
use crate::{Error, Ports, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

/// Code that triq runs for you.
///
/// The node is only ever on one thread at a time. `run` is called when an
/// input has data. Take what you need from the inputs each time; `run` is not
/// called again for data it left waiting. A node can also, or instead, be
/// called on a fixed period: see [`trigger`](Node::trigger).
pub trait Node: Send + Sized + 'static {
    type Ports: Ports;
    /// Settings chosen when the node is created. Use `()` if there are none.
    /// A factory advertises `Params::default()` so a tool can offer a form.
    type Params: Serialize + DeserializeOwned + Default + Send + 'static;

    fn new(params: Self::Params) -> Result<Self>;

    fn run(&mut self, ports: &mut Self::Ports) -> Result;

    /// Called on every period, if the trigger has one. Does what `run` does
    /// unless the node says otherwise.
    fn tick(&mut self, ports: &mut Self::Ports) -> Result {
        self.run(ports)
    }

    /// What makes the node run.
    fn trigger(&self) -> Trigger {
        Trigger::Input
    }

    /// Apply new settings to the running node.
    fn set_params(&mut self, params: Self::Params) -> Result {
        let _ = params;
        Err(Error::Mismatch("this node's parameters cannot change while it runs".into()))
    }
}

/// What makes a node run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Data arriving on any input.
    Input,
    /// A fixed period, calling `tick`. Arriving data does not wake the node;
    /// it reads its inputs when it runs.
    Every(Duration),
    /// Data arriving, calling `run`, and a fixed period as well, calling `tick`.
    InputOrEvery(Duration),
}

impl Trigger {
    pub(crate) fn on_input(self) -> bool {
        !matches!(self, Self::Every(_))
    }

    pub(crate) fn period(self) -> Option<Duration> {
        match self {
            Self::Input => None,
            Self::Every(period) | Self::InputOrEvery(period) => Some(period),
        }
    }
}

/// Which thread runs a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    /// A thread of its own. The node may block without delaying anyone.
    Dedicated,
    /// A thread of the named pool. Whether the node keeps one thread or takes any free one
    /// was decided when the pool was declared.
    Pool(String),
}

impl Placement {
    pub fn pool(name: &str) -> Self {
        Self::Pool(name.into())
    }
}

/// A node as an executor sees it: something to run, and what to wait on for it.
pub(crate) struct Slot {
    body: Mutex<Box<dyn Body>>,
    inputs: Vec<Arc<dyn InputPort>>,
    trigger: Mutex<Trigger>,
    pub paused: AtomicBool,
    pub runs: AtomicU64,
    pub errors: AtomicU64,
    pub last_error: Mutex<Option<String>>,
    /// Set once, with the panic message, if `run` panicked.
    pub stopped: OnceLock<String>,
    /// Why the node should run next: set by the thread that waits, cleared by the thread that runs.
    due: AtomicU8,
    /// Used by shared pools only: keeps the node on one thread at a time.
    claim: AtomicU8,
}

/// Bits of `Slot::due`.
const INPUT: u8 = 1;
const TICK: u8 = 2;

/// Nobody is running the node.
const FREE: u8 = 0;
/// A worker has the node, or is about to.
const CLAIMED: u8 = 1;
/// A worker has the node, and more work arrived for it meanwhile.
const AGAIN: u8 = 2;

/// A node and its ports with the types erased.
trait Body: Send {
    fn run(&mut self) -> Result;
    fn tick(&mut self) -> Result;
    fn trigger(&self) -> Trigger;
    /// Returns the settings as they will be reported.
    fn set_params(&mut self, params: Value) -> Result<Value>;
}

struct Owned<N: Node> {
    node: N,
    ports: N::Ports,
}

impl<N: Node> Body for Owned<N> {
    fn run(&mut self) -> Result {
        self.node.run(&mut self.ports)
    }
    fn tick(&mut self) -> Result {
        self.node.tick(&mut self.ports)
    }
    fn trigger(&self) -> Trigger {
        self.node.trigger()
    }
    fn set_params(&mut self, params: Value) -> Result<Value> {
        let params: N::Params = serde_json::from_value(params)?;
        let shown = serde_json::to_value(&params)?;
        self.node.set_params(params)?;
        Ok(shown)
    }
}

impl Slot {
    pub fn new<N: Node>(node: N, ports: N::Ports, inputs: Vec<Arc<dyn InputPort>>) -> Self {
        Self {
            trigger: Mutex::new(node.trigger()),
            body: Mutex::new(Box::new(Owned { node, ports })),
            inputs,
            paused: AtomicBool::new(false),
            runs: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last_error: Mutex::new(None),
            stopped: OnceLock::new(),
            due: AtomicU8::new(0),
            claim: AtomicU8::new(FREE),
        }
    }

    /// Note that an input has data.
    pub fn input_arrived(&self) {
        self.due.fetch_or(INPUT, Ordering::AcqRel);
    }

    /// Note that a period elapsed.
    pub fn period_elapsed(&self) {
        self.due.fetch_or(TICK, Ordering::AcqRel);
    }

    /// Call the node for whatever is due. An error is recorded and the node
    /// goes on; a panic stops it for good.
    pub fn run(&self) {
        let due = self.due.swap(0, Ordering::AcqRel);
        if due == 0 || self.paused.load(Ordering::Acquire) || self.stopped.get().is_some() {
            return;
        }
        let mut body = self.body.lock().unwrap_or_else(PoisonError::into_inner);
        self.runs.fetch_add(1, Ordering::Relaxed);
        let call = |body: &mut Box<dyn Body>| -> Result {
            if due & INPUT != 0 {
                body.run()?;
            }
            if due & TICK != 0 {
                body.tick()?;
            }
            Ok(())
        };
        match catch_unwind(AssertUnwindSafe(|| call(&mut body))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.errors.fetch_add(1, Ordering::Relaxed);
                *self.last_error.lock().unwrap() = Some(error.to_string());
            }
            Err(panic) => {
                let message = panic
                    .downcast_ref::<&str>()
                    .map(|text| text.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panicked".into());
                let _ = self.stopped.set(message);
            }
        }
    }

    /// Ask for the node on behalf of a worker thread. True if the caller should
    /// queue it; false if a worker already has it and will now run it once more.
    pub fn claim(&self) -> bool {
        let claim = |state| Some(if state == FREE { CLAIMED } else { AGAIN });
        self.claim.fetch_update(Ordering::AcqRel, Ordering::Acquire, claim) == Ok(FREE)
    }

    /// Run a claimed node until no more work arrived for it, then give it up.
    pub fn run_claimed(&self) {
        loop {
            self.run();
            if self.claim.compare_exchange(CLAIMED, FREE, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return;
            }
            self.claim.store(CLAIMED, Ordering::Release);
        }
    }

    pub fn is_free(&self) -> bool {
        self.claim.load(Ordering::Acquire) == FREE
    }

    /// Waits for a run in progress, so the node never sees its settings change mid-run.
    pub fn set_params(&self, params: Value) -> Result<Value> {
        let mut body = self.body.lock().unwrap_or_else(PoisonError::into_inner);
        let shown = body.set_params(params)?;
        // New settings may change when the node runs.
        *self.trigger.lock().unwrap() = body.trigger();
        Ok(shown)
    }

    pub fn trigger(&self) -> Trigger {
        *self.trigger.lock().unwrap()
    }

    /// What wakes a node that runs on input: one listener per linked output of every input.
    pub fn listeners(&self) -> Vec<Arc<Listener>> {
        if !self.trigger().on_input() {
            return Vec::new();
        }
        self.inputs.iter().flat_map(|input| input.listeners()).collect()
    }
}
