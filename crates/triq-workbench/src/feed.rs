//! The workbench's link to the machine, kept off the thread that draws.
//!
//! One thread keeps the list of inventories fresh: it asks every runtime ten
//! times a second, and at once when one announces a change. Which runtimes
//! there are it finds out when one announces a change, and once a second
//! besides: looking the machine over takes far longer than asking them. From
//! one reading of the counters to another it works out how fast data is
//! flowing, and it keeps what each connection did as history. Another thread carries out what
//! the user asked for, one request at a time, so a slow runtime never freezes
//! the window. Requests that belong together are carried out in their order,
//! as far as the runtimes go along with them. A third thread reads the topics
//! whose messages the user looks into.
//!
//! What was asked for shows a moment later, in what the runtimes report
//! next. [`Feed::settled`] says whether that moment has passed.
use crate::graph;
use crate::history::History;
use crate::signals::{Signals, ended};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use triq::{Inventory, Placement, Remote, Watch};

/// How often the machine is read when nothing announces a change. Every
/// reading is a point on a chart.
const REFRESH: Duration = Duration::from_millis(100);
/// How often the machine is looked over for runtimes when none has announced a change, in case
/// one came without saying so. Looking it over takes a millisecond or so for every service
/// there is: with fifty outputs running, as long as there is between two readings.
const LOOKED_OVER: Duration = Duration::from_secs(2);
/// How often the rates beside the pins are worked out again. Changing ten
/// times a second they would be hard to read, and counted over so short a
/// time they would be rough.
const RATES: Duration = Duration::from_millis(500);

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
    /// Put a node in a group, as `outer/inner` for one inside another; or, with an empty name, in none.
    SetGroup {
        runtime: String,
        node: String,
        group: String,
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
    /// Requests, each time those that belong together. `None` only while the feed is being dropped.
    requests: Option<mpsc::Sender<Vec<Request>>>,
    /// The thread that watches the machine, and the one that carries out requests.
    threads: Vec<JoinHandle<()>>,
    signals: Signals,
}

struct Shared {
    inventories: Mutex<Arc<Vec<Inventory>>>,
    /// Samples per second through each port, by `runtime/node/port`.
    rates: Mutex<Arc<BTreeMap<String, f64>>>,
    history: Mutex<History>,
    /// The most recent request that failed, and why.
    problem: Mutex<Option<String>>,
    /// Of requests that belonged together, the one that failed and those after it.
    unmade: Mutex<Vec<Request>>,
    /// How many times something was asked for, how many of those were seen to, and how
    /// many had been seen to when the reading that `inventories` holds was begun.
    asked: AtomicU64,
    seen_to: AtomicU64,
    shown: AtomicU64,
    stop: AtomicBool,
}

impl Feed {
    /// `redraw` is called from the background whenever there is something new to show.
    pub fn start(redraw: impl Fn() + Send + Sync + 'static) -> triq::Result<Self> {
        let shared = Arc::new(Shared {
            inventories: Mutex::default(),
            rates: Mutex::default(),
            history: Mutex::default(),
            problem: Mutex::default(),
            unmade: Mutex::default(),
            asked: AtomicU64::new(0),
            seen_to: AtomicU64::new(0),
            shown: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let redraw = Arc::new(redraw);
        let watch = Watch::new()?;
        let watching = thread::Builder::new().name("workbench-watch".into()).spawn({
            let (shared, redraw) = (shared.clone(), redraw.clone());
            move || {
                // The reading the pins' rates are measured from, and when it was made.
                let mut rated = (Instant::now(), Arc::new(Vec::new()));
                // When history last took a reading, and when the next regular one is due.
                let (mut recorded, mut due) = (None::<Instant>, Instant::now());
                // The runtimes that are there, when the machine was last looked over for them,
                // and whether one has announced a change since.
                let (mut runtimes, mut looked, mut heard) = (Vec::<String>::new(), None::<Instant>, true);
                while !shared.stop.load(Ordering::Acquire) {
                    // Whatever was seen to before the machine is read shows in the reading.
                    let seen_to = shared.seen_to.load(Ordering::Acquire);
                    let found = if heard || looked.is_none_or(|at| at.elapsed() >= LOOKED_OVER) {
                        looked = Some(Instant::now());
                        triq::discover()
                    } else {
                        // A runtime that does not answer is gone, and is left out until it is found again.
                        let asked = |runtime: &String| Remote::open(runtime).and_then(|remote| remote.inventory()).ok();
                        Ok(runtimes.iter().filter_map(asked).collect())
                    };
                    if let Ok(found) = found {
                        runtimes = found.iter().map(|inventory| inventory.runtime.clone()).collect();
                        let found = Arc::new(found);
                        // Changes can be announced in quick succession, and two
                        // readings a few milliseconds apart would say little.
                        if recorded.is_none_or(|at| at.elapsed() >= REFRESH / 2) {
                            shared.history.lock().unwrap().record(&found);
                            recorded = Some(Instant::now());
                        }
                        if rated.0.elapsed() >= RATES {
                            *shared.rates.lock().unwrap() = Arc::new(graph::rates(&rated.1, &found));
                            rated = (Instant::now(), found.clone());
                        }
                        *shared.inventories.lock().unwrap() = found;
                        shared.shown.store(seen_to, Ordering::Release);
                        redraw();
                    }
                    // Sleep until a runtime announces a change or the next reading is due.
                    let now = Instant::now();
                    while due <= now {
                        due += REFRESH;
                    }
                    // Not hearing is no proof that nothing changed: then the machine is looked over.
                    heard = watch.wait(due - now).unwrap_or(true);
                }
            }
        })?;
        let (requests, queue) = mpsc::channel::<Vec<Request>>();
        let asking = thread::Builder::new().name("workbench-requests".into()).spawn({
            let shared = shared.clone();
            move || {
                for together in queue {
                    let refused = together.iter().enumerate().find_map(|(made, request)| {
                        let error = request.carry_out().err()?;
                        Some((made, format!("Could not {request}: {error}")))
                    });
                    shared.seen_to.fetch_add(1, Ordering::AcqRel);
                    let Some((made, mut problem)) = refused else { continue };
                    // Of several, the one that was refused and those after it go back to whoever asked.
                    if together.len() > 1 {
                        problem +=
                            &format!(". {made} of the {} changes were made, the others are held back", together.len());
                        shared.unmade.lock().unwrap().extend_from_slice(&together[made..]);
                    }
                    *shared.problem.lock().unwrap() = Some(problem);
                    redraw();
                }
            }
        })?;
        Ok(Self { shared, requests: Some(requests), threads: vec![watching, asking], signals: Signals::start()? })
    }

    /// The numbers inside the messages of the topics that are watched.
    pub fn signals(&self) -> &Signals {
        &self.signals
    }

    /// What every runtime on the machine last reported.
    pub fn inventories(&self) -> Arc<Vec<Inventory>> {
        self.shared.inventories.lock().unwrap().clone()
    }

    /// Samples per second through each port, by `runtime/node/port`, over the last half second or so.
    pub fn rates(&self) -> Arc<BTreeMap<String, f64>> {
        self.shared.rates.lock().unwrap().clone()
    }

    /// What every connection did over the last hour, ten points a second.
    /// Readings wait while this is held, so let go of it once a frame is drawn.
    pub fn history(&self) -> MutexGuard<'_, History> {
        self.shared.history.lock().unwrap()
    }

    /// Carry out a request in the background. Its effect shows in a later inventory.
    pub fn ask(&self, request: Request) {
        self.apply(vec![request]);
    }

    /// Carry out several requests in the background, in this order. If a
    /// runtime refuses one of them, that one and those after it are not
    /// made: [`Feed::unmade`] hands them back.
    pub fn apply(&self, requests: Vec<Request>) {
        if let Some(queue) = &self.requests {
            self.shared.asked.fetch_add(1, Ordering::AcqRel);
            let _ = queue.send(requests);
        }
    }

    /// Whether everything that was asked for shows in what [`Feed::inventories`]
    /// reports from now on: it was carried out or refused, and the machine was
    /// read after that. Until then, something that was just asked for may look
    /// as if it were not there. Ask this before the inventories, not after.
    pub fn settled(&self) -> bool {
        self.shared.shown.load(Ordering::Acquire) == self.shared.asked.load(Ordering::Acquire)
    }

    /// The requests of [`Feed::apply`] that were not made, handed back once.
    pub fn unmade(&self) -> Vec<Request> {
        std::mem::take(&mut self.shared.unmade.lock().unwrap())
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
        // With nobody left to ask, the thread that carries out requests ends. The one that
        // watches the machine looks for the stop within a tenth of a second.
        self.requests = None;
        self.threads.drain(..).for_each(ended);
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
            Self::SetGroup { runtime, node, group } => Remote::open(runtime)?.set_group(node, group),
            Self::Pause { runtime, node } => Remote::open(runtime)?.pause(node),
            Self::Resume { runtime, node } => Remote::open(runtime)?.resume(node),
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { runtime, kind, name, .. } => write!(f, "create {kind} {name} in {runtime}"),
            Self::Remove { runtime, node } => write!(f, "remove {runtime}/{node}"),
            Self::Connect { from, to } => write!(f, "connect {from} to {to}"),
            Self::Disconnect { from, to } => write!(f, "disconnect {from} from {to}"),
            Self::SetParams { runtime, node, .. } => write!(f, "change the settings of {runtime}/{node}"),
            Self::SetGroup { runtime, node, group } if group.is_empty() => {
                write!(f, "take {runtime}/{node} out of its group")
            }
            Self::SetGroup { runtime, node, group } => write!(f, "put {runtime}/{node} in group {group}"),
            Self::Pause { runtime, node } => write!(f, "pause {runtime}/{node}"),
            Self::Resume { runtime, node } => write!(f, "resume {runtime}/{node}"),
        }
    }
}
