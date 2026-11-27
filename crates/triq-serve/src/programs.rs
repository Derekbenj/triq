//! The programs of triq that can be started from here, and those that were: each a process of
//! its own, as one started from a terminal is, with what it prints kept to be read.
//!
//! There may be several of one program at a time, each known by a number of its own. A program
//! that was started here is stopped when this one is told to quit: what it prints goes here,
//! and would have nowhere to go.
//!
//! A command of the `triq` tool that is typed on the page is run the same way, the tool being
//! the program: a process that prints and ends. Those are listed apart, and only the last few.
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use triq::{Inventory, Remote};

/// How many of the lines a program has printed are kept: the last of them. And how many of
/// those the page is sent each time it asks.
const KEPT: usize = 500;
const SHOWN: usize = 200;
/// How many commands of the tool are kept, with what they printed.
const ASKED: usize = 20;
/// How often the programs that can be started are looked for again.
const LOOK: Duration = Duration::from_secs(3);

pub struct Programs {
    /// Those that can be started, by the names they are run by.
    pub found: Vec<(String, PathBuf)>,
    /// Those that were, in the order they were started in.
    pub started: Vec<Started>,
    /// How many have been started: the next is known by the next number.
    count: u64,
    /// When the programs were last looked for, if they are looked for again from time to time.
    looked: Option<Instant>,
}

/// What became of a line that was typed: a program was started, or the tool was run with it.
/// Each with the number it is known by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    Started(u64),
    Asked(u64),
}

/// A program that was started here.
pub struct Started {
    /// What it is known by for as long as this program runs: its place in the list changes.
    pub id: u64,
    pub program: String,
    /// What it was told when it was started.
    pub told: Vec<String>,
    pub pid: u32,
    /// Whether it is the tool, run with a command that was typed.
    asked: bool,
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

/// What was typed, split into words as a terminal splits it: at spaces, but that what is
/// between two quotes of a kind is in one word, spaces and all, with the quotes left out, and a
/// backslash outside quotes takes the letter after it as it is.
pub fn words(line: &str) -> Vec<String> {
    let (mut words, mut word, mut begun, mut within) = (Vec::new(), String::new(), false, None);
    let mut letters = line.chars();
    while let Some(letter) = letters.next() {
        match (within, letter) {
            (Some(quote), _) if letter == quote => within = None,
            (Some(_), _) => word.push(letter),
            (None, '\'' | '"') => (within, begun) = (Some(letter), true),
            (None, '\\') => {
                word.extend(letters.next());
                begun = true;
            }
            (None, _) if letter.is_whitespace() => {
                if std::mem::take(&mut begun) {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, _) => {
                word.push(letter);
                begun = true;
            }
        }
    }
    if begun {
        words.push(word);
    }
    words
}

/// The programs of triq found on this machine, but for the one that asks.
fn found() -> Vec<(String, PathBuf)> {
    let own = std::env::current_exe().ok();
    triq::programs().into_iter().filter(|(_, path)| Some(path) != own.as_ref()).collect()
}

impl Programs {
    /// Those programs, and no others for as long as this is kept.
    pub fn new(found: Vec<(String, PathBuf)>) -> Self {
        Self { found, started: Vec::new(), count: 0, looked: None }
    }

    /// The programs of triq found on this machine. They are looked for again from time to
    /// time, so that one that is built or registered later is found without starting over.
    pub fn here() -> Self {
        Self { looked: Some(Instant::now()), ..Self::new(found()) }
    }

    /// Start the program at that place among those found, told the words of `told`: what would
    /// be typed after its name in a terminal. It is known by the number returned.
    pub fn start(&mut self, program: usize, told: &str) -> std::io::Result<u64> {
        let (name, path) = self.found[program].clone();
        self.run(name, &path, words(told), false)
    }

    /// Run the tool with a command that was typed: `told` is what would be typed after the
    /// tool's name. What it prints is kept like a program's, under the number returned.
    pub fn ask(&mut self, tool: &Path, told: &str) -> std::io::Result<u64> {
        self.run("triq".to_owned(), tool, words(told), true)
    }

    /// Take a line as it is typed in a terminal, with the tool's name in front or without. The
    /// name of a program starts that program, told the rest, as the tool would hand it over;
    /// anything else is for the tool. None when nothing was typed.
    pub fn typed(&mut self, tool: &Path, line: &str) -> std::io::Result<Option<Typed>> {
        let mut typed = words(line);
        if typed.first().is_some_and(|first| first == "triq") {
            typed.remove(0);
        }
        let Some(first) = typed.first() else { return Ok(None) };
        match self.found.iter().find(|(name, _)| name == first).cloned() {
            Some((name, path)) => self.run(name, &path, typed[1..].to_vec(), false).map(|id| Some(Typed::Started(id))),
            None => self.run("triq".to_owned(), tool, typed, true).map(|id| Some(Typed::Asked(id))),
        }
    }

    /// Run a program as a process of its own, told those words, and keep what it prints.
    fn run(&mut self, name: String, path: &Path, told: Vec<String>, asked: bool) -> std::io::Result<u64> {
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
        // Of the commands that have ended, only the last are kept.
        let commands = |started: &[Started]| started.iter().filter(|started| started.asked).count();
        while asked && commands(&self.started) >= ASKED {
            let Some(oldest) = self.started.iter().position(|started| started.asked && started.ended.is_some()) else {
                break;
            };
            self.started.remove(oldest);
        }
        self.count += 1;
        let id = self.count;
        let (pid, began) = (child.id(), Instant::now());
        self.started.push(Started {
            id,
            program: name,
            told,
            pid,
            asked,
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
    /// has run or ran, how it ended, and the last lines it printed. The commands of the tool
    /// that were typed are told the same way, apart, in `asked`.
    pub fn told(&self, machine: &[Inventory]) -> Value {
        let found: Vec<&String> = self.found.iter().map(|(name, _)| name).collect();
        let told = |(place, started): (usize, &Started)| {
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
        };
        let of = |asked: bool| -> Vec<Value> {
            self.started.iter().enumerate().filter(|(_, started)| started.asked == asked).map(told).collect()
        };
        json!({ "found": found, "started": of(false), "asked": of(true) })
    }

    /// Notice those that have ended since this was last asked, and look for the programs again
    /// if that was not done for a while.
    pub fn poll(&mut self) {
        if self.looked.is_some_and(|at| at.elapsed() > LOOK) {
            (self.found, self.looked) = (found(), Some(Instant::now()));
        }
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

    /// Forget one that has ended, by its number; or with no number, every program that has.
    pub fn forget(&mut self, id: Option<u64>) {
        let stays = |started: &Started| id.map_or(started.asked, |id| id != started.id);
        self.started.retain(|started| started.ended.is_none() || stays(started));
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
