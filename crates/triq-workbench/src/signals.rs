//! The numbers inside messages, over time.
//!
//! The workbench is built without the message types of the programs it looks
//! at. A topic that is watched is read as bytes, on a thread of its own, and
//! the numbers its type lists are taken out of every message. The runtime
//! that owns the output says which numbers those are and where they lie.
//!
//! Reading a topic makes the workbench one more of its readers, so only what
//! somebody looks at is read: a topic opened in the list, or with a number on
//! a chart. The past of a number is kept from when it is put on a chart.
use crate::history::{History, between};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use triq::{Field, Remote, Stamp};
use triq_tap::{Tap, Taps, Topic};

/// How often the topics that are watched are read. A reader's queue holds 32
/// messages, or 2 slices, so nothing is missed below a few thousand messages
/// a second, or a few hundred slices.
const EVERY: Duration = Duration::from_millis(2);
/// How many messages are taken from one topic before the next has its turn.
const AT_ONCE: usize = 64;
/// How often the topics of the machine are listed again.
const LISTED: Duration = Duration::from_secs(1);
/// How many points of one number are kept: 16 MB of them.
const MOST: usize = 1_000_000;

/// The topics to read, and of each the numbers whose past a chart wants.
pub type Wanted = BTreeMap<String, BTreeSet<String>>;

pub struct Signals {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    wanted: Wanted,
    /// Every publish-subscribe topic on the machine.
    topics: Vec<Topic>,
    /// What is being read, by topic.
    read: BTreeMap<String, (Watched, Lines)>,
}

/// The past of the numbers a chart wants, by name: when each message was sent, and the value.
type Lines = BTreeMap<String, VecDeque<(f64, f64)>>;

/// A topic that is being read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Watched {
    /// Why it cannot be read, if it cannot.
    pub problem: Option<String>,
    /// Its numbers by name, each as it stood in the newest message.
    pub numbers: Vec<(String, Option<f64>)>,
}

impl Signals {
    /// Start the thread that lists the machine's topics and reads the ones that are watched.
    pub fn start() -> triq::Result<Self> {
        let (taps, shared) = (Taps::new()?, Arc::new(Shared::default()));
        thread::Builder::new().name("workbench-taps".into()).spawn({
            let shared = shared.clone();
            move || read(&taps, &shared)
        })?;
        Ok(Self { shared })
    }

    /// Say what to read from now on. What is left out is no longer read, and its past is let go.
    pub fn watch(&self, wanted: Wanted) {
        let mut state = self.shared.state.lock().unwrap();
        if state.wanted != wanted {
            state.wanted = wanted;
        }
    }

    /// Every publish-subscribe topic on the machine, sorted by name, as of a second ago at most.
    pub fn topics(&self) -> Vec<Topic> {
        self.shared.state.lock().unwrap().topics.clone()
    }

    /// The numbers of a topic that is being read, as they stand.
    pub fn watched(&self, topic: &str) -> Option<Watched> {
        Some(self.shared.state.lock().unwrap().read.get(topic)?.0.clone())
    }

    /// One number of a topic as it stood in the newest message.
    pub fn latest(&self, topic: &str, number: &str) -> Option<f64> {
        let state = self.shared.state.lock().unwrap();
        let numbers = &state.read.get(topic)?.0.numbers;
        numbers.iter().find(|(name, _)| name == number)?.1
    }

    /// One number of a topic between two times, as a chart draws it: pairs of time and value, oldest first.
    pub fn line(&self, topic: &str, number: &str, since: f64, until: f64) -> Vec<(f64, f64)> {
        let state = self.shared.state.lock().unwrap();
        let line = state.read.get(topic).and_then(|(_, lines)| lines.get(number));
        line.map_or(Vec::new(), |line| between(line, |point| *point, since, until))
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

/// A topic as it is read: its tap, and where in a message its numbers are.
struct Open {
    /// `None` if the topic could not be opened; `problem` says why.
    tap: Option<Tap>,
    problem: Option<String>,
    numbers: Vec<(String, Place)>,
    /// Whether triq's stamp is beside every message, with the time it was sent.
    stamped: bool,
}

/// Where a number is taken from.
enum Place {
    Message(Field),
    Beside(Field),
    /// How many elements a slice has, given the size of one.
    Length(usize),
}

impl Open {
    fn new(taps: &Taps, name: &str) -> Self {
        let tap = match taps.open(name) {
            Ok(tap) => tap,
            Err(problem) => {
                return Self { tap: None, problem: Some(problem.to_string()), numbers: Vec::new(), stamped: false };
            }
        };
        // A triq output is called `runtime/node/port`, and its runtime knows what its messages hold.
        let layout = match name.split('/').collect::<Vec<_>>()[..] {
            [runtime, node, port] => Remote::open(runtime).and_then(|remote| remote.layout(node, port)).ok(),
            _ => None,
        };
        let (slice, size, stamped) = (tap.topic().slice, tap.topic().size, layout.is_some());
        let layout = layout.unwrap_or_default();
        let mut numbers = Vec::new();
        if slice {
            numbers.push(("length".to_owned(), Place::Length(size.max(1))));
        }
        for field in layout.payload {
            // A slice lists the numbers of its first element.
            let name = match (slice, field.name.as_str()) {
                (false, name) => name.to_owned(),
                (true, "value") => "[0]".to_owned(),
                (true, name) => format!("[0].{name}"),
            };
            numbers.push((name, Place::Message(field)));
        }
        for field in layout.header {
            numbers.push((format!("header.{}", field.name), Place::Beside(field)));
        }
        Self { tap: Some(tap), problem: None, numbers, stamped }
    }

    /// The oldest message that is waiting: when it was sent, and its numbers.
    fn take(&self) -> Option<(f64, Vec<Option<f64>>)> {
        let read = |beside: &[u8], message: &[u8]| {
            let sent = Stamp::read(beside).filter(|_| self.stamped).map(|stamp| stamp.sent_ns as f64 / 1e9);
            let value = |place: &Place| match place {
                Place::Message(field) => field.read(message),
                Place::Beside(field) => field.read(beside),
                Place::Length(size) => Some((message.len() / size) as f64),
            };
            // A topic that is not triq's has no stamp: its messages are placed when they are read.
            (sent.unwrap_or_else(now), self.numbers.iter().map(|(_, place)| value(place)).collect())
        };
        self.tap.as_ref()?.take(read).ok().flatten()
    }

    /// As the window is told of it before any message came.
    fn watched(&self) -> Watched {
        let numbers = self.numbers.iter().map(|(name, _)| (name.clone(), None)).collect();
        Watched { problem: self.problem.clone(), numbers }
    }
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
}

/// The body of the thread: read what is wanted until the workbench closes.
fn read(taps: &Taps, shared: &Shared) {
    let mut open: BTreeMap<String, Open> = BTreeMap::new();
    let mut listed = None::<Instant>;
    while !shared.stop.load(Ordering::Acquire) {
        if listed.is_none_or(|at| at.elapsed() >= LISTED) {
            if let Ok(topics) = taps.topics() {
                shared.state.lock().unwrap().topics = topics;
            }
            listed = Some(Instant::now());
        }
        // Stop reading what is no longer wanted, and start on what is new.
        let wanted = shared.state.lock().unwrap().wanted.clone();
        open.retain(|topic, _| wanted.contains_key(topic));
        for topic in wanted.keys().filter(|topic| !open.contains_key(*topic)).collect::<Vec<_>>() {
            open.insert(topic.clone(), Open::new(taps, topic));
        }
        // Take what is waiting, without holding up the window, and then hand it over.
        let waiting = |open: &Open| std::iter::from_fn(|| open.take()).take(AT_ONCE).collect::<Vec<_>>();
        let taken: Vec<_> = open.values().map(waiting).collect();
        let mut state = shared.state.lock().unwrap();
        state.read.retain(|topic, _| open.contains_key(topic));
        for ((topic, open), messages) in open.iter().zip(taken) {
            let (watched, lines) = state.read.entry(topic.clone()).or_insert_with(|| (open.watched(), Lines::new()));
            // Only the numbers that are on a chart have a past.
            lines.retain(|number, _| wanted[topic].contains(number));
            for (sent, values) in messages {
                for ((name, latest), value) in watched.numbers.iter_mut().zip(values) {
                    *latest = value;
                    if let (Some(value), true) = (value, wanted[topic].contains(name)) {
                        let line = lines.entry(name.clone()).or_default();
                        line.push_back((sent, value));
                        while line.len() > MOST || line.front().is_some_and(|old| old.0 < sent - History::LONGEST) {
                            line.pop_front();
                        }
                    }
                }
            }
        }
        drop(state);
        thread::sleep(EVERY);
    }
}
