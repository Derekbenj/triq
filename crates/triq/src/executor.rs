//! An executor: one thread that waits for work and runs nodes.
//!
//! The thread sleeps in an iceoryx2 `WaitSet` holding one listener per
//! connected output and one timer per periodic node. A sender wakes it
//! directly, so a message costs one thread wake-up and nothing in between.
//! What the thread waits on is rebuilt only when the nodes or their
//! connections change.
mod workers;

use crate::Result;
use crate::node::Slot;
use crate::raw::{IpcNode, Listener, Notifier, Svc};
use iceoryx2::prelude::{CallbackProgression, ServiceName, SignalHandlingMode, WaitSetAttachmentId, WaitSetBuilder};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use workers::Workers;

pub(crate) struct Executor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    slots: Mutex<Vec<Arc<Slot>>>,
    /// Bumped whenever the thread must look again at what it waits on.
    version: AtomicU64,
    /// The version the thread is currently waiting with.
    applied: AtomicU64,
    stop: AtomicBool,
    /// Set by the thread as it exits.
    finished: AtomicBool,
    /// Interrupts the thread's wait.
    wake: Notifier,
    /// In a shared pool, the threads that run the nodes instead of this one.
    workers: Option<Workers>,
}

/// Why the thread woke up.
#[derive(Clone, Copy)]
enum Wake {
    /// Data for the slot at this index, signalled by the listener at that index.
    Input(usize, usize),
    Tick(usize),
}

impl Executor {
    /// With `workers == 0` the thread runs its nodes itself; otherwise it only
    /// waits, and hands each ready node to one of that many worker threads.
    pub fn start(ipc: &IpcNode, runtime: &str, name: &str, workers: usize) -> Result<Self> {
        let service =
            ipc.service_builder(&ServiceName::new(&format!("{runtime}/_executor/{name}"))?).event().open_or_create()?;
        let listener = service.listener_builder().create()?;
        let shared = Arc::new(Shared {
            slots: Mutex::new(Vec::new()),
            version: AtomicU64::new(0),
            applied: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            wake: service.notifier_builder().create()?,
            workers: if workers == 0 { None } else { Some(Workers::start(name, workers)?) },
        });
        let thread = thread::Builder::new().name(format!("triq-{name}")).spawn({
            let shared = shared.clone();
            move || {
                if let Err(error) = serve(&shared, &listener) {
                    eprintln!("triq: executor thread stopped: {error}");
                }
                shared.finished.store(true, Ordering::Release);
            }
        })?;
        Ok(Self { shared, thread: Some(thread) })
    }

    pub fn add(&self, slot: Arc<Slot>) {
        self.shared.slots.lock().unwrap().push(slot);
        self.refresh();
    }

    /// Returns once the thread no longer refers to the slot, so the caller's
    /// reference is the last one. Gives up waiting if the node is stuck in `run`.
    pub fn remove(&self, slot: &Arc<Slot>) {
        self.shared.slots.lock().unwrap().retain(|other| !Arc::ptr_eq(other, slot));
        let version = self.refresh();
        wait_until(|| self.shared.applied.load(Ordering::Acquire) >= version || self.finished());
        wait_until(|| slot.is_free());
    }

    fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// Tell the thread that nodes, connections or periods changed.
    pub fn refresh(&self) -> u64 {
        let version = self.shared.version.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self.shared.wake.notify();
        version
    }

    /// How many nodes this executor runs.
    pub fn load(&self) -> usize {
        self.shared.slots.lock().unwrap().len()
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.shared.wake.notify();
        let Some(thread) = self.thread.take() else {
            return;
        };
        // A node stuck in `run` cannot be interrupted; do not wait for it forever.
        let own = thread.thread().id() == thread::current().id();
        if !own && wait_until(|| self.finished()) {
            let _ = thread.join();
        }
    }
}

impl Shared {
    fn run(&self, slot: &Arc<Slot>) {
        match &self.workers {
            None => slot.run(),
            Some(workers) => workers.submit(slot),
        }
    }
}

/// Poll for up to two seconds.
pub(crate) fn wait_until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done() && Instant::now() < deadline {
        thread::sleep(Duration::from_micros(100));
    }
    done()
}

fn serve(shared: &Shared, wake: &Listener) -> Result {
    let waitset = WaitSetBuilder::new().signal_handling_mode(SignalHandlingMode::Disabled).create::<Svc>()?;
    let _wake = waitset.attach_notification(wake)?;
    while !shared.stop.load(Ordering::Acquire) {
        let version = shared.version.load(Ordering::Acquire);
        let slots = shared.slots.lock().unwrap().clone();

        // Everything this thread waits on, and which node each wake-up belongs to.
        let listeners: Vec<(usize, Arc<Listener>)> = slots
            .iter()
            .enumerate()
            .flat_map(|(index, slot)| slot.listeners().into_iter().map(move |listener| (index, listener)))
            .collect();
        let mut reasons = HashMap::new();
        let (mut signals, mut timers) = (Vec::new(), Vec::new());
        for (position, (index, listener)) in listeners.iter().enumerate() {
            let guard = waitset.attach_notification(listener.as_ref())?;
            reasons.insert(WaitSetAttachmentId::from_guard(&guard), Wake::Input(*index, position));
            signals.push(guard);
        }
        for (index, slot) in slots.iter().enumerate() {
            if let Some(period) = slot.trigger().period() {
                let guard = waitset.attach_interval(period)?;
                reasons.insert(WaitSetAttachmentId::from_guard(&guard), Wake::Tick(index));
                timers.push(guard);
            }
        }
        shared.applied.store(version, Ordering::Release);

        // A resumed or newly connected node may already have data waiting.
        for slot in slots.iter().filter(|slot| slot.trigger().on_input()) {
            slot.input_arrived();
            shared.run(slot);
        }

        let mut woken = Vec::new();
        let mut ready = vec![false; slots.len()];
        while shared.version.load(Ordering::Acquire) == version && !shared.stop.load(Ordering::Acquire) {
            waitset.wait_and_process_once(|id| {
                woken.extend(reasons.get(&id).copied());
                CallbackProgression::Continue
            })?;
            wake.try_wait(|_| {})?;
            for reason in woken.drain(..) {
                match reason {
                    Wake::Input(index, position) => {
                        // Clear the signal before running: data arriving during the run signals again.
                        listeners[position].1.try_wait(|_| {})?;
                        slots[index].input_arrived();
                        ready[index] = true;
                    }
                    Wake::Tick(index) => {
                        slots[index].period_elapsed();
                        ready[index] = true;
                    }
                }
            }
            for (slot, ready) in slots.iter().zip(&mut ready) {
                if std::mem::take(ready) {
                    shared.run(slot);
                }
            }
        }
    }
    Ok(())
}
