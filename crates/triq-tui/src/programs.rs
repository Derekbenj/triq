//! The programs of triq that can be started from here, and those that were: each a process of
//! its own, as one started from a terminal is, with what it prints kept to be read.
//!
//! A program that was started here is stopped when the app closes: what it prints goes to the
//! app, and would have nowhere to go.
use crate::feed::{Ask, perform};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use triq::Inventory;

/// How many of the lines a program has printed are kept: the last of them.
const KEPT: usize = 500;

pub struct Programs {
    /// Those that can be started, by the names they are run by.
    pub found: Vec<(String, PathBuf)>,
    /// Those that were, in the order they were started in.
    pub started: Vec<Started>,
}

/// A program that was started here.
pub struct Started {
    pub program: String,
    /// What it was told when it was started.
    pub told: Vec<String>,
    pub pid: u32,
    child: Child,
    lines: Arc<Mutex<VecDeque<String>>>,
    /// How it ended, once it has.
    pub ended: Option<String>,
}

/// A row of the list of programs: one that can be started, or under it one that was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listed {
    /// Its place among those found.
    Program(usize),
    /// Its place among those started.
    Started(usize),
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
        Self { found, started: Vec::new() }
    }

    /// The programs of triq found on this machine, but for the one that asks.
    pub fn here() -> Self {
        let own = std::env::current_exe().ok();
        Self::new(triq::programs().into_iter().filter(|(_, path)| Some(path) != own.as_ref()).collect())
    }

    /// The list as it is shown: each program, and under it those of it that were started.
    pub fn listed(&self) -> Vec<Listed> {
        let mut listed = Vec::new();
        for (index, (name, _)) in self.found.iter().enumerate() {
            listed.push(Listed::Program(index));
            let started = self.started.iter().enumerate().filter(|(_, started)| started.program == *name);
            listed.extend(started.map(|(index, _)| Listed::Started(index)));
        }
        listed
    }

    /// Start the program at that place among those found, told the words of `told`: what would
    /// be typed after its name in a terminal, a space between two of them.
    pub fn start(&mut self, program: usize, told: &str) -> std::io::Result<()> {
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
        self.started.push(Started { program: name, told, pid: child.id(), child, lines, ended: None });
        Ok(())
    }

    /// Notice those that have ended since this was last asked.
    pub fn poll(&mut self) {
        for started in self.started.iter_mut().filter(|started| started.ended.is_none()) {
            started.ended = match started.child.try_wait() {
                Ok(None) => None,
                Ok(Some(status)) if status.success() => Some("ended".to_owned()),
                Ok(Some(status)) => {
                    Some(status.code().map_or("was ended".to_owned(), |code| format!("ended with status {code}")))
                }
                Err(error) => Some(format!("is lost: {error}")),
            };
        }
    }

    /// The last lines one that was started has printed, the newest last.
    pub fn printed(&self, started: usize) -> Vec<String> {
        self.started[started].lines.lock().unwrap().iter().cloned().collect()
    }

    /// The runtime one that was started is, if it runs and is one: the runtime of its process.
    pub fn runtime<'a>(&self, started: usize, machine: &'a [Inventory]) -> Option<&'a str> {
        let started = &self.started[started];
        let runtime = machine.iter().find(|inventory| inventory.pid == started.pid && started.ended.is_none());
        runtime.map(|inventory| inventory.runtime.as_str())
    }

    /// Stop one that runs. A program that is a runtime is asked to shut down, which is what is
    /// returned to be asked of it: so it clears its own things away. One that is not is ended.
    pub fn stop(&mut self, started: usize, machine: &[Inventory]) -> Option<Ask> {
        if self.started[started].ended.is_some() {
            return None;
        }
        if let Some(runtime) = self.runtime(started, machine) {
            return Some(Ask::Shutdown { runtime: runtime.to_owned() });
        }
        let _ = self.started[started].child.kill();
        None
    }

    /// Forget those that have ended.
    pub fn forget_ended(&mut self) {
        self.started.retain(|started| started.ended.is_none());
    }

    /// Stop every one that runs, as the app closes: each runtime is asked to shut down and given
    /// two seconds to, and what still runs then is ended.
    pub fn close(&mut self, machine: &[Inventory]) {
        for started in 0..self.started.len() {
            if let Some(ask) = self.stop(started, machine) {
                let _ = perform(&ask);
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
    }
}
