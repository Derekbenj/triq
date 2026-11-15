//! The control plane: how other programs ask a runtime to change.
//!
//! Every runtime owns one request/response service named `<runtime>/_control`
//! and answers it from a small thread of its own. Requests and replies are
//! JSON. No message between nodes ever passes through here.
//!
//! Owning the service's only server port is also what makes a runtime name
//! unique on the machine.
use crate::raw::{IpcNode, Listener, Notifier, Svc, any_alive};
use crate::runtime::Inner;
use crate::{Error, Inventory, Placement, Result};
use iceoryx2::prelude::{
    AllocationStrategy, CallbackProgression, PortFactory, ServiceName, SignalHandlingMode, WaitSetBuilder,
};
use iceoryx2::service::builder::request_response::RequestResponseOpenError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type Service = iceoryx2::service::port_factory::request_response::PortFactory<Svc, [u8], (), [u8], ()>;
type Server = iceoryx2::port::server::Server<Svc, [u8], (), [u8], ()>;
type Client = iceoryx2::port::client::Client<Svc, [u8], (), [u8], ()>;

const MAX_CLIENTS: usize = 32;
/// How often a runtime looks for outputs its inputs are still waiting for.
const HOUSEKEEPING: Duration = Duration::from_millis(250);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const LIVENESS_CHECK: Duration = Duration::from_millis(100);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Request {
    Inventory,
    Spawn { kind: String, name: String, placement: Placement, params: Value },
    Remove { node: String },
    Connect { from: String, to: String },
    Disconnect { from: String, to: String },
    SetParams { node: String, params: Value },
    Pause { node: String },
    Resume { node: String },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Reply {
    Done,
    Inventory(Box<Inventory>),
}

/// The one machine-wide service on which every runtime announces that it changed.
/// It carries no data: a tool that hears it asks the runtimes for their inventories.
pub(crate) fn announcements(ipc: &IpcNode) -> Result<Announcements> {
    const RUNTIMES: usize = 256;
    const WATCHERS: usize = 64;
    Ok(ipc
        .service_builder(&ServiceName::new("_triq/changes")?)
        .event()
        .max_notifiers(RUNTIMES)
        .max_listeners(WATCHERS)
        .max_nodes(RUNTIMES + WATCHERS)
        .open_or_create()?)
}

pub(crate) type Announcements = iceoryx2::service::port_factory::event::PortFactory<Svc>;

/// The two services behind a runtime's control name: requests, and the
/// signal that tells the other side there is something to read.
struct Channel {
    requests: Service,
    listener: Listener,
    notifier: Notifier,
}

impl Channel {
    /// `create` is true for the runtime itself and false for a program talking to it.
    fn open(ipc: &IpcNode, runtime: &str, create: bool) -> Result<Self> {
        let name = ServiceName::new(&format!("{runtime}/_control"))?;
        let requests = ipc
            .service_builder(&name)
            .request_response::<[u8], [u8]>()
            .max_servers(1)
            .max_clients(MAX_CLIENTS)
            .max_nodes(MAX_CLIENTS + 1);
        let signals = ipc
            .service_builder(&name)
            .event()
            .max_listeners(MAX_CLIENTS + 1)
            .max_notifiers(MAX_CLIENTS + 1)
            .max_nodes(MAX_CLIENTS + 1);
        let unknown = || Error::Unknown(format!("runtime {runtime:?}"));
        let (requests, signals) = if create {
            (requests.open_or_create()?, signals.open_or_create()?)
        } else {
            let requests = match requests.open() {
                Ok(requests) => requests,
                Err(RequestResponseOpenError::DoesNotExist) => return Err(unknown()),
                Err(error) => return Err(error.into()),
            };
            // A runtime that died leaves its service behind until iceoryx2 cleans up.
            if !answering(&requests) {
                return Err(unknown());
            }
            (requests, signals.open().map_err(|_| unknown())?)
        };
        Ok(Self {
            listener: signals.listener_builder().create()?,
            notifier: signals.notifier_builder().create()?,
            requests,
        })
    }
}

/// Whether the runtime behind a control service is still running.
fn answering(requests: &Service) -> bool {
    let mut servers = Vec::new();
    requests.dynamic_config().list_servers(|server| {
        servers.push(server.node_id);
        CallbackProgression::Continue
    });
    any_alive(&servers, requests)
}

/// The thread that answers for a runtime. Dropping it releases the runtime's name.
pub(crate) struct Control {
    stop: Arc<AtomicBool>,
    notifier: Arc<Notifier>,
    thread: Option<JoinHandle<()>>,
}

impl Control {
    /// Claim the runtime's name. The thread starts answering once [`Control::serve`] is called.
    pub fn claim(ipc: &IpcNode, runtime: &str) -> Result<Claim> {
        let channel = Channel::open(ipc, runtime, true)?;
        let server = channel
            .requests
            .server_builder()
            .initial_max_slice_len(4096)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(|error| Error::Name(format!("a runtime named {runtime:?} is already running ({error:?})")))?;
        Ok(Claim { channel, server })
    }
}

pub(crate) struct Claim {
    channel: Channel,
    server: Server,
}

impl Claim {
    pub fn serve(self, owner: Weak<Inner>) -> Result<Control> {
        let stop = Arc::new(AtomicBool::new(false));
        let Channel { requests, listener, notifier } = self.channel;
        let notifier = Arc::new(notifier);
        let thread = thread::Builder::new().name("triq-control".into()).spawn({
            let (stop, notifier, server) = (stop.clone(), notifier.clone(), self.server);
            move || {
                // Keeps the service alive for exactly as long as the thread.
                let _requests = requests;
                if let Err(error) = serve(&server, &listener, &notifier, &owner, &stop) {
                    eprintln!("triq: control thread stopped: {error}");
                }
            }
        })?;
        Ok(Control { stop, notifier, thread: Some(thread) })
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.notifier.notify();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(server: &Server, listener: &Listener, notifier: &Notifier, owner: &Weak<Inner>, stop: &AtomicBool) -> Result {
    let waitset = WaitSetBuilder::new().signal_handling_mode(SignalHandlingMode::Disabled).create::<Svc>()?;
    let _requests = waitset.attach_notification(listener)?;
    let _housekeeping = waitset.attach_interval(HOUSEKEEPING)?;
    while !stop.load(Ordering::Acquire) {
        waitset.wait_and_process_once(|_| CallbackProgression::Continue)?;
        listener.try_wait(|_| {})?;
        let Some(owner) = owner.upgrade() else { break };
        while let Some(request) = server.receive()? {
            let answer: Result<Reply> = serde_json::from_slice(request.payload())
                .map_err(Error::from)
                .and_then(|request| handle(&owner, request));
            let bytes = serde_json::to_vec(&answer)?;
            // The asking program may have given up or exited; that is its business.
            let sent = request.loan_slice_uninit(bytes.len()).map(|reply| reply.write_from_slice(&bytes).send());
            if matches!(sent, Ok(Ok(_))) {
                notifier.notify()?;
            }
        }
        owner.link_pending();
    }
    Ok(())
}

fn handle(owner: &Arc<Inner>, request: Request) -> Result<Reply> {
    match request {
        Request::Inventory => return Ok(Reply::Inventory(Box::new(owner.inventory()))),
        Request::Spawn { kind, name, placement, params } => owner.spawn_kind(&kind, &name, placement, params)?,
        Request::Remove { node } => owner.remove(&node)?,
        Request::Connect { from, to } => owner.connect(&from, &to)?,
        Request::Disconnect { from, to } => owner.disconnect(&from, &to)?,
        Request::SetParams { node, params } => owner.set_params(&node, params)?,
        Request::Pause { node } => owner.set_paused(&node, true)?,
        Request::Resume { node } => owner.set_paused(&node, false)?,
        Request::Shutdown => owner.shut_down(),
    }
    Ok(Reply::Done)
}

/// The asking side of a runtime's control service.
pub(crate) struct Caller {
    client: Client,
    channel: Channel,
}

impl Caller {
    pub fn open(ipc: &IpcNode, runtime: &str) -> Result<Self> {
        let channel = Channel::open(ipc, runtime, false)?;
        let client = channel
            .requests
            .client_builder()
            .initial_max_slice_len(1024)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?;
        Ok(Self { client, channel })
    }

    pub fn ask(&self, request: &Request) -> Result<Reply> {
        let bytes = serde_json::to_vec(request)?;
        let pending = self.client.loan_slice_uninit(bytes.len())?.write_from_slice(&bytes).send()?;
        self.channel.notifier.notify()?;
        let deadline = Instant::now() + ANSWER_TIMEOUT;
        let read = || -> Result<Option<Result<Reply>>> {
            match pending.receive()? {
                Some(answer) => Ok(Some(serde_json::from_slice::<Result<Reply>>(answer.payload())?)),
                None => Ok(None),
            }
        };
        loop {
            if let Some(answer) = read()? {
                return answer;
            }
            if !answering(&self.channel.requests) {
                // It may have answered just before exiting.
                return read()?.unwrap_or_else(|| Err(Error::Unknown("runtime: it exited before answering".into())));
            }
            let left = deadline.checked_duration_since(Instant::now()).ok_or(Error::Timeout)?;
            // Woken by any answer on this service; ours may not be the one, so look again.
            // The wait is short so that a runtime exiting without an answer is noticed.
            self.channel.listener.timed_wait(|_| {}, left.min(LIVENESS_CHECK))?;
        }
    }
}
