//! The sending end of a connection: one iceoryx2 service per output.
use super::{NodeGuard, PortOptions, PortSpec};
use crate::inventory::OutputInfo;
use crate::message::{Headed, now_ns};
use crate::raw::{IpcNode, Notifier, PubSub, Publisher, RawLoan};
use crate::{Error, Fields, Header, Layout, Payload, Result, Stamp};
use iceoryx2::port::SendError;
use iceoryx2::prelude::{PortFactory, PortName, ServiceName, ZeroCopySend};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_SUBSCRIBERS: usize = 8;
const DEFAULT_HOLD: usize = 2;

/// Sends messages of type `T` to every connected input.
///
/// By default a slow input loses its oldest waiting sample and the sender
/// never waits. With `#[port(reliable_ms = N)]` the sender instead waits up to
/// `N` milliseconds for a full input and then fails with [`Error::Undelivered`].
///
/// `H` is an optional header sent beside every message, such as the width and
/// height of a frame whose pixels are the message.
pub struct Output<T: Payload + ?Sized, H: Header = ()> {
    shared: Arc<Shared<T, H>>,
    _node: Arc<NodeGuard>,
}

/// What the runtime keeps of an output after handing it to its node.
pub(crate) trait OutputPort: Send + Sync {
    fn info(&self, spec: &PortSpec) -> OutputInfo;
    /// The numbers inside what it sends.
    fn layout(&self) -> Layout;
}

pub(crate) struct Shared<T: Payload + ?Sized, H: Header> {
    name: String,
    publisher: Publisher<T, H>,
    /// Wakes the threads waiting on the connected inputs.
    notifier: Notifier,
    service: PubSub<T, H>,
    seq: AtomicU64,
    sent: AtomicU64,
    /// When the newest sample was sent. A report reads this and `sent` one
    /// after the other; a sample sent between the two reads is counted and
    /// not yet timed, or timed and not yet counted, until the next report.
    sent_ns: AtomicU64,
    undelivered: AtomicU64,
}

impl<T: Payload + ?Sized, H: Header> Output<T, H> {
    pub(crate) fn new(shared: Arc<Shared<T, H>>, node: Arc<NodeGuard>) -> Self {
        Self { shared, _node: node }
    }

    /// The machine-wide name inputs connect to: `runtime/node/port`.
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// How many inputs are connected right now. Worth checking before
    /// producing something expensive that nobody would receive.
    pub fn subscribers(&self) -> usize {
        self.shared.service.dynamic_config().number_of_subscribers()
    }
}

impl<T: Payload, H: Header> Output<T, H> {
    /// Copy `value` into shared memory and deliver it.
    /// Returns how many inputs received it.
    pub fn send(&self, value: T) -> Result<usize> {
        self.send_with(value, H::default())
    }

    /// Like [`Output::send`], with a header.
    pub fn send_with(&self, value: T, header: H) -> Result<usize> {
        let loan = self.shared.publisher.loan_uninit()?.write_payload(value);
        self.shared.deliver(loan, header)
    }

    /// Shared memory holding `T::default()`, to be filled in place and sent.
    pub fn loan(&self) -> Result<Loan<'_, T, H>>
    where
        T: Default,
    {
        Ok(Loan { raw: self.shared.publisher.loan()?, header: H::default(), output: &self.shared })
    }
}

impl<T: Debug + ZeroCopySend + Fields + 'static, H: Header> Output<[T], H> {
    /// Copy `values` into shared memory once and deliver them.
    pub fn send_slice(&self, values: &[T]) -> Result<usize>
    where
        T: Copy,
    {
        self.send_slice_with(values, H::default())
    }

    /// Like [`Output::send_slice`], with a header.
    pub fn send_slice_with(&self, values: &[T], header: H) -> Result<usize>
    where
        T: Copy,
    {
        let loan = self.shared.publisher.loan_slice_uninit(values.len())?.write_from_slice(values);
        self.shared.deliver(loan, header)
    }

    /// Shared memory for `len` default elements, to be filled in place and sent.
    pub fn loan(&self, len: usize) -> Result<Loan<'_, [T], H>>
    where
        T: Default,
    {
        Ok(Loan { raw: self.shared.publisher.loan_slice(len)?, header: H::default(), output: &self.shared })
    }
}

/// A message under construction, already in shared memory.
/// Dropping it without calling [`Loan::send`] gives the memory back.
pub struct Loan<'a, T: Payload + ?Sized, H: Header = ()> {
    raw: RawLoan<T, H>,
    header: H,
    output: &'a Shared<T, H>,
}

impl<T: Payload + ?Sized, H: Header> Loan<'_, T, H> {
    pub fn header(&self) -> &H {
        &self.header
    }

    /// The header to send with the message; it starts as `H::default()`.
    pub fn header_mut(&mut self) -> &mut H {
        &mut self.header
    }

    /// Deliver the message. Returns how many inputs received it.
    pub fn send(self) -> Result<usize> {
        self.output.deliver(self.raw, self.header)
    }
}

impl<T: Payload + ?Sized, H: Header> Deref for Loan<'_, T, H> {
    type Target = T;
    fn deref(&self) -> &T {
        T::loaned(&self.raw)
    }
}

impl<T: Payload + ?Sized, H: Header> DerefMut for Loan<'_, T, H> {
    fn deref_mut(&mut self) -> &mut T {
        T::loaned_mut(&mut self.raw)
    }
}

impl<T: Payload + ?Sized, H: Header> Shared<T, H> {
    pub fn create(ipc: &IpcNode, service: &str, port: &str, options: &PortOptions) -> Result<Self> {
        if options.depth.is_some() {
            return Err(Error::Mismatch(format!("{service}: `depth` belongs on an input")));
        }
        let name = ServiceName::new(service)?;
        let subscribers = options.subscribers.unwrap_or(DEFAULT_SUBSCRIBERS);
        // The wake-up service comes first, so an input that finds the data service finds both.
        // A restarted node reopens the services its subscribers kept alive.
        let events = ipc
            .service_builder(&name)
            .event()
            .max_listeners(subscribers)
            .max_nodes(subscribers + 2)
            .open_or_create()?;
        let service = T::open_or_create(
            ipc.service_builder(&name)
                .publish_subscribe::<T>()
                .user_header::<Headed<H>>()
                .max_publishers(1)
                .max_subscribers(subscribers)
                .max_nodes(subscribers + 2)
                .subscriber_max_buffer_size(options.queue.unwrap_or(T::DEFAULT_QUEUE))
                .subscriber_max_borrowed_samples(options.hold.unwrap_or(DEFAULT_HOLD))
                .enable_safe_overflow(options.reliable_ms.is_none()),
        )?;
        Ok(Self {
            name: name.to_string(),
            publisher: T::publisher(&service, &PortName::new(port)?, options)?,
            notifier: events.notifier_builder().create()?,
            service,
            seq: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            sent_ns: AtomicU64::new(0),
            undelivered: AtomicU64::new(0),
        })
    }

    fn deliver(&self, mut loan: RawLoan<T, H>, header: H) -> Result<usize> {
        let stamp = Stamp { seq: self.seq.fetch_add(1, Ordering::Relaxed) + 1, sent_ns: now_ns() };
        *loan.user_header_mut() = Headed { stamp, header };
        self.sent.fetch_add(1, Ordering::Relaxed);
        self.sent_ns.store(stamp.sent_ns, Ordering::Relaxed);
        let delivered = match loan.send() {
            Ok(delivered) => delivered,
            // A reliable output gave up on one input; the others have the sample.
            Err(SendError::UnableToDeliver) => {
                self.undelivered.fetch_add(1, Ordering::Relaxed);
                self.notifier.notify()?;
                return Err(Error::Undelivered);
            }
            Err(error) => return Err(error.into()),
        };
        if delivered > 0 {
            self.notifier.notify()?;
        }
        Ok(delivered)
    }
}

impl<T: Payload + ?Sized, H: Header> OutputPort for Shared<T, H> {
    fn info(&self, spec: &PortSpec) -> OutputInfo {
        OutputInfo {
            port: spec.clone(),
            service: self.name.clone(),
            subscribers: self.service.dynamic_config().number_of_subscribers(),
            sent: self.sent.load(Ordering::Relaxed),
            sent_ns: self.sent_ns.load(Ordering::Relaxed),
            undelivered: self.undelivered.load(Ordering::Relaxed),
        }
    }

    fn layout(&self) -> Layout {
        T::layout::<H>()
    }
}
