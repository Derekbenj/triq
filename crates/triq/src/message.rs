//! What travels between ports: plain `#[repr(C)]` data, or a slice of it.
//!
//! The data lives in shared memory. A sender writes it there once and every
//! receiver reads it in place, so nothing is serialized and nothing is copied
//! on the way.
use crate::raw::{PubSub, Publisher, PublisherBuilder, RawLoan, RawSample, ServiceBuilder, Subscriber};
use crate::{Fields, Layout, PortOptions, Result};
use iceoryx2::port::{BackpressureAction, BackpressureInfo};
use iceoryx2::prelude::{AllocationStrategy, BackpressureStrategy, IceoryxSend, PortName, ZeroCopySend};
use iceoryx2::service::builder::publish_subscribe::PublishSubscribeOpenError;
use iceoryx2::service::static_config::message_type_details::{TypeDetail, TypeVariant};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Written by the sending output into the header of every sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend)]
#[repr(C)]
#[type_name("triq.Stamp")]
pub struct Stamp {
    /// Counts the samples of one output, starting at 1. A gap means a loss.
    pub seq: u64,
    /// Wall-clock send time in nanoseconds since the Unix epoch.
    pub sent_ns: u64,
}

impl Stamp {
    /// The stamp at the start of `beside`: the bytes iceoryx2 carries beside
    /// a message, which it calls the user header. For tools that read a
    /// message without its type. `None` if the bytes are too few.
    pub fn read(beside: &[u8]) -> Option<Self> {
        let number = |at: usize| Some(u64::from_ne_bytes(*beside.get(at..)?.first_chunk()?));
        Some(Self { seq: number(0)?, sent_ns: number(8)? })
    }
}

pub(crate) fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos() as u64)
}

/// Data that travels with every message of a port, beside the payload: for a
/// camera frame, its width and height. A port without one uses `()`.
pub trait Header: Debug + Default + ZeroCopySend + Fields + 'static {}

impl<H: Debug + Default + ZeroCopySend + Fields + 'static> Header for H {}

/// What iceoryx2 carries beside every payload: triq's stamp and the port's header.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[repr(C)]
pub struct Headed<H: Header> {
    pub(crate) stamp: Stamp,
    pub(crate) header: H,
}

/// The identity iceoryx2 checks when an input opens an output's service.
/// An input and an output can be connected when their message types are equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageType {
    /// `#[type_name("...")]` of the message, or its Rust path without one.
    pub name: String,
    pub size: usize,
    pub align: usize,
    /// True for `[T]`: a sequence whose length is chosen per sample.
    pub slice: bool,
    /// The name of the port's header type; empty when it has none.
    pub header: String,
}

/// As a person would write it: `Pose`, `u8[]`, or `u8[] + Frame` with a header.
impl Display for MessageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, if self.slice { "[]" } else { "" })?;
        if self.header.is_empty() { Ok(()) } else { write!(f, " + {}", self.header) }
    }
}

/// A message type: any `T: ZeroCopySend + Fields`, or a slice `[T]` of it.
///
/// iceoryx2 implements the two shapes in separate blocks. The hidden methods
/// below are the places where that shows, so the rest of triq is written once.
pub trait Payload: Debug + ZeroCopySend + IceoryxSend + 'static {
    /// Samples each subscriber can have waiting unless the output says otherwise.
    /// Small fixed-size messages get room to ride out a delayed reader; slices
    /// are usually large frames where only the newest ones matter.
    const DEFAULT_QUEUE: usize;

    /// How a port of this message, with header `H`, is advertised.
    fn message_type<H: Header>() -> MessageType;
    /// The numbers inside this message and inside the header `H`.
    fn layout<H: Header>() -> Layout;
    #[doc(hidden)]
    fn publisher<H: Header>(
        service: &PubSub<Self, H>,
        name: &PortName,
        options: &PortOptions,
    ) -> Result<Publisher<Self, H>>;
    #[doc(hidden)]
    fn open<H: Header>(
        service: ServiceBuilder<Self, H>,
    ) -> std::result::Result<PubSub<Self, H>, PublishSubscribeOpenError>;
    #[doc(hidden)]
    fn open_or_create<H: Header>(service: ServiceBuilder<Self, H>) -> Result<PubSub<Self, H>>;
    #[doc(hidden)]
    fn receive<H: Header>(subscriber: &Subscriber<Self, H>) -> Result<Option<RawSample<Self, H>>>;
    #[doc(hidden)]
    fn received<H: Header>(sample: &RawSample<Self, H>) -> &Self;
    #[doc(hidden)]
    fn loaned<H: Header>(loan: &RawLoan<Self, H>) -> &Self;
    #[doc(hidden)]
    fn loaned_mut<H: Header>(loan: &mut RawLoan<Self, H>) -> &mut Self;
}

/// The hooks that read the same for both shapes but resolve to different iceoryx2 methods.
macro_rules! shared_hooks {
    () => {
        fn open<H: Header>(
            service: ServiceBuilder<Self, H>,
        ) -> std::result::Result<PubSub<Self, H>, PublishSubscribeOpenError> {
            service.open()
        }
        fn open_or_create<H: Header>(service: ServiceBuilder<Self, H>) -> Result<PubSub<Self, H>> {
            Ok(service.open_or_create()?)
        }
        fn receive<H: Header>(subscriber: &Subscriber<Self, H>) -> Result<Option<RawSample<Self, H>>> {
            Ok(subscriber.receive()?)
        }
        fn received<H: Header>(sample: &RawSample<Self, H>) -> &Self {
            sample.payload()
        }
        fn loaned<H: Header>(loan: &RawLoan<Self, H>) -> &Self {
            loan.payload()
        }
        fn loaned_mut<H: Header>(loan: &mut RawLoan<Self, H>) -> &mut Self {
            loan.payload_mut()
        }
    };
}

impl<T: Debug + ZeroCopySend + Fields + 'static> Payload for T {
    const DEFAULT_QUEUE: usize = 32;

    fn message_type<H: Header>() -> MessageType {
        describe::<H>(TypeDetail::new::<T>(TypeVariant::FixedSize), false)
    }
    fn layout<H: Header>() -> Layout {
        layout::<T, H>()
    }
    fn publisher<H: Header>(
        service: &PubSub<Self, H>,
        name: &PortName,
        options: &PortOptions,
    ) -> Result<Publisher<Self, H>> {
        Ok(configure(service.publisher_builder(), name, options).create()?)
    }
    shared_hooks!();
}

impl<T: Debug + ZeroCopySend + Fields + 'static> Payload for [T] {
    const DEFAULT_QUEUE: usize = 2;

    fn message_type<H: Header>() -> MessageType {
        describe::<H>(TypeDetail::new::<T>(TypeVariant::Dynamic), true)
    }
    fn layout<H: Header>() -> Layout {
        layout::<T, H>()
    }
    fn publisher<H: Header>(
        service: &PubSub<Self, H>,
        name: &PortName,
        options: &PortOptions,
    ) -> Result<Publisher<Self, H>> {
        // No size is declared: iceoryx2 grows the shared memory when a longer slice is sent.
        Ok(configure(service.publisher_builder(), name, options)
            .initial_max_slice_len(1024)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?)
    }
    shared_hooks!();
}

/// The numbers of a `T` and of the header `H`, which iceoryx2 carries after the stamp.
fn layout<T: Fields, H: Header>() -> Layout {
    let (mut payload, mut header) = (Vec::new(), Vec::new());
    T::fields("", 0, &mut payload);
    H::fields("", std::mem::offset_of!(Headed<H>, header), &mut header);
    Layout { payload, header }
}

fn describe<H: Header>(detail: TypeDetail, slice: bool) -> MessageType {
    let header = TypeDetail::new::<H>(TypeVariant::FixedSize).type_name().to_string();
    MessageType {
        name: detail.type_name().to_string(),
        size: detail.size(),
        align: detail.alignment(),
        slice,
        header: if header == "()" { String::new() } else { header },
    }
}

/// A reliable output keeps retrying a full subscriber for `reliable_ms`, then gives up on it.
fn configure<'a, T: Payload + ?Sized, H: Header>(
    builder: PublisherBuilder<'a, T, H>,
    name: &PortName,
    options: &PortOptions,
) -> PublisherBuilder<'a, T, H> {
    let builder = builder.name(name);
    let Some(patience) = options.reliable_ms.map(Duration::from_millis) else {
        return builder;
    };
    builder.backpressure_strategy(BackpressureStrategy::RetryUntilDelivered).set_backpressure_handler(
        move |incident: &BackpressureInfo| {
            if incident.elapsed_time < patience {
                BackpressureAction::Retry
            } else {
                BackpressureAction::DiscardDataAndFail
            }
        },
    )
}
