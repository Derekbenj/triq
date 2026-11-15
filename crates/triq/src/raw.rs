//! The iceoryx2 types triq is built on, named once.
//!
//! Everything uses the thread-safe IPC service variant: a port may be created
//! on one thread and used on another, and the same code path serves threads
//! of one process and separate processes.
use crate::message::Headed;

pub type Svc = iceoryx2::service::ipc_threadsafe::Service;
pub type IpcNode = iceoryx2::node::Node<Svc>;
pub type Listener = iceoryx2::port::listener::Listener<Svc>;
pub type Notifier = iceoryx2::port::notifier::Notifier<Svc>;

// A port of message type `T` with header type `H`. iceoryx2 carries triq's
// stamp and the port's header together, as its "user header".
pub type ServiceBuilder<T, H> = iceoryx2::service::builder::publish_subscribe::Builder<T, Headed<H>, Svc>;
pub type PubSub<T, H> = iceoryx2::service::port_factory::publish_subscribe::PortFactory<Svc, T, Headed<H>>;
pub type Publisher<T, H> = iceoryx2::port::publisher::Publisher<Svc, T, Headed<H>>;
pub type PublisherBuilder<'a, T, H> =
    iceoryx2::service::port_factory::publisher::PortFactoryPublisher<'a, Svc, T, Headed<H>>;
pub type Subscriber<T, H> = iceoryx2::port::subscriber::Subscriber<Svc, T, Headed<H>>;
pub type RawSample<T, H> = iceoryx2::sample::Sample<Svc, T, Headed<H>>;
pub type RawLoan<T, H> = iceoryx2::sample_mut::SampleMut<Svc, T, Headed<H>>;

/// Whether any of `owners`, the iceoryx2 nodes that created a service's sending
/// ports, is still alive. A port count alone cannot tell: a process that was
/// killed leaves its ports registered until iceoryx2 cleans up after it.
pub fn any_alive(
    owners: &[iceoryx2::identifiers::UniqueNodeId],
    service: &impl iceoryx2::prelude::PortFactory<Service = Svc>,
) -> bool {
    use iceoryx2::prelude::{CallbackProgression, NodeState};
    let mut alive = false;
    let _ = service.nodes(|node| {
        alive |= matches!(node, NodeState::Alive(_)) && owners.contains(node.node_id());
        CallbackProgression::Continue
    });
    alive
}

/// iceoryx2 logs at warning level by default, starting with a notice about a
/// missing config file. triq reports problems through its own results, so only
/// errors are let through unless `IOX2_LOG_LEVEL` says otherwise.
pub fn quiet_logs() {
    iceoryx2::prelude::set_log_level_from_env_or(iceoryx2::prelude::LogLevel::Error);
}
