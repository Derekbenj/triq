//! What the app knows of the machine, and what it asks of it, both off the thread that draws.
//!
//! One thread looks at the machine: whenever a runtime says it changed, and twice a second
//! besides, since a runtime that is killed says nothing. Another does what is asked, one
//! request to the runtime concerned at a time, so that a runtime slow to answer holds up
//! neither the drawing nor the looking.
use serde_json::Value;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use triq::{Inventory, Placement, Remote, Watch};

/// Something asked of a runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Spawn {
        runtime: String,
        kind: String,
        name: String,
        placement: Placement,
        params: Value,
    },
    Remove {
        runtime: String,
        node: String,
    },
    Pause {
        runtime: String,
        node: String,
    },
    Resume {
        runtime: String,
        node: String,
    },
    /// Change the settings named, and leave the node's others as they are.
    Change {
        runtime: String,
        node: String,
        changes: Value,
    },
    /// Both ends are `runtime/node/port`.
    Connect {
        from: String,
        to: String,
    },
    Disconnect {
        from: String,
        to: String,
    },
    Shutdown {
        runtime: String,
    },
}

impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { runtime, kind, name, .. } => write!(f, "create {runtime}/{name}, a {kind}"),
            Self::Remove { runtime, node } => write!(f, "remove {runtime}/{node}"),
            Self::Pause { runtime, node } => write!(f, "pause {runtime}/{node}"),
            Self::Resume { runtime, node } => write!(f, "resume {runtime}/{node}"),
            Self::Change { runtime, node, changes } => write!(f, "change {runtime}/{node} {changes}"),
            Self::Connect { from, to } => write!(f, "connect {from} to {to}"),
            Self::Disconnect { from, to } => write!(f, "disconnect {from} from {to}"),
            Self::Shutdown { runtime } => write!(f, "shut down {runtime}"),
        }
    }
}

/// Ask it, and wait for the runtime's answer.
pub fn perform(ask: &Ask) -> triq::Result {
    match ask {
        Ask::Spawn { runtime, kind, name, placement, params } => {
            Remote::open(runtime)?.spawn(kind, name, placement.clone(), params.clone())
        }
        Ask::Remove { runtime, node } => Remote::open(runtime)?.remove(node),
        Ask::Pause { runtime, node } => Remote::open(runtime)?.pause(node),
        Ask::Resume { runtime, node } => Remote::open(runtime)?.resume(node),
        Ask::Change { runtime, node, changes } => Remote::open(runtime)?.change_params(node, changes.clone()),
        Ask::Connect { from, to } => triq::connect(from, to),
        Ask::Disconnect { from, to } => triq::disconnect(from, to),
        Ask::Shutdown { runtime } => Remote::open(runtime)?.shutdown(),
    }
}

/// The machine as it was last seen, and the way to ask things of it.
pub struct Feed {
    machine: Arc<Mutex<Vec<Inventory>>>,
    /// What went wrong with the last thing that did: an asking refused, or the looking itself.
    said: Arc<Mutex<Option<String>>>,
    /// None only while it is being dropped.
    asks: Option<Sender<Ask>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Feed {
    pub fn start() -> Self {
        let (machine, said) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(None)));
        let stop = Arc::new(AtomicBool::new(false));
        let looking = std::thread::spawn({
            let (machine, said, stop) = (machine.clone(), said.clone(), stop.clone());
            move || {
                let watch = Watch::new();
                while !stop.load(Ordering::Acquire) {
                    match triq::discover() {
                        Ok(mut found) => {
                            found.sort_by(|a, b| a.runtime.cmp(&b.runtime));
                            *machine.lock().unwrap() = found;
                        }
                        Err(error) => *said.lock().unwrap() = Some(format!("could not look at the machine: {error}")),
                    }
                    let waited = watch.as_ref().map(|watch| watch.wait(Duration::from_millis(500)));
                    if !matches!(waited, Ok(Ok(_))) {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                triq::release();
            }
        });
        let (asks, asked) = mpsc::channel::<Ask>();
        let asking = std::thread::spawn({
            let said = said.clone();
            move || {
                for ask in asked {
                    if let Err(error) = perform(&ask) {
                        *said.lock().unwrap() = Some(format!("could not {ask}: {error}"));
                    }
                }
                triq::release();
            }
        });
        Self { machine, said, asks: Some(asks), stop, threads: vec![looking, asking] }
    }

    /// Every runtime on the machine, in the order of their names, as last seen.
    pub fn machine(&self) -> Vec<Inventory> {
        self.machine.lock().unwrap().clone()
    }

    pub fn ask(&self, ask: Ask) {
        if let Some(asks) = &self.asks {
            let _ = asks.send(ask);
        }
    }

    /// What went wrong since this was last asked, if anything did.
    pub fn said(&self) -> Option<String> {
        self.said.lock().unwrap().take()
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.asks = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
