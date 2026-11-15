//! The optional dispatcher behind a shared pool.
//!
//! An ordinary executor runs a ready node itself. With workers, it hands the
//! node to whichever worker thread is free and goes straight back to waiting,
//! so a node that blocks holds one worker and delays nobody else. The price is
//! one more thread wake-up per run. Nothing else in triq depends on this file.
use crate::node::Slot;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

pub(crate) struct Workers {
    /// `None` only while shutting down.
    ready: Option<mpsc::Sender<Arc<Slot>>>,
    threads: Vec<JoinHandle<()>>,
    running: Arc<AtomicUsize>,
}

impl Workers {
    pub fn start(pool: &str, count: usize) -> std::io::Result<Self> {
        let (ready, queue) = mpsc::channel::<Arc<Slot>>();
        let queue = Arc::new(Mutex::new(queue));
        let running = Arc::new(AtomicUsize::new(count));
        let threads = (0..count)
            .map(|index| {
                let (queue, running) = (queue.clone(), running.clone());
                thread::Builder::new().name(format!("triq-{pool}-{index}")).spawn(move || {
                    // One idle worker holds the lock and waits; the others queue up behind it.
                    while let Ok(slot) = { queue.lock().unwrap().recv() } {
                        slot.run_claimed();
                    }
                    running.fetch_sub(1, Ordering::Release);
                })
            })
            .collect::<std::io::Result<_>>()?;
        Ok(Self { ready: Some(ready), threads, running })
    }

    /// Have a worker run the node, unless one already is; then it runs once more afterwards.
    pub fn submit(&self, slot: &Arc<Slot>) {
        if slot.claim()
            && let Some(ready) = &self.ready
        {
            let _ = ready.send(slot.clone());
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        // Closing the queue ends the workers once they are idle.
        self.ready = None;
        if super::wait_until(|| self.running.load(Ordering::Acquire) == 0) {
            self.threads.drain(..).for_each(|thread| _ = thread.join());
        }
    }
}
