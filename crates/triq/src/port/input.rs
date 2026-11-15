//! The receiving end of a connection: one iceoryx2 subscriber per connected output.
use super::{NodeGuard, PortOptions};
use crate::inventory::SourceInfo;
use crate::message::Headed;
use crate::message::now_ns;
use crate::raw::{IpcNode, Listener, PubSub, RawSample, Subscriber, any_alive};
use crate::{Error, Header, Payload, Result, Stamp};
use iceoryx2::prelude::{CallbackProgression, PortFactory, PortName, ServiceName};
use iceoryx2::service::builder::event::EventOpenError;
use iceoryx2::service::builder::publish_subscribe::PublishSubscribeOpenError;
use std::ops::Deref;
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
    /// Remember that this input wants `output`, and subscribe if it exists already.
    fn connect(&self, ipc: &IpcNode, output: &str) -> Result;
    fn disconnect(&self, output: &str);
    /// Subscribe to wanted outputs that have appeared since. True if any did.
    fn link_pending(&self, ipc: &IpcNode) -> bool;
    /// One per linked output; a thread waiting on them wakes when data arrives.
    fn listeners(&self) -> Vec<Arc<Listener>>;
    fn info(&self) -> Vec<SourceInfo>;
}

pub(crate) struct Shared<T: Payload + ?Sized, H: Header> {
    /// `node/port`: the name every subscriber of this input carries in iceoryx2.
    port: PortName,
    depth: Option<usize>,
    sources: Mutex<Sources<T, H>>,
}

struct Sources<T: Payload + ?Sized, H: Header> {
    list: Vec<Source<T, H>>,
    /// Where the next `recv` starts looking, so one busy output cannot starve the others.
    next: usize,
}

/// One output this input wants to hear from.
struct Source<T: Payload + ?Sized, H: Header> {
    output: Arc<str>,
    /// `None` while the output does not exist yet.
    link: Option<Link<T, H>>,
    /// Why the last attempt to link failed, other than the output not existing.
    problem: Option<String>,
    last_seq: u64,
    /// When the sample before was sent, by its stamp.
    last_sent_ns: u64,
    received: u64,
    lost: u64,
    latency_ns: u64,
    /// The time from one sample to the next, by when they were sent.
    gap_ns: Lately,
}

/// The largest value noted lately: in the current second by the clock, or in
/// the one before. Nothing is reset when it is read, so any number of tools
/// that look now and then all see every peak.
#[derive(Default)]
struct Lately {
    /// The second that `now` is the largest of.
    second: u64,
    now: u64,
    before: u64,
}

impl Lately {
    const SECOND: u64 = 1_000_000_000;

    fn note(&mut self, at_ns: u64, value: u64) {
        let second = at_ns / Self::SECOND;
        if second != self.second {
            // The largest of a second ago moves back; anything older is forgotten.
            self.before = if second == self.second + 1 { self.now } else { 0 };
            (self.second, self.now) = (second, 0);
        }
        self.now = self.now.max(value);
    }

    fn largest(&self, at_ns: u64) -> u64 {
        match (at_ns / Self::SECOND).saturating_sub(self.second) {
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
}

/// Whether the node behind an output is running.
fn has_sender<T: Payload + ?Sized, H: Header>(service: &PubSub<T, H>) -> bool {
    let mut publishers = Vec::new();
    service.dynamic_config().list_publishers(|publisher| {
        publishers.push(publisher.node_id);
        CallbackProgression::Continue
    });
    any_alive(&publishers, service)
}

enum LinkError {
    Absent,
    Failed(Error),
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
            let Some(link) = &source.link else { continue };
            if let Some(raw) = T::receive(&link.subscriber)? {
                source.count(raw.user_header().stamp);
                let output = source.output.clone();
                sources.next = index + 1;
                return Ok(Some(Sample { raw, output }));
            }
        }
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
    fn new(output: &str) -> Self {
        Self {
            output: output.into(),
            link: None,
            problem: None,
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
        })
    }

    /// Open the output's services and subscribe. iceoryx2 refuses a different message type.
    fn link(&self, ipc: &IpcNode, output: &str) -> std::result::Result<Link<T, H>, LinkError> {
        let failed = |error: Error| LinkError::Failed(error);
        let name = ServiceName::new(output).map_err(|error| failed(error.into()))?;
        let service = match T::open(ipc.service_builder(&name).publish_subscribe::<T>().user_header::<Headed<H>>()) {
            Ok(service) => service,
            Err(PublishSubscribeOpenError::DoesNotExist) => return Err(LinkError::Absent),
            Err(PublishSubscribeOpenError::IncompatibleTypes) => {
                let expected = T::message_type::<H>();
                return Err(failed(Error::Mismatch(format!(
                    "{output} does not carry {}{}",
                    expected.name,
                    if expected.slice { " slices" } else { "" }
                ))));
            }
            Err(error) => return Err(failed(error.into())),
        };
        let events = match ipc.service_builder(&name).event().open() {
            Ok(events) => events,
            Err(EventOpenError::DoesNotExist) => return Err(LinkError::Absent),
            Err(error) => return Err(failed(error.into())),
        };
        let mut subscriber = service.subscriber_builder().name(&self.port);
        if let Some(depth) = self.depth {
            subscriber = subscriber.buffer_size(depth);
        }
        Ok(Link {
            subscriber: subscriber.create().map_err(|error| failed(error.into()))?,
            listener: Arc::new(events.listener_builder().create().map_err(|error| failed(error.into()))?),
            service: Arc::new(service),
        })
    }
}

// The lock on `sources` is the one a node takes in `recv`. Everything below
// holds it only to read or change the list; calls into iceoryx2, which open
// services and look at other processes, are made without it.
impl<T: Payload + ?Sized, H: Header> InputPort for Shared<T, H> {
    fn connect(&self, ipc: &IpcNode, output: &str) -> Result {
        let wanted = |sources: &Sources<T, H>| sources.list.iter().any(|source| &*source.output == output);
        if wanted(&self.sources.lock().unwrap()) {
            return Ok(());
        }
        let mut source = Source::new(output);
        match self.link(ipc, output) {
            Ok(link) => source.link = Some(link),
            Err(LinkError::Absent) => {}
            Err(LinkError::Failed(error)) => return Err(error),
        }
        let mut sources = self.sources.lock().unwrap();
        if !wanted(&sources) {
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

    fn link_pending(&self, ipc: &IpcNode) -> bool {
        let pending: Vec<Arc<str>> = {
            let sources = self.sources.lock().unwrap();
            sources.list.iter().filter(|source| source.link.is_none()).map(|source| source.output.clone()).collect()
        };
        let mut linked = false;
        for output in pending {
            let attempt = self.link(ipc, &output);
            let mut sources = self.sources.lock().unwrap();
            // The source may have been disconnected while the attempt was under way.
            let Some(source) = sources.list.iter_mut().find(|source| source.output == output && source.link.is_none())
            else {
                continue;
            };
            match attempt {
                Ok(link) => {
                    source.link = Some(link);
                    source.problem = None;
                    linked = true;
                }
                Err(LinkError::Absent) => {}
                Err(LinkError::Failed(error)) => source.problem = Some(error.to_string()),
            }
        }
        linked
    }

    fn listeners(&self) -> Vec<Arc<Listener>> {
        let sources = self.sources.lock().unwrap();
        sources.list.iter().filter_map(|source| source.link.as_ref()).map(|link| link.listener.clone()).collect()
    }

    fn info(&self) -> Vec<SourceInfo> {
        // The services are looked at after the lock is released.
        let (mut infos, services): (Vec<SourceInfo>, Vec<_>) = {
            let sources = self.sources.lock().unwrap();
            let describe = |source: &Source<T, H>| SourceInfo {
                output: source.output.to_string(),
                connected: false,
                problem: source.problem.clone(),
                received: source.received,
                lost: source.lost,
                latency_us: source.latency_ns / 1000,
                gap_us: source.gap_ns.largest(now_ns()) / 1000,
            };
            let service = |source: &Source<T, H>| source.link.as_ref().map(|link| link.service.clone());
            sources.list.iter().map(|source| (describe(source), service(source))).unzip()
        };
        for (info, service) in infos.iter_mut().zip(services) {
            info.connected = service.is_some_and(|service| has_sender(&service));
        }
        infos
    }
}
