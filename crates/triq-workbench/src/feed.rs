//! The workbench's link to the machine, kept off the thread that draws.
//!
//! One thread keeps the list of inventories fresh: it listens for runtimes
//! announcing a change and asks them again. Another carries out what the user
//! asked for, one request at a time, so a slow runtime never freezes the window.
use serde_json::Value;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use triq::{Inventory, Placement, Remote, Watch};

/// How often counters are refreshed when nothing announces a change.
const REFRESH: Duration = Duration::from_millis(500);

/// Something the user asked for. Each becomes one request to the runtime that owns the node.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
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
    /// Both ends are `runtime/node/port`.
    Connect {
        from: String,
        to: String,
    },
    Disconnect {
        from: String,
        to: String,
    },
    SetParams {
        runtime: String,
        node: String,
        params: Value,
    },
    Pause {
        runtime: String,
        node: String,
    },
    Resume {
        runtime: String,
        node: String,
    },
}

pub struct Feed {
    shared: Arc<Shared>,
    requests: mpsc::Sender<Request>,
}

struct Shared {
    inventories: Mutex<Arc<Vec<Inventory>>>,
    /// The most recent request that failed, and why.
    problem: Mutex<Option<String>>,
    stop: AtomicBool,
}

impl Feed {
    /// `redraw` is called from the background whenever there is something new to show.
    pub fn start(redraw: impl Fn() + Send + Sync + 'static) -> triq::Result<Self> {
        let shared =
            Arc::new(Shared { inventories: Mutex::default(), problem: Mutex::default(), stop: AtomicBool::new(false) });
        let redraw = Arc::new(redraw);
        let watch = Watch::new()?;
        thread::Builder::new().name("workbench-watch".into()).spawn({
            let (shared, redraw) = (shared.clone(), redraw.clone());
            move || {
                while !shared.stop.load(Ordering::Acquire) {
                    if let Ok(found) = triq::discover() {
                        *shared.inventories.lock().unwrap() = Arc::new(found);
                        redraw();
                    }
                    let _ = watch.wait(REFRESH);
                }
            }
        })?;
        let (requests, queue) = mpsc::channel::<Request>();
        thread::Builder::new().name("workbench-requests".into()).spawn({
            let shared = shared.clone();
            move || {
                for request in queue {
                    if let Err(error) = request.carry_out() {
                        *shared.problem.lock().unwrap() = Some(format!("Could not {request}: {error}"));
                        redraw();
                    }
                }
            }
        })?;
        Ok(Self { shared, requests })
    }

    /// What every runtime on the machine last reported.
    pub fn inventories(&self) -> Arc<Vec<Inventory>> {
        self.shared.inventories.lock().unwrap().clone()
    }

    /// Carry out a request in the background. Its effect shows in a later inventory.
    pub fn ask(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// Show the user a problem that did not come from a runtime.
    pub fn report(&self, problem: String) {
        *self.shared.problem.lock().unwrap() = Some(problem);
    }

    /// The most recent thing that went wrong, until it is dismissed.
    pub fn problem(&self) -> Option<String> {
        self.shared.problem.lock().unwrap().clone()
    }

    pub fn dismiss(&self) {
        *self.shared.problem.lock().unwrap() = None;
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

impl Request {
    fn carry_out(&self) -> triq::Result {
        match self {
            Self::Spawn { runtime, kind, name, placement, params } => {
                Remote::open(runtime)?.spawn(kind, name, placement.clone(), params.clone())
            }
            Self::Remove { runtime, node } => Remote::open(runtime)?.remove(node),
            Self::Connect { from, to } => triq::connect(from, to),
            Self::Disconnect { from, to } => triq::disconnect(from, to),
            Self::SetParams { runtime, node, params } => Remote::open(runtime)?.set_params(node, params.clone()),
            Self::Pause { runtime, node } => Remote::open(runtime)?.pause(node),
            Self::Resume { runtime, node } => Remote::open(runtime)?.resume(node),
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { runtime, kind, name, .. } => write!(f, "create {kind} node {name} in {runtime}"),
            Self::Remove { runtime, node } => write!(f, "remove {runtime}/{node}"),
            Self::Connect { from, to } => write!(f, "connect {from} to {to}"),
            Self::Disconnect { from, to } => write!(f, "disconnect {from} from {to}"),
            Self::SetParams { runtime, node, .. } => write!(f, "change the settings of {runtime}/{node}"),
            Self::Pause { runtime, node } => write!(f, "pause {runtime}/{node}"),
            Self::Resume { runtime, node } => write!(f, "resume {runtime}/{node}"),
        }
    }
}
