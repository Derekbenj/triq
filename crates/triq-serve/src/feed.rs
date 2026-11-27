//! What the page knows of the machine, and the one thing it asks of a runtime, both off the
//! thread that answers the browser.
//!
//! One thread looks at the machine: whenever a runtime says it changed, and twice a second
//! besides, since a runtime that is killed says nothing. Another asks a runtime to shut down
//! when the page wants that, one at a time, so that a runtime slow to answer holds up neither
//! the answering nor the looking.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use triq::{Inventory, Remote, Watch};

/// The machine as it was last seen, and the way to ask a runtime of it to shut down.
pub struct Feed {
    machine: Arc<Mutex<Vec<Inventory>>>,
    /// What went wrong last: a runtime that could not be asked, or the looking itself.
    said: Arc<Mutex<Option<String>>>,
    /// The names of the runtimes to shut down. None only while it is being dropped.
    asks: Option<Sender<String>>,
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
        let (asks, asked) = mpsc::channel::<String>();
        let asking = std::thread::spawn({
            let said = said.clone();
            move || {
                for runtime in asked {
                    if let Err(error) = Remote::open(&runtime).and_then(|remote| remote.shutdown()) {
                        *said.lock().unwrap() = Some(format!("could not shut down {runtime}: {error}"));
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

    /// Ask a runtime to shut down. If it cannot be asked, that is said.
    pub fn shut_down(&self, runtime: &str) {
        if let Some(asks) = &self.asks {
            let _ = asks.send(runtime.to_owned());
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
