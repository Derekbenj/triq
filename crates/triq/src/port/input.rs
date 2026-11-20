//! The receiving end of a connection: one iceoryx2 subscriber per connected output.
//!
//! A connection is between two ports that are there, and ends when either
//! goes. It is to the node that sends, not to its name: a node that later
//! takes the name is not heard unless it is connected.
use super::{NodeGuard, PortOptions};
use crate::inventory::SourceInfo;
use crate::message::Headed;
use crate::message::now_ns;
use crate::raw::{IpcNode, Listener, PubSub, RawSample, Subscriber};
use crate::{Error, Header, Payload, Result, Stamp};
use iceoryx2::identifiers::UniquePublisherId;
use iceoryx2::prelude::{CallbackProgression, NodeState, PortFactory, PortName, ServiceName};
use iceoryx2::service::builder::event::EventOpenError;
use iceoryx2::service::builder::publish_subscribe::PublishSubscribeOpenError;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Receives messages of type `T` from every output it is connected to.
///
/// Connections are made and removed while the node runs, by the node's own
/// runtime or on request from any other program. The node only ever calls
/// [`Input::recv`] or [`Input::latest`].
pub struct Input<T: Payload + ?Sized, H: Header = ()> {
    shared: Arc<Shared<T, H>>,
    _node: Arc<NodeGuard>,
}

/// What the runtime keeps of an input after handing it to its node.
pub(crate) trait InputPort: Send + Sync {
    /// Subscribe to `output`. Refused if no node is there that sends on it.
    fn connect(&self, ipc: &IpcNode, output: &str) -> Result;
    fn disconnect(&self, output: &str);
    /// Let go of the outputs that are gone: their node was removed, or their
    /// program stopped. True if any was.
    fn drop_gone(&self) -> bool;
    /// One per connected output; a thread waiting on them wakes when data arrives.
    fn listeners(&self) -> Vec<Arc<Listener>>;
    fn info(&self) -> Vec<SourceInfo>;
    /// How many samples the node has taken from this input so far.
    fn taken(&self) -> u64;
    /// Whether the node stopped taking samples here while some were left: it took one, has
    /// not since found the input empty, and a sample waits. A node that reads until nothing
    /// is left has seen that for itself, and is told of whatever comes after.
    fn left_waiting(&self) -> bool;
}

pub(crate) struct Shared<T: Payload + ?Sized, H: Header> {
    /// `node/port`: the name every subscriber of this input carries in iceoryx2.
    port: PortName,
    depth: Option<usize>,
    sources: Mutex<Sources<T, H>>,
    taken: AtomicU64,
    /// True from when the node takes a sample until it finds nothing waiting.
    unsure: AtomicBool,
}

struct Sources<T: Payload + ?Sized, H: Header> {
    list: Vec<Source<T, H>>,
    /// Where the next `recv` starts looking, so one busy output cannot starve the others.
    next: usize,
}

/// One output this input hears.
struct Source<T: Payload + ?Sized, H: Header> {
    output: Arc<str>,
    link: Link<T, H>,
    last_seq: u64,
    /// When the newest sample was sent, by its stamp. Zero before the first.
    last_sent_ns: u64,
    received: u64,
    lost: u64,
    latency_ns: u64,
    /// The time from one sample to the next, by when they were sent.
    gap_ns: Lately,
}

/// The largest value noted lately: in the current half second by the clock,
/// or in the one before. Nothing is reset when it is read, so any number of
/// tools that look at least twice a second all see every peak.
#[derive(Default)]
struct Lately {
    /// The half second that `now` is the largest of, counted from 1970.
    half: u64,
    now: u64,
    before: u64,
}

impl Lately {
    const HALF_SECOND: u64 = 500_000_000;

    fn note(&mut self, at_ns: u64, value: u64) {
        let half = at_ns / Self::HALF_SECOND;
        if half != self.half {
            // The largest of the half second before moves back; anything older is forgotten.
            self.before = if half == self.half + 1 { self.now } else { 0 };
            (self.half, self.now) = (half, 0);
        }
        self.now = self.now.max(value);
    }

    fn largest(&self, at_ns: u64) -> u64 {
        match (at_ns / Self::HALF_SECOND).saturating_sub(self.half) {
            0 => self.now.max(self.before),
            1 => self.now,
            _ => 0,
        }
    }
}

struct Link<T: Payload + ?Sized, H: Header> {
    subscriber: Subscriber<T, H>,
    listener: Arc<Listener>,
    service: Arc<PubSub<T, H>>,
    /// The sending port of the node that was there when the link was made.
    sender: UniquePublisherId,
}

/// The sending port of the node behind an output. `None` when no node is
/// there: the port was closed, or the program it was in has died. A program
/// that iceoryx2 cannot tell the state of counts as living, so that no
/// connection is let go of on a doubt.
fn sender<T: Payload + ?Sized, H: Header>(service: &PubSub<T, H>) -> Option<UniquePublisherId> {
    let mut ports = Vec::new();
    service.dynamic_config().list_publishers(|publisher| {
        ports.push((publisher.publisher_id, publisher.node_id));
        CallbackProgression::Continue
    });
    let mut dead = Vec::new();
    let _ = service.nodes(|node| {
        if matches!(node, NodeState::Dead(_)) {
            dead.push(*node.node_id());
        }
        CallbackProgression::Continue
    });
    ports.into_iter().find(|(_, owner)| !dead.contains(owner)).map(|(sender, _)| sender)
}

impl<T: Payload + ?Sized, H: Header> Input<T, H> {
    pub(crate) fn new(shared: Arc<Shared<T, H>>, node: Arc<NodeGuard>) -> Self {
        Self { shared, _node: node }
    }

    /// The next waiting sample, from whichever connected output has one.
    pub fn recv(&self) -> Result<Option<Sample<T, H>>> {
        let mut sources = self.shared.sources.lock().unwrap();
        let count = sources.list.len();
        for offset in 0..count {
            let index = (sources.next + offset) % count;
            let source = &mut sources.list[index];
            let link = &source.link;
            while let Some(raw) = T::receive(&link.subscriber)? {
                // A node that took the name of the one this input is linked to is not heard:
                // what it sends before the link is let go of is passed over.
                if raw.origin() != link.sender {
                    continue;
                }
                source.count(raw.user_header().stamp);
                let output = source.output.clone();
                sources.next = index + 1;
                self.shared.taken.fetch_add(1, Ordering::Relaxed);
                self.shared.unsure.store(true, Ordering::Relaxed);
                return Ok(Some(Sample { raw, output }));
            }
        }
        self.shared.unsure.store(false, Ordering::Relaxed);
        Ok(None)
    }

    /// The newest waiting sample; everything older is skipped.
    pub fn latest(&self) -> Result<Option<Sample<T, H>>> {
        let mut newest = None;
        while let Some(sample) = self.recv()? {
            newest = Some(sample);
        }
        Ok(newest)
    }

    /// Samples that were overwritten in the queue before this input read them.
    pub fn lost(&self) -> u64 {
        self.shared.sources.lock().unwrap().list.iter().map(|source| source.lost).sum()
    }
}

/// A received message, read in place from shared memory.
///
/// An output limits how many samples each input may hold at once (`hold`,
/// 2 by default). Copy the data out to keep it longer.
pub struct Sample<T: Payload + ?Sized, H: Header = ()> {
    raw: RawSample<T, H>,
    output: Arc<str>,
}

impl<T: Payload + ?Sized, H: Header> Sample<T, H> {
    /// Sequence number and send time, written by the output.
    pub fn stamp(&self) -> Stamp {
        self.raw.user_header().stamp
    }

    /// What the output sent beside the message, if its port has a header type.
    pub fn header(&self) -> &H {
        &self.raw.user_header().header
    }

    /// The output this sample came from: `runtime/node/port`.
    pub fn source(&self) -> &str {
        &self.output
    }

    /// Time since the sample was sent.
    pub fn age(&self) -> Duration {
        Duration::from_nanos(now_ns().saturating_sub(self.stamp().sent_ns))
    }
}

impl<T: Payload + ?Sized, H: Header> Deref for Sample<T, H> {
    type Target = T;
    fn deref(&self) -> &T {
        T::received(&self.raw)
    }
}

impl<T: Payload + ?Sized, H: Header> Source<T, H> {
    fn new(output: &str, link: Link<T, H>) -> Self {
        Self {
            output: output.into(),
            link,
            last_seq: 0,
            last_sent_ns: 0,
            received: 0,
            lost: 0,
            latency_ns: 0,
            gap_ns: Lately::default(),
        }
    }

    fn count(&mut self, stamp: Stamp) {
        let now = now_ns();
        // Losses and gaps are counted from the first sample seen. A sequence that
        // goes backwards means the output's node was restarted: start afresh.
        if self.received > 0 && stamp.seq > self.last_seq {
            self.lost += stamp.seq - self.last_seq - 1;
            self.gap_ns.note(now, stamp.sent_ns.saturating_sub(self.last_sent_ns));
        }
        self.last_seq = stamp.seq;
        self.last_sent_ns = stamp.sent_ns;
        self.received += 1;
        self.latency_ns = now.saturating_sub(stamp.sent_ns);
    }
}

impl<T: Payload + ?Sized, H: Header> Shared<T, H> {
    pub fn create(port: &str, options: &PortOptions) -> Result<Self> {
        let output_only = [
            ("queue", options.queue.is_some()),
            ("subscribers", options.subscribers.is_some()),
            ("hold", options.hold.is_some()),
            ("reliable_ms", options.reliable_ms.is_some()),
        ];
        if let Some((option, _)) = output_only.iter().find(|(_, set)| *set) {
            return Err(Error::Mismatch(format!("{port}: `{option}` belongs on an output")));
        }
        Ok(Self {
            port: PortName::new(port)?,
            depth: options.depth,
            sources: Mutex::new(Sources { list: Vec::new(), next: 0 }),
            taken: AtomicU64::new(0),
            unsure: AtomicBool::new(false),
        })
    }

    /// Open the output's services and subscribe. iceoryx2 refuses a different message type.
    /// An output that no node sends on is not there, also while others still hold its services.
    fn link(&self, ipc: &IpcNode, output: &str) -> Result<Link<T, H>> {
        let absent = || Error::Unknown(format!("output {output:?}"));
        let name = ServiceName::new(output)?;
        let service = match T::open(ipc.service_builder(&name).publish_subscribe::<T>().user_header::<Headed<H>>()) {
            Ok(service) => service,
            Err(PublishSubscribeOpenError::DoesNotExist) => return Err(absent()),
            Err(PublishSubscribeOpenError::IncompatibleTypes) => {
                let expected = T::message_type::<H>();
                return Err(Error::Mismatch(format!(
                    "{output} does not carry {}{}",
                    expected.name,
                    if expected.slice { " slices" } else { "" }
                )));
            }
            Err(error) => return Err(error.into()),
        };
        let sender = sender(&service).ok_or_else(absent)?;
        let events = match ipc.service_builder(&name).event().open() {
            Ok(events) => events,
            Err(EventOpenError::DoesNotExist) => return Err(absent()),
            Err(error) => return Err(error.into()),
        };
        let mut subscriber = service.subscriber_builder().name(&self.port);
        if let Some(depth) = self.depth {
            subscriber = subscriber.buffer_size(depth);
        }
        Ok(Link {
            subscriber: subscriber.create()?,
            listener: Arc::new(events.listener_builder().create()?),
            service: Arc::new(service),
            sender,
        })
    }
}

// The lock on `sources` is the one a node takes in `recv`. Everything below
// holds it only to read or change the list; calls into iceoryx2, which open
// services and look at other processes, are made without it.
impl<T: Payload + ?Sized, H: Header> InputPort for Shared<T, H> {
    fn connect(&self, ipc: &IpcNode, output: &str) -> Result {
        // A link to a node that is gone is no connection to the node that has its name now.
        self.drop_gone();
        let connected = |sources: &Sources<T, H>| sources.list.iter().any(|source| &*source.output == output);
        if connected(&self.sources.lock().unwrap()) {
            return Ok(());
        }
        let source = Source::new(output, self.link(ipc, output)?);
        let mut sources = self.sources.lock().unwrap();
        if !connected(&sources) {
            sources.list.push(source);
        }
        Ok(())
    }

    fn disconnect(&self, output: &str) {
        let mut sources = self.sources.lock().unwrap();
        let (gone, kept) = std::mem::take(&mut sources.list).into_iter().partition(|source| &*source.output == output);
        sources.list = kept;
        drop(sources);
        // Unsubscribing happens here, after the lock is released.
        let _: Vec<Source<T, H>> = gone;
    }

    fn drop_gone(&self) -> bool {
        // The services are looked at after the lock is released.
        let links: Vec<(Arc<PubSub<T, H>>, UniquePublisherId)> = {
            let sources = self.sources.lock().unwrap();
            sources.list.iter().map(|source| (source.link.service.clone(), source.link.sender)).collect()
        };
        // An output is gone when the port that sends on it is not the one the link was made to.
        let gone = |(service, linked): (Arc<PubSub<T, H>>, UniquePublisherId)| {
            (sender(&service) != Some(linked)).then_some(linked)
        };
        let lost: Vec<UniquePublisherId> = links.into_iter().filter_map(gone).collect();
        if lost.is_empty() {
            return false;
        }
        let mut sources = self.sources.lock().unwrap();
        // A source that was connected anew in the meantime has another sender, and stays.
        let is_lost = |source: &Source<T, H>| lost.contains(&source.link.sender);
        let (left, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut sources.list).into_iter().partition(is_lost);
        sources.list = kept;
        drop(sources);
        // Unsubscribing happens here, after the lock is released.
        !left.is_empty()
    }

    fn listeners(&self) -> Vec<Arc<Listener>> {
        let sources = self.sources.lock().unwrap();
        sources.list.iter().map(|source| source.link.listener.clone()).collect()
    }

    fn taken(&self) -> u64 {
        self.taken.load(Ordering::Relaxed)
    }

    fn left_waiting(&self) -> bool {
        if !self.unsure.load(Ordering::Relaxed) {
            return false;
        }
        let sources = self.sources.lock().unwrap();
        sources.list.iter().any(|source| source.link.subscriber.has_samples().unwrap_or(false))
    }

    fn info(&self) -> Vec<SourceInfo> {
        // The services are looked at after the lock is released.
        let (mut infos, links): (Vec<SourceInfo>, Vec<_>) = {
            let sources = self.sources.lock().unwrap();
            let describe = |source: &Source<T, H>| SourceInfo {
                output: source.output.to_string(),
                connected: false,
                received: source.received,
                lost: source.lost,
                sent_ns: source.last_sent_ns,
                latency_us: source.latency_ns / 1000,
                gap_us: source.gap_ns.largest(now_ns()) / 1000,
            };
            let link = |source: &Source<T, H>| (source.link.service.clone(), source.link.sender);
            sources.list.iter().map(|source| (describe(source), link(source))).unzip()
        };
        for (info, (service, linked)) in infos.iter_mut().zip(links) {
            info.connected = sender(&service) == Some(linked);
        }
        infos
    }
}
