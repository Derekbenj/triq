//! The programs of triq that can be started from here, and those that were: each a process of
//! its own, as one started from a terminal is, with what it prints kept to be read.
//!
//! There may be several of one program at a time, each known by a number of its own. A program
//! that was started here is stopped when this one is told to quit: what it prints goes here,
//! and would have nowhere to go.
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use triq::{Inventory, Remote};

/// How many of the lines a program has printed are kept: the last of them. And how many of
/// those the page is sent each time it asks.
const KEPT: usize = 500;
const SHOWN: usize = 200;

pub struct Programs {
    /// Those that can be started, by the names they are run by.
    pub found: Vec<(String, PathBuf)>,
    /// Those that were, in the order they were started in.
    pub started: Vec<Started>,
    /// How many have been started: the next is known by the next number.
    count: u64,
}

/// A program that was started here.
pub struct Started {
    /// What it is known by for as long as this program runs: its place in the list changes.
    pub id: u64,
    pub program: String,
    /// What it was told when it was started.
    pub told: Vec<String>,
    pub pid: u32,
    child: Child,
    lines: Arc<Mutex<VecDeque<String>>>,
    /// When it was started, and for how long it ran, once it has ended.
    began: Instant,
    ran: Option<Duration>,
    /// Whether it was asked here to stop.
    stopping: bool,
    /// How it ended, once it has: in words.
    pub ended: Option<String>,
    /// Whether it ended without being asked to, saying that something was wrong.
    failed: bool,
}

/// A line as it is read on a screen: without the marks that colour it in a terminal.
pub fn plain(line: &str) -> String {
    let (mut plain, mut letters) = (String::new(), line.chars());
    while let Some(letter) = letters.next() {
        if letter != '\u{1b}' {
            plain.push(letter);
        } else if letters.next() == Some('[') {
            // A mark is the escape, a bracket, numbers and semicolons, and one letter that ends it.
            for mark in letters.by_ref() {
                if mark.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    plain
}

impl Programs {
    pub fn new(found: Vec<(String, PathBuf)>) -> Self {
        Self { found, started: Vec::new(), count: 0 }
    }

    /// The programs of triq found on this machine, but for the one that asks.
    pub fn here() -> Self {
        let own = std::env::current_exe().ok();
        Self::new(triq::programs().into_iter().filter(|(_, path)| Some(path) != own.as_ref()).collect())
    }

    /// Start the program at that place among those found, told the words of `told`: what would
    /// be typed after its name in a terminal, a space between two of them. It is known by the
    /// number returned.
    pub fn start(&mut self, program: usize, told: &str) -> std::io::Result<u64> {
        let (name, path) = self.found[program].clone();
        let told: Vec<String> = told.split_whitespace().map(str::to_owned).collect();
        let mut child = Command::new(path)
            .args(&told)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let lines = Arc::new(Mutex::new(VecDeque::new()));
        // What it prints, and what it says is wrong, are read as they come, each by a thread.
        let printed = child.stdout.take().map(|printed| Box::new(printed) as Box<dyn Read + Send>);
        let wrong = child.stderr.take().map(|wrong| Box::new(wrong) as Box<dyn Read + Send>);
        for from in [printed, wrong].into_iter().flatten() {
            let lines = lines.clone();
            std::thread::spawn(move || {
                let mut from = BufReader::new(from);
                let mut line = Vec::new();
                while from.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
                    let mut lines = lines.lock().unwrap();
                    lines.push_back(plain(String::from_utf8_lossy(&line).trim_end()));
                    if lines.len() > KEPT {
                        lines.pop_front();
                    }
                    line.clear();
                }
            });
        }
        self.count += 1;
        let id = self.count;
        let (pid, began) = (child.id(), Instant::now());
        self.started.push(Started {
            id,
            program: name,
            told,
            pid,
            child,
            lines,
            began,
            ran: None,
            stopping: false,
            ended: None,
            failed: false,
        });
        Ok(id)
    }

    /// Where one that was started is in the list, by the number it is known by.
    pub fn place(&self, id: u64) -> Option<usize> {
        self.started.iter().position(|started| started.id == id)
    }

    /// The number of the one that is that process, if one that runs is.
    pub fn of(&self, pid: u32) -> Option<u64> {
        let runs = self.started.iter().find(|started| started.pid == pid && started.ended.is_none());
        runs.map(|started| started.id)
    }

    /// The programs as the page is told of them: the names of those that can be started, and
    /// each that was: the runtime it is and how many nodes that has, for how many seconds it
    /// has run or ran, how it ended, and the last lines it printed.
    pub fn told(&self, machine: &[Inventory]) -> Value {
        let found: Vec<&String> = self.found.iter().map(|(name, _)| name).collect();
        let started = self.started.iter().enumerate().map(|(place, started)| {
            let (printed, home) = (self.printed(place), self.home(place, machine));
            json!({
                "id": started.id,
                "program": started.program,
                "told": started.told,
                "pid": started.pid,
                "runtime": home.map(|home| &home.runtime),
                "nodes": home.map(|home| home.nodes.len()),
                "seconds": started.ran.unwrap_or_else(|| started.began.elapsed()).as_secs(),
                "stopping": started.stopping && started.ended.is_none(),
                "ended": started.ended,
                "failed": started.failed,
                "printed": printed[printed.len().saturating_sub(SHOWN)..],
            })
        });
        json!({ "found": found, "started": started.collect::<Vec<_>>() })
    }

    /// Notice those that have ended since this was last asked.
    pub fn poll(&mut self) {
        for started in self.started.iter_mut().filter(|started| started.ended.is_none()) {
            let (failed, how) = match started.child.try_wait() {
                Ok(None) => continue,
                Ok(Some(_)) if started.stopping => (false, "stopped".to_owned()),
                Ok(Some(status)) if status.success() => (false, "ended".to_owned()),
                // One that a signal ended has no status of its own.
                Ok(Some(status)) => {
                    (true, status.code().map_or("was ended".to_owned(), |code| format!("ended with status {code}")))
                }
                Err(error) => (true, format!("is lost: {error}")),
            };
            (started.failed, started.ended, started.ran) = (failed, Some(how), Some(started.began.elapsed()));
        }
    }

    /// The last lines one that was started has printed, the newest last.
    pub fn printed(&self, started: usize) -> Vec<String> {
        self.started[started].lines.lock().unwrap().iter().cloned().collect()
    }

    /// The runtime one that was started is, if it runs and is one: the runtime of its process.
    fn home<'a>(&self, started: usize, machine: &'a [Inventory]) -> Option<&'a Inventory> {
        let started = &self.started[started];
        machine.iter().find(|inventory| inventory.pid == started.pid && started.ended.is_none())
    }

    /// Stop one that runs. A program that is a runtime is asked to shut down, so that it clears
    /// its own things away: its runtime's name is returned, for that to be asked of it. One that
    /// is no runtime is ended, and so is one that was asked before and runs on.
    pub fn stop(&mut self, started: usize, machine: &[Inventory]) -> Option<String> {
        let runtime = self.home(started, machine).map(|home| home.runtime.clone());
        let started = &mut self.started[started];
        if started.ended.is_some() {
            return None;
        }
        let asked_before = std::mem::replace(&mut started.stopping, true);
        if runtime.is_some() && !asked_before {
            return runtime;
        }
        let _ = started.child.kill();
        None
    }

    /// Forget one that has ended, by its number; or with no number, every one that has.
    pub fn forget(&mut self, id: Option<u64>) {
        self.started.retain(|started| started.ended.is_none() || id.is_some_and(|id| id != started.id));
    }

    /// Stop every one that runs, as the app closes: each runtime is asked to shut down and given
    /// two seconds to, and what still runs then is ended.
    pub fn close(&mut self, machine: &[Inventory]) {
        for started in 0..self.started.len() {
            if let Some(runtime) = self.stop(started, machine) {
                let _ = Remote::open(&runtime).and_then(|remote| remote.shutdown());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && self.started.iter().any(|started| started.ended.is_none()) {
            std::thread::sleep(Duration::from_millis(20));
            self.poll();
        }
        for started in self.started.iter_mut().filter(|started| started.ended.is_none()) {
            let _ = started.child.kill();
            let _ = started.child.wait();
        }
        self.poll();
    }
}
