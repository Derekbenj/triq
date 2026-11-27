//! The programs of triq that can be started from here, and those that were: each a process of
//! its own, as one started from a terminal is, with what it prints kept to be read.
//!
//! There may be several of one program at a time, each known by a number of its own. A program
//! that is started here does not depend on this one. It is in a group of processes of its own,
//! so that what ends this program at its terminal is not sent to it, and it prints into a file
//! and not to this program. Of each there is a record on disk: see [`Records`]. So a program
//! may be left running when this one ends, and the next triq-serve finds it by its record,
//! with what it has printed, and may stop it though it is not its child.
//!
//! A command of the `triq` tool that is typed on the page is run by the tool, as a process
//! that prints to this program and ends. Those are listed apart, and only the last few.
use crate::records::{Record, Records, make};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use triq::{Inventory, Remote};

/// How many of the lines a program has printed are kept: the last of them. And how many of
/// those the page is sent each time it asks.
const KEPT: usize = 500;
const SHOWN: usize = 200;
/// How many commands of the tool are kept, with what they printed.
const ASKED: usize = 20;
/// How often the programs that can be started are looked for again.
const LOOK: Duration = Duration::from_secs(3);
/// How often a program that is not this one's child is looked at, to see whether it still runs.
const CHECK: Duration = Duration::from_secs(2);

/// The lines a program has printed, the newest last, as the thread that reads them leaves them.
type Lines = Arc<Mutex<VecDeque<String>>>;

pub struct Programs {
    /// Those that can be started, by the names they are run by.
    pub found: Vec<(String, PathBuf)>,
    /// Those that were, in the order they were started in.
    pub started: Vec<Started>,
    /// What is kept on disk of those that were started.
    pub records: Records,
    /// How many have been started: the next is known by the next number.
    count: u64,
    /// When the programs were last looked for, if they are looked for again from time to time.
    looked: Option<Instant>,
    /// When those that are not this program's children were last looked at.
    checked: Instant,
}

/// What became of a line that was typed: a program was started, or the tool was run with it.
/// Each with the number it is known by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    Started(u64),
    Asked(u64),
}

/// A program that was started, here or by a triq-serve before this one.
pub struct Started {
    /// What it is known by, here and on its record.
    pub id: u64,
    pub program: String,
    /// What it was told when it was started.
    pub told: Vec<String>,
    pub pid: u32,
    /// Whether it is the tool, run with a command that was typed.
    asked: bool,
    /// The process, if this program started it. One that an earlier triq-serve left running is
    /// not this one's child, and is known by its number among the processes alone.
    child: Option<Child>,
    lines: Lines,
    /// Set once it has ended, for the thread that reads what it printed: no more is to come.
    over: Arc<AtomicBool>,
    /// When it was started, and for how long it ran, once it has ended.
    began: SystemTime,
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
/// backslash outside quotes takes the letter after it as it is. A word that begins with a bare
/// `~` is in the person's own folder.
pub fn words(line: &str) -> Vec<String> {
    let (mut words, mut word, mut within) = (Vec::new(), String::new(), None);
    // Whether a word has begun, and whether it began with a letter that was neither in quotes
    // nor after a backslash.
    let (mut begun, mut bare) = (false, false);
    let home = |word: String, bare: bool| if bare { at_home(&word) } else { word };
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
                    words.push(home(std::mem::take(&mut word), std::mem::take(&mut bare)));
                }
            }
            (None, _) => {
                bare |= !begun;
                word.push(letter);
                begun = true;
            }
        }
    }
    if begun {
        words.push(home(word, bare));
    }
    words
}

/// A place as a terminal has it: `~`, alone or before a `/`, is the person's own folder.
pub fn at_home(place: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if place == "~" || place.starts_with("~/") => format!("{home}{}", &place[1..]),
        _ => place.to_owned(),
    }
}

/// The programs of triq found on this machine, but for the one that asks.
fn found() -> Vec<(String, PathBuf)> {
    let own = std::env::current_exe().ok();
    triq::programs().into_iter().filter(|(_, path)| Some(path) != own.as_ref()).collect()
}

/// Whether a process of that number is there.
fn alive(pid: u32) -> bool {
    signal(pid, "-0")
}

/// Send a process a signal, as `kill` does. False if it could not be sent: there is no such
/// process, or it is not this person's.
fn signal(pid: u32, which: &str) -> bool {
    let mut sent = Command::new("kill");
    sent.args([which, &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    cfg!(unix) && sent.status().is_ok_and(|done| done.success())
}

/// Whether the program a record is of still runs: a process of its number is there, and is that
/// program. A number is given to another process once the one that had it has ended.
fn runs(record: &Record) -> bool {
    let asked = Command::new("ps").args(["-ww", "-p", &record.pid.to_string(), "-o", "command="]).output();
    let (Ok(said), Some(name)) = (asked, record.path.file_name()) else { return false };
    alive(record.pid) && String::from_utf8_lossy(&said.stdout).contains(&*name.to_string_lossy())
}

/// What went wrong with a file, with the file's name before it: an error of the system's
/// names none.
fn about(file: &Path) -> impl Fn(std::io::Error) -> std::io::Error + '_ {
    move |error| std::io::Error::new(error.kind(), format!("{}: {error}", file.display()))
}

/// Put a line a program printed with those kept of it, and let the oldest go.
fn keep(lines: &Lines, line: &[u8]) {
    let mut lines = lines.lock().unwrap();
    lines.push_back(plain(String::from_utf8_lossy(line).trim_end()));
    if lines.len() > KEPT {
        lines.pop_front();
    }
}

/// Read what a program prints into a file, from the beginning and on as the file grows, until
/// the program is `over` and nothing more is in the file.
fn follow(log: PathBuf, lines: Lines, over: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(file) = File::open(log) else { return };
        let (mut from, mut line) = (BufReader::new(file), Vec::new());
        loop {
            // Asked before the file is read: what it printed last is in the file by then.
            let last = over.load(Ordering::Acquire);
            match from.read_until(b'\n', &mut line) {
                Ok(0) if last => break,
                Ok(0) => std::thread::sleep(Duration::from_millis(100)),
                // A line that is not whole yet is read on when the rest of it is there.
                Ok(_) if line.ends_with(b"\n") => {
                    keep(&lines, &line);
                    line.clear();
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if !line.is_empty() {
            keep(&lines, &line);
        }
    });
}

impl Programs {
    /// Those programs, and no others for as long as this is kept, with their records there.
    pub fn new(found: Vec<(String, PathBuf)>, records: Records) -> Self {
        Self { found, started: Vec::new(), records, count: 0, looked: None, checked: Instant::now() }
    }

    /// The programs of triq found on this machine, and those that an earlier triq-serve
    /// started and left a record of. The programs are looked for again from time to time, so
    /// that one that is built or registered later is found without starting over.
    pub fn here(records: Records) -> Self {
        let mut here = Self { looked: Some(Instant::now()), ..Self::new(found(), records) };
        here.adopt();
        here
    }

    /// Take up the programs there is a record of, and that are not known here yet: one that
    /// still runs as one that was started, and one that is gone as one that has ended.
    pub fn adopt(&mut self) {
        for record in self.records.all() {
            if self.started.iter().any(|started| started.id == record.id && !started.asked) {
                continue;
            }
            let (runs, began) = (runs(&record), UNIX_EPOCH + Duration::from_secs(record.began));
            // One that is gone ran for about as long as it printed.
            let printed = std::fs::metadata(self.records.log(record.id)).and_then(|log| log.modified());
            let ran = printed.ok().and_then(|last| last.duration_since(began).ok()).unwrap_or_default();
            let (lines, over) = (Lines::default(), Arc::new(AtomicBool::new(!runs)));
            follow(self.records.log(record.id), lines.clone(), over.clone());
            self.count = self.count.max(record.id);
            self.started.push(Started {
                id: record.id,
                program: record.program,
                told: record.told,
                pid: record.pid,
                asked: false,
                child: None,
                lines,
                over,
                began,
                ran: (!runs).then_some(ran),
                stopping: false,
                ended: (!runs).then(|| "ended".to_owned()),
                failed: false,
            });
        }
    }

    /// Start the program at that place among those found, told the words of `told`: what would
    /// be typed after its name in a terminal. It is known by the number returned.
    pub fn start(&mut self, program: usize, told: &str) -> std::io::Result<u64> {
        let (name, path) = self.found[program].clone();
        self.begin(name, &path, words(told))
    }

    /// Run the tool with a command that was typed: `told` is what would be typed after the
    /// tool's name. What it prints is kept like a program's, under the number returned.
    pub fn ask(&mut self, tool: &Path, told: &str) -> std::io::Result<u64> {
        self.run(tool, words(told))
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
            Some((name, path)) => self.begin(name, &path, typed[1..].to_vec()).map(|id| Some(Typed::Started(id))),
            None => self.run(tool, typed).map(|id| Some(Typed::Asked(id))),
        }
    }

    /// A number that nothing was started under, here or as far as the records say.
    fn number(&mut self) -> u64 {
        self.count = self.count.max(self.records.free() - 1) + 1;
        self.count
    }

    /// Start a program as a process that does not depend on this one, told those words: in a
    /// group of its own, printing into a file, and on record.
    fn begin(&mut self, name: String, path: &Path, told: Vec<String>) -> std::io::Result<u64> {
        let id = self.number();
        make(self.records.folder()).map_err(about(self.records.folder()))?;
        let printed = self.records.log(id);
        let log = File::options().create(true).append(true).open(&printed).map_err(about(&printed))?;
        let mut program = Command::new(path);
        program.args(&told).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
        // What ends this program at its terminal, Ctrl-C or the window closed, is sent to the
        // group it is in. The program that is started is in one of its own.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut program, 0);
        let mut child = program.spawn().map_err(about(path))?;
        let began = SystemTime::now();
        let since = began.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let record = Record {
            id,
            program: name.clone(),
            path: path.to_owned(),
            told: told.clone(),
            pid: child.id(),
            began: since,
        };
        if let Err(error) = self.records.keep(&record) {
            // One that cannot be put on record would be lost to the next: it is not started.
            let _ = child.kill();
            let _ = child.wait();
            self.records.forget(id);
            return Err(about(self.records.folder())(error));
        }
        let (lines, over) = (Lines::default(), Arc::new(AtomicBool::new(false)));
        follow(self.records.log(id), lines.clone(), over.clone());
        self.started.push(Started {
            id,
            program: name,
            told,
            pid: child.id(),
            asked: false,
            child: Some(child),
            lines,
            over,
            began,
            ran: None,
            stopping: false,
            ended: None,
            failed: false,
        });
        Ok(id)
    }

    /// Run the tool with a command, as a process that prints to this program and ends.
    fn run(&mut self, tool: &Path, told: Vec<String>) -> std::io::Result<u64> {
        let mut asked = Command::new(tool);
        asked.args(&told).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = asked.spawn().map_err(about(tool))?;
        let lines = Lines::default();
        // What it prints, and what it says is wrong, are read as they come, each by a thread.
        let printed = child.stdout.take().map(|printed| Box::new(printed) as Box<dyn Read + Send>);
        let wrong = child.stderr.take().map(|wrong| Box::new(wrong) as Box<dyn Read + Send>);
        for from in [printed, wrong].into_iter().flatten() {
            let lines = lines.clone();
            std::thread::spawn(move || {
                let (mut from, mut line) = (BufReader::new(from), Vec::new());
                while from.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
                    keep(&lines, &line);
                    line.clear();
                }
            });
        }
        // Of the commands that have ended, only the last are kept.
        let commands = |started: &[Started]| started.iter().filter(|started| started.asked).count();
        while commands(&self.started) >= ASKED {
            let oldest = self.started.iter().position(|started| started.asked && started.ended.is_some());
            let Some(oldest) = oldest else { break };
            self.started.remove(oldest);
        }
        let id = self.number();
        self.started.push(Started {
            id,
            program: "triq".to_owned(),
            told,
            pid: child.id(),
            asked: true,
            child: Some(child),
            lines,
            over: Arc::default(),
            began: SystemTime::now(),
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
    /// has run or ran, how it ended, the last lines it printed and the file they are in, and
    /// whether a triq-serve before this one started it. The commands of the tool that were
    /// typed are told the same way, apart, in `asked`.
    pub fn told(&self, machine: &[Inventory]) -> Value {
        let found: Vec<&String> = self.found.iter().map(|(name, _)| name).collect();
        let told = |(place, started): (usize, &Started)| {
            let (printed, home) = (self.printed(place), self.home(place, machine));
            let ran = started.ran.or_else(|| started.began.elapsed().ok()).unwrap_or_default();
            json!({
                "id": started.id,
                "program": started.program,
                "told": started.told,
                "pid": started.pid,
                "runtime": home.map(|home| &home.runtime),
                "nodes": home.map(|home| home.nodes.len()),
                "seconds": ran.as_secs(),
                "stopping": started.stopping && started.ended.is_none(),
                "ended": started.ended,
                "failed": started.failed,
                "printed": printed[printed.len().saturating_sub(SHOWN)..],
                "log": (!started.asked).then(|| self.records.log(started.id)),
                "adopted": !started.asked && started.child.is_none(),
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
        // One that is not this program's child is looked at from time to time, and each time
        // while it is being stopped.
        let check = self.checked.elapsed() > CHECK;
        if check {
            self.checked = Instant::now();
        }
        let mut answered = false;
        for started in self.started.iter_mut().filter(|started| started.ended.is_none()) {
            let (stopped, ended) = (started.stopping, |how: &str| Some((false, how.to_owned())));
            let ended = match &mut started.child {
                Some(child) => match child.try_wait() {
                    Ok(None) => None,
                    Ok(Some(_)) if stopped => ended("stopped"),
                    Ok(Some(status)) if status.success() => ended("ended"),
                    // One that a signal ended has no status of its own.
                    Ok(Some(status)) => Some((
                        true,
                        status.code().map_or("was ended".to_owned(), |code| format!("ended with status {code}")),
                    )),
                    Err(error) => Some((true, format!("is lost: {error}"))),
                },
                None if (check || stopped) && !alive(started.pid) => ended(if stopped { "stopped" } else { "ended" }),
                None => None,
            };
            if let Some((failed, how)) = ended {
                (started.failed, started.ended, started.ran) = (failed, Some(how), started.began.elapsed().ok());
                started.over.store(true, Ordering::Release);
                answered |= started.asked;
            }
        }
        // A command of the tool may have registered a folder: its programs are there at once.
        if answered && self.looked.is_some() {
            (self.found, self.looked) = (found(), Some(Instant::now()));
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
        match &mut started.child {
            Some(child) => _ = child.kill(),
            // Not this program's child: it is asked to end as any process is, and is ended
            // outright if it has to be stopped again.
            None => _ = signal(started.pid, if asked_before { "-KILL" } else { "-TERM" }),
        }
        None
    }

    /// Forget one that has ended, by its number; or with no number, every program that has.
    /// Its record goes, and what it printed.
    pub fn forget(&mut self, id: Option<u64>) {
        let stays = |started: &Started| id.map_or(started.asked, |id| id != started.id);
        let (records, goes) = (&self.records, |started: &Started| started.ended.is_some() && !stays(started));
        self.started.iter().filter(|started| goes(started) && !started.asked).for_each(|gone| records.forget(gone.id));
        self.started.retain(|started| !goes(started));
    }

    /// As this program ends. With `leave`, the programs that run are left running, each with
    /// its record, for the next triq-serve to find. Without, every one is stopped: each runtime
    /// is asked to shut down and given two seconds to, and what still runs then is ended. Of
    /// one that has ended nothing is kept either way.
    pub fn close(&mut self, machine: &[Inventory], leave: bool) {
        let runs_on = |started: &Started, leave: bool| leave && started.ended.is_none();
        let stays = |started: &Started| leave && !started.asked;
        for started in 0..self.started.len() {
            if stays(&self.started[started]) {
                continue;
            }
            if let Some(runtime) = self.stop(started, machine) {
                let _ = Remote::open(&runtime).and_then(|remote| remote.shutdown());
            }
        }
        let runs = |started: &Started| started.ended.is_none() && !stays(started);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && self.started.iter().any(runs) {
            std::thread::sleep(Duration::from_millis(20));
            self.poll();
        }
        for started in self.started.iter_mut().filter(|started| runs(started)) {
            match &mut started.child {
                Some(child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                None => _ = signal(started.pid, "-KILL"),
            }
        }
        self.poll();
        // What is left running keeps its record, for the next to find it by. Nothing else does.
        for started in self.started.iter().filter(|started| !started.asked && !runs_on(started, leave)) {
            self.records.forget(started.id);
        }
    }
}
