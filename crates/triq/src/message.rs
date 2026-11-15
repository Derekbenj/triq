//! What travels between ports: plain `#[repr(C)]` data, or a slice of it.
//!
//! The data lives in shared memory. A sender writes it there once and every
//! receiver reads it in place, so nothing is serialized and nothing is copied
//! on the way.
use crate::raw::{PubSub, Publisher, PublisherBuilder, RawLoan, RawSample, ServiceBuilder, Subscriber};
use crate::{PortOptions, Result};
use iceoryx2::port::{BackpressureAction, BackpressureInfo};
use iceoryx2::prelude::{AllocationStrategy, BackpressureStrategy, IceoryxSend, PortName, ZeroCopySend};
use iceoryx2::service::builder::publish_subscribe::PublishSubscribeOpenError;
use iceoryx2::service::static_config::message_type_details::{TypeDetail, TypeVariant};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
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

pub(crate) fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos() as u64)
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
}

/// A message type: any `T: ZeroCopySend`, or a slice `[T]` of it.
///
/// iceoryx2 implements the two shapes in separate blocks. The hidden methods
/// below are the places where that shows, so the rest of triq is written once.
pub trait Payload: Debug + ZeroCopySend + IceoryxSend + 'static {
    /// Samples each subscriber can have waiting unless the output says otherwise.
    /// Small fixed-size messages get room to ride out a delayed reader; slices
    /// are usually large frames where only the newest ones matter.
    const DEFAULT_QUEUE: usize;

    fn message_type() -> MessageType;
    #[doc(hidden)]
    fn publisher(service: &PubSub<Self>, name: &PortName, options: &PortOptions) -> Result<Publisher<Self>>;
    #[doc(hidden)]
    fn open(service: ServiceBuilder<Self>) -> std::result::Result<PubSub<Self>, PublishSubscribeOpenError>;
    #[doc(hidden)]
    fn open_or_create(service: ServiceBuilder<Self>) -> Result<PubSub<Self>>;
    #[doc(hidden)]
    fn receive(subscriber: &Subscriber<Self>) -> Result<Option<RawSample<Self>>>;
    #[doc(hidden)]
    fn received(sample: &RawSample<Self>) -> &Self;
    #[doc(hidden)]
    fn loaned(loan: &RawLoan<Self>) -> &Self;
    #[doc(hidden)]
    fn loaned_mut(loan: &mut RawLoan<Self>) -> &mut Self;
}

/// The hooks that read the same for both shapes but resolve to different iceoryx2 methods.
macro_rules! shared_hooks {
    () => {
        fn open(service: ServiceBuilder<Self>) -> std::result::Result<PubSub<Self>, PublishSubscribeOpenError> {
            service.open()
        }
        fn open_or_create(service: ServiceBuilder<Self>) -> Result<PubSub<Self>> {
            Ok(service.open_or_create()?)
        }
        fn receive(subscriber: &Subscriber<Self>) -> Result<Option<RawSample<Self>>> {
            Ok(subscriber.receive()?)
        }
        fn received(sample: &RawSample<Self>) -> &Self {
            sample.payload()
        }
        fn loaned(loan: &RawLoan<Self>) -> &Self {
            loan.payload()
        }
        fn loaned_mut(loan: &mut RawLoan<Self>) -> &mut Self {
            loan.payload_mut()
        }
    };
}

impl<T: Debug + ZeroCopySend + 'static> Payload for T {
    const DEFAULT_QUEUE: usize = 32;

    fn message_type() -> MessageType {
        describe(TypeDetail::new::<T>(TypeVariant::FixedSize), false)
    }
    fn publisher(service: &PubSub<Self>, name: &PortName, options: &PortOptions) -> Result<Publisher<Self>> {
        Ok(configure(service.publisher_builder(), name, options).create()?)
    }
    shared_hooks!();
}

impl<T: Debug + ZeroCopySend + 'static> Payload for [T] {
    const DEFAULT_QUEUE: usize = 2;

    fn message_type() -> MessageType {
        describe(TypeDetail::new::<T>(TypeVariant::Dynamic), true)
    }
    fn publisher(service: &PubSub<Self>, name: &PortName, options: &PortOptions) -> Result<Publisher<Self>> {
        // No size is declared: iceoryx2 grows the shared memory when a longer slice is sent.
        Ok(configure(service.publisher_builder(), name, options)
            .initial_max_slice_len(1024)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?)
    }
    shared_hooks!();
}

fn describe(detail: TypeDetail, slice: bool) -> MessageType {
    MessageType { name: detail.type_name().to_string(), size: detail.size(), align: detail.alignment(), slice }
}

/// A reliable output keeps retrying a full subscriber for `reliable_ms`, then gives up on it.
fn configure<'a, T: Payload + ?Sized>(
    builder: PublisherBuilder<'a, T>,
    name: &PortName,
    options: &PortOptions,
) -> PublisherBuilder<'a, T> {
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
