//! Read any iceoryx2 topic as bytes, without having its message type.
//!
//! A program that subscribes to a topic normally names the Rust type of its
//! messages, and iceoryx2 checks it. A tool that shows whatever is on the
//! machine has no such type. iceoryx2 lets it subscribe all the same, the way
//! its own `iox2 service subscribe` does: by telling the subscriber the size
//! and alignment the topic itself declares.
//!
//! This is the one crate of triq that uses `unsafe`, in three places, each
//! with the reason it is sound beside it. `triq` itself forbids it.
//!
//! A tap is an ordinary reader. It takes one of the places a topic has for
//! subscribers, and it has a queue of its own, which loses its oldest message
//! when it is full: the sender never waits for a tap. A topic whose sender
//! does wait for full readers is not tapped at all.
use iceoryx2::node::Node;
use iceoryx2::port::subscriber::Subscriber;
use iceoryx2::prelude::{CallbackProgression, NodeBuilder, Service, ServiceName, SignalHandlingMode};
use iceoryx2::service::marker::{CustomHeaderMarker, CustomPayloadMarker};
use iceoryx2::service::static_config::message_type_details::TypeVariant;
use iceoryx2::service::static_config::messaging_pattern::MessagingPattern;
use iceoryx2::service::static_config::publish_subscribe::StaticConfig;
use std::fmt::{self, Debug, Display};

/// The kind of service triq's ports are: usable from any thread.
type Svc = iceoryx2::service::ipc_threadsafe::Service;

/// Why a topic could not be listed, opened or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Turns whatever iceoryx2 reports into an error that says what was being done.
fn while_trying<E: Debug>(to: impl Display) -> impl FnOnce(E) -> Error {
    move |error| Error(format!("could not {to}: {error:?}"))
}

/// One publish-subscribe topic, as iceoryx2 describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub name: String,
    /// The name its publisher gave the type of a message, or of one element of a slice.
    pub message: String,
    /// The size of that type, in bytes.
    pub size: usize,
    /// True when a message is a sequence of elements, of a length chosen for each message.
    pub slice: bool,
    /// How many bytes travel beside each message, in what iceoryx2 calls the user header.
    pub beside: usize,
    /// True when its sender waits for a reader whose queue is full. Such a topic cannot be tapped.
    pub waits: bool,
}

impl Topic {
    fn new(name: &str, config: &StaticConfig) -> Self {
        let types = config.message_type_details();
        Self {
            name: name.to_owned(),
            message: types.payload.type_name().to_string(),
            size: types.payload.size(),
            slice: types.payload.variant() == TypeVariant::Dynamic,
            beside: types.user_header.size(),
            waits: !config.has_safe_overflow(),
        }
    }
}

/// A program's way to the topics of this machine. Make one, and keep it for
/// as long as its taps are in use.
pub struct Taps {
    node: Node<Svc>,
}

impl Taps {
    pub fn new() -> Result<Self> {
        let node = NodeBuilder::new().signal_handling_mode(SignalHandlingMode::Disabled).create::<Svc>();
        Ok(Self { node: node.map_err(while_trying("join the machine's topics"))? })
    }

    /// Every publish-subscribe topic on this machine, sorted by name.
    pub fn topics(&self) -> Result<Vec<Topic>> {
        let mut topics = Vec::new();
        let listed = Svc::list(self.node.config(), |service| {
            let details = &service.static_details;
            if let MessagingPattern::PublishSubscribe(config) = details.messaging_pattern() {
                topics.push(Topic::new(details.name().as_str(), config));
            }
            CallbackProgression::Continue
        });
        listed.map_err(while_trying("list the topics"))?;
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(topics)
    }

    /// Start reading a topic that exists. A tap never creates one.
    pub fn open(&self, topic: &str) -> Result<Tap> {
        let opening = format!("open {topic}");
        let name = ServiceName::new(topic).map_err(while_trying(&opening))?;
        let details = Svc::details(&name, self.node.config(), iceoryx2::prelude::MessagingPattern::PublishSubscribe);
        let details = details.map_err(while_trying(&opening))?;
        let config = match details.as_ref().map(|details| details.static_details.messaging_pattern()) {
            Some(MessagingPattern::PublishSubscribe(config)) => config,
            _ => return Err(Error(format!("could not {opening}: there is no such topic"))),
        };
        if !config.has_safe_overflow() {
            let stalled = "a tap that fell behind would hold up its sender";
            return Err(Error(format!("could not {opening}: it waits for its readers, and {stalled}")));
        }
        let types = config.message_type_details();
        let builder = self.node.service_builder(&name).publish_subscribe::<[CustomPayloadMarker]>();
        // SAFETY: the subscriber is told the size and alignment that the topic
        // itself declares for its messages and for what travels beside them,
        // so iceoryx2 hands out memory of exactly the layout its publisher
        // wrote. Nothing here reads that memory as anything but bytes.
        let service = unsafe {
            builder
                .user_header::<CustomHeaderMarker>()
                .__internal_set_payload_type_details(&types.payload)
                .__internal_set_user_header_type_details(&types.user_header)
                .open()
        };
        let service = service.map_err(while_trying(&opening))?;
        let subscriber = service.subscriber_builder().create().map_err(while_trying(&opening))?;
        Ok(Tap { subscriber, beside: types.user_header.size() })
    }
}

/// One reader of one topic. Dropping it stops reading.
pub struct Tap {
    subscriber: Subscriber<Svc, [CustomPayloadMarker], CustomHeaderMarker>,
    /// How many bytes travel beside each message.
    beside: usize,
}

impl Tap {
    /// Take the oldest message that is waiting, if there is one, and show it
    /// to `read`: the bytes that travel beside it, and its own. They lie in
    /// shared memory and are not copied; they are gone when `read` returns.
    pub fn take<R>(&self, read: impl FnOnce(&[u8], &[u8]) -> R) -> Result<Option<R>> {
        let Some(sample) = self.subscriber.receive().map_err(while_trying("read a message"))? else {
            return Ok(None);
        };
        let header: *const CustomHeaderMarker = sample.user_header();
        // SAFETY: iceoryx2 keeps `beside` bytes for the user header at this
        // address, the size the topic declares, and the sample keeps them
        // alive and unchanged for as long as it is held. The bytes between
        // the members of a struct hold whatever was there before, which is
        // still bytes.
        let beside = unsafe { std::slice::from_raw_parts(header.cast::<u8>(), self.beside) };
        let payload = sample.payload();
        // SAFETY: a `CustomPayloadMarker` is one byte, so the same memory is
        // a slice of as many bytes; the sample keeps it alive as above.
        let message = unsafe { std::slice::from_raw_parts(payload.as_ptr().cast::<u8>(), payload.len()) };
        Ok(Some(read(beside, message)))
    }
}
