//! A runtime: the part of a process that owns nodes and answers for them.
mod group;
mod local;

pub use local::{Local, LocalFactory, LocalNode, LocalNodes};

use crate::control::{Control, announcements};
use crate::executor::Executor;
use crate::inventory::{FactoryInfo, Group, InputInfo, Inventory, NodeInfo, NodeState, PoolInfo, merged, shown};
use crate::message::now_ns;
use crate::node::Slot;
use crate::port::{Binder, InputPort, NodeGuard, OutputPort};
use crate::raw::{IpcNode, Notifier, Svc, quiet_logs};
use crate::{Error, Layout, Node, Placement, PortSpec, Ports, Result};
use iceoryx2::prelude::{NodeBuilder, NodeName, SignalHandlingMode};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

/// Owns the nodes of one process and makes them visible to the machine.
///
/// Creating a runtime does not take over the calling thread.
pub struct Runtime {
    /// Answers other programs. Declared first so it stops before the nodes do.
    _control: Control,
    inner: Arc<Inner>,
    /// Declared last: announces the runtime's end once everything above is gone.
    _farewell: Farewell,
}

struct Farewell(Arc<Notifier>);

impl Drop for Farewell {
    fn drop(&mut self) {
        let _ = self.0.notify();
    }
}

/// Chooses a runtime's thread pools before it starts.
pub struct Builder {
    name: String,
    /// Name, thread count, and whether the pool is shared.
    pools: Vec<(String, usize, bool)>,
}

pub(crate) struct Inner {
    name: String,
    /// The iceoryx2 node all ports of this runtime are created under.
    ipc: Mutex<IpcNode>,
    pools: BTreeMap<String, Pool>,
    state: Mutex<State>,
    /// Set when the program is asked to exit; `Runtime::wait` sleeps on it.
    shutdown: (Mutex<bool>, Condvar),
    /// Counts the changes that a tool looking at this runtime would want to see.
    revision: AtomicU64,
    /// Tells every watching tool on the machine to look again.
    announce: Arc<Notifier>,
}

struct Pool {
    /// One per thread, or the single waiting thread of a shared pool.
    executors: Vec<Arc<Executor>>,
    threads: usize,
    shared: bool,
}

#[derive(Default)]
struct State {
    nodes: BTreeMap<String, Record>,
    factories: BTreeMap<String, Arc<Factory>>,
    next_id: u64,
}

/// What the runtime knows about one node.
#[derive(Clone)]
struct Record {
    /// Distinguishes this node from an earlier one of the same name.
    id: u64,
    kind: String,
    /// The group the node is in; empty for none.
    group: String,
    placement: Option<Placement>,
    params: Value,
    /// Held while the node is given new settings, so that it is given one set at a time. It is
    /// also what tells this node from a later one of its name.
    params_lock: Arc<Mutex<()>>,
    inputs: Vec<(PortSpec, Arc<dyn InputPort>)>,
    outputs: Vec<(PortSpec, Arc<dyn OutputPort>)>,
    /// `None` when the application runs the node itself.
    hosted: Option<Hosted>,
    /// How to tell the application about requests for a node its local factory made.
    notify: Option<Notify>,
    /// For nodes the application runs: whether it was asked to pause, and what
    /// it reported going wrong. A node the runtime runs keeps these in its slot.
    paused: bool,
    errors: u64,
    last_error: Option<String>,
}

/// A node the runtime runs: what to run, and the thread that runs it.
#[derive(Clone)]
struct Hosted {
    slot: Arc<Slot>,
    executor: Arc<Executor>,
}

/// A kind of node this runtime can create on request.
struct Factory {
    ports: Vec<PortSpec>,
    params: Value,
    info: Mutex<Value>,
    local: bool,
    /// What it creates, if that is a group of nodes.
    group: Option<Group>,
    spawn: Spawn,
}

/// Creates a node: the runtime, the node's name, where to run it, and its settings.
type Spawn = Box<dyn Fn(&Arc<Inner>, &str, Placement, Value) -> Result + Send + Sync>;

/// An input, and the thread that waits on it if the runtime runs its node.
type Wired = (Arc<dyn InputPort>, Option<Arc<Executor>>);

/// A request concerning a node that the application runs.
enum Notice {
    Params(Value),
    Paused(bool),
    Removed,
}

/// Passes a notice on to the application; for new settings, returns them as they will be reported.
type Notify = Arc<dyn Fn(&str, Notice) -> Result<Value> + Send + Sync>;

impl Builder {
    /// Add a pool of `threads` threads. A node placed in it keeps one of them,
    /// which is the fastest arrangement; a node that blocks delays the nodes
    /// sharing its thread.
    pub fn pool(mut self, name: &str, threads: usize) -> Self {
        self.pools.push((name.into(), threads, false));
        self
    }

    /// Add a pool whose free threads take whichever node is ready, so a node
    /// that blocks delays nobody. Each run costs one more thread wake-up.
    pub fn shared_pool(mut self, name: &str, threads: usize) -> Self {
        self.pools.push((name.into(), threads, true));
        self
    }

    pub fn start(self) -> Result<Runtime> {
        check_name("runtime", &self.name)?;
        quiet_logs();
        let ipc = NodeBuilder::new()
            .name(&NodeName::new(&self.name)?)
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<Svc>()?;
        let claim = Control::claim(&ipc, &self.name)?;
        let announce = Arc::new(announcements(&ipc)?.notifier_builder().create()?);
        let mut pools = BTreeMap::new();
        for (pool, threads, shared) in &self.pools {
            check_name("pool", pool)?;
            if *threads == 0 {
                return Err(Error::Mismatch(format!("pool {pool:?} needs at least one thread")));
            }
            let executors = if *shared {
                vec![Arc::new(Executor::start(&ipc, &self.name, pool, *threads)?)]
            } else {
                (0..*threads)
                    .map(|index| Executor::start(&ipc, &self.name, &format!("{pool}-{index}"), 0).map(Arc::new))
                    .collect::<Result<_>>()?
            };
            let pool_entry = Pool { executors, threads: *threads, shared: *shared };
            if pools.insert(pool.clone(), pool_entry).is_some() {
                return Err(Error::Name(format!("pool {pool:?} is declared twice")));
            }
        }
        let inner = Arc::new(Inner {
            name: self.name,
            ipc: Mutex::new(ipc),
            pools,
            state: Mutex::default(),
            shutdown: Default::default(),
            revision: AtomicU64::new(0),
            announce: announce.clone(),
        });
        let control = claim.serve(Arc::downgrade(&inner))?;
        inner.changed();
        Ok(Runtime { _control: control, inner, _farewell: Farewell(announce) })
    }
}

impl Runtime {
    /// Start a runtime without pools. The name must be unique among the
    /// runtimes running on this machine.
    pub fn new(name: &str) -> Result<Self> {
        Self::builder(name).start()
    }

    pub fn builder(name: &str) -> Builder {
        Builder { name: name.into(), pools: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Create a node and run it on the thread `placement` names.
    pub fn spawn<N: Node>(&self, name: &str, placement: Placement, params: N::Params) -> Result {
        self.inner.spawn(name, "", placement, params, N::new)
    }

    /// Offer nodes of type `N` under the name `kind`. Registering creates
    /// nothing; any program may then ask this runtime to spawn one.
    pub fn register<N: Node>(&self, kind: &str) -> Result {
        self.register_with(kind, |_, params| N::new(params))
    }

    /// Like [`Runtime::register`], for nodes that are handed something at
    /// birth that settings cannot carry. `make` is given the name and the
    /// settings of each node asked for, and whatever else it holds: for
    /// example a place where the nodes leave what the application's window
    /// is to draw.
    pub fn register_with<N: Node>(
        &self,
        kind: &str,
        make: impl Fn(&str, N::Params) -> Result<N> + Send + Sync + 'static,
    ) -> Result {
        let factory = Factory {
            ports: N::Ports::spec(),
            params: shown(&N::Params::default())?,
            info: Mutex::default(),
            local: false,
            group: None,
            spawn: Box::new({
                let kind = kind.to_owned();
                move |inner, name, placement, params| {
                    // No settings given means the defaults.
                    let params = if params.is_null() { N::Params::default() } else { serde_json::from_value(params)? };
                    inner.spawn(name, &kind, placement, params, |params| make(name, params))
                }
            }),
        };
        self.inner.offer(kind, factory)
    }

    /// Attach free-form information to a factory, shown in the inventory: for
    /// example the devices a driver currently sees.
    pub fn set_factory_info(&self, kind: &str, info: Value) -> Result {
        let state = self.inner.state.lock().unwrap();
        let factory = state.factories.get(kind).ok_or_else(|| Error::Unknown(format!("factory {kind:?}")))?;
        *factory.info.lock().unwrap() = info;
        self.inner.changed();
        Ok(())
    }

    /// Create a node of a registered kind, with its settings as JSON.
    pub fn spawn_kind(&self, kind: &str, name: &str, placement: Placement, params: Value) -> Result {
        self.inner.spawn_kind(kind, name, placement, params)
    }

    /// Create a node whose ports the application uses itself, from its own
    /// threads and its own loop. The node exists until the ports are dropped.
    pub fn attach<P: Ports>(&self, name: &str) -> Result<P> {
        Ok(self.inner.bind(name, "", None, Value::Null, None)?.0)
    }

    /// Make the input `to` receive from the output `from`.
    ///
    /// Names are `node/port` within this runtime or `runtime/node/port`.
    /// `to` must belong to this runtime. Both ports must be there: an output
    /// that no node sends on is refused, as an input that does not exist is.
    /// The connection is to the node that sends now. When that node is
    /// removed, or its program stops, the connection is gone with it.
    pub fn connect(&self, from: &str, to: &str) -> Result {
        self.inner.connect(from, to)
    }

    pub fn disconnect(&self, from: &str, to: &str) -> Result {
        self.inner.disconnect(from, to)
    }

    /// Stop a node and forget it. Its connections go with it: an input that
    /// heard one of its outputs, in any runtime, lets go of that output, and
    /// does not hear a node that takes the name later.
    pub fn remove(&self, node: &str) -> Result {
        self.inner.remove(node)
    }

    /// Stop running a node until [`Runtime::resume`]. Its inputs keep queueing.
    pub fn pause(&self, node: &str) -> Result {
        self.inner.set_paused(node, true)
    }

    pub fn resume(&self, node: &str) -> Result {
        self.inner.set_paused(node, false)
    }

    /// Give a running node new settings, in place of all it has: a setting that `params`
    /// leaves out is its default. To change some and leave the rest, see
    /// [`change_params`](Self::change_params).
    pub fn set_params(&self, node: &str, params: Value) -> Result {
        self.inner.set_params(node, params)
    }

    /// Change some of a running node's settings, and leave the rest as they are.
    ///
    /// `changes` names the settings to change, any number of them: `{"gain": 2}` changes the
    /// gain and nothing else. Settings within settings are named the same way,
    /// `{"limits": {"high": 9}}`. A list is taken whole, and a setting named as `null` is left
    /// out, so that it is its default again if it has one. The node is then given all of its
    /// settings, with the changes in them, exactly as [`set_params`](Self::set_params) would
    /// give them: it checks them as it always does, and if it refuses them nothing has changed.
    ///
    /// The changes are made to the settings as the runtime shows them. A node with a setting
    /// that JSON cannot show, as a number that is not finite, cannot be changed this way.
    pub fn change_params(&self, node: &str, changes: Value) -> Result {
        self.inner.change_params(node, changes)
    }

    /// Record that something is wrong with a node the application runs. It
    /// shows in the inventory as the node's last error. The same problem
    /// reported again is only counted, so it may be reported as often as it happens.
    pub fn report(&self, node: &str, problem: &str) -> Result {
        let mut state = self.inner.state.lock().unwrap();
        let record = state.nodes.get_mut(node).ok_or_else(|| Error::Unknown(format!("node {node:?}")))?;
        if record.hosted.is_some() {
            return Err(Error::Mismatch(format!(
                "node {node:?} is run by the runtime, which reports its errors itself"
            )));
        }
        record.errors += 1;
        let news = record.last_error.as_deref() != Some(problem);
        record.last_error = Some(problem.into());
        drop(state);
        if news {
            self.inner.changed();
        }
        Ok(())
    }

    /// Everything this runtime owns, with live counters.
    pub fn inventory(&self) -> Inventory {
        self.inner.inventory()
    }

    /// Sleep until the program is asked to exit, by [`Runtime::shutdown`] or by another program.
    pub fn wait(&self) {
        let (asked, changed) = &self.inner.shutdown;
        let _asked = changed.wait_while(asked.lock().unwrap(), |asked| !*asked).unwrap();
    }

    pub fn shutdown(&self) {
        self.inner.shut_down();
    }

    /// Whether the program has been asked to exit. A program with its own
    /// loop checks this where others would sleep in [`Runtime::wait`].
    pub fn stopping(&self) -> bool {
        *self.inner.shutdown.0.lock().unwrap()
    }
}

impl Inner {
    /// Record that something a tool would show has changed, and say so.
    pub(crate) fn changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.announce.notify();
    }

    /// Create a node, which `make` builds from its settings, and run it.
    fn spawn<N: Node>(
        self: &Arc<Self>,
        name: &str,
        kind: &str,
        placement: Placement,
        params: N::Params,
        make: impl FnOnce(N::Params) -> Result<N>,
    ) -> Result {
        // Everything that can fail cheaply comes first, so nothing is left behind.
        let executor = self.executor(name, &placement)?;
        let shown = shown(&params)?;
        let node = make(params)?;
        let trigger = node.trigger().checked()?;
        let (ports, inputs) = self.bind::<N::Ports>(name, kind, Some(placement), shown, None)?;
        let slot = Arc::new(Slot::new(node, trigger, ports, inputs));
        match self.state.lock().unwrap().nodes.get_mut(name) {
            Some(record) => record.hosted = Some(Hosted { slot: slot.clone(), executor: executor.clone() }),
            None => {
                return Err(Error::Unknown(format!("node {name:?}: removed while starting")));
            }
        }
        executor.add(slot);
        self.changed();
        Ok(())
    }

    fn offer(&self, kind: &str, factory: Factory) -> Result {
        check_name("factory", kind)?;
        let mut state = self.state.lock().unwrap();
        if state.factories.contains_key(kind) {
            return Err(Error::Name(format!("factory {kind:?} is already registered")));
        }
        state.factories.insert(kind.into(), Arc::new(factory));
        self.changed();
        Ok(())
    }

    pub(crate) fn spawn_kind(self: &Arc<Self>, kind: &str, name: &str, placement: Placement, params: Value) -> Result {
        let factory = self.state.lock().unwrap().factories.get(kind).cloned();
        let factory = factory.ok_or_else(|| Error::Unknown(format!("factory {kind:?} in runtime {:?}", self.name)))?;
        (factory.spawn)(self, name, placement, params)
    }

    /// The thread a new node will run on.
    fn executor(&self, node: &str, placement: &Placement) -> Result<Arc<Executor>> {
        match placement {
            Placement::Dedicated => {
                check_name("node", node)?;
                Ok(Arc::new(Executor::start(&self.ipc.lock().unwrap(), &self.name, &format!("node-{node}"), 0)?))
            }
            Placement::Pool(pool) => {
                let pool = self.pools.get(pool).ok_or_else(|| Error::Unknown(format!("pool {pool:?}")))?;
                Ok(pool.executors.iter().min_by_key(|executor| executor.load()).expect("a pool has threads").clone())
            }
        }
    }

    /// Create the ports of a new node and record them.
    fn bind<P: Ports>(
        self: &Arc<Self>,
        node: &str,
        kind: &str,
        placement: Option<Placement>,
        params: Value,
        notify: Option<Notify>,
    ) -> Result<(P, Vec<Arc<dyn InputPort>>)> {
        check_name("node", node)?;
        let id = {
            let mut state = self.state.lock().unwrap();
            if state.nodes.contains_key(node) {
                return Err(Error::Name(format!("node {node:?} already exists in runtime {:?}", self.name)));
            }
            state.next_id += 1;
            let id = state.next_id;
            // Reserves the name while the ports are being created.
            let (inputs, outputs) = (Vec::new(), Vec::new());
            let record = Record {
                id,
                kind: kind.into(),
                group: String::new(),
                placement,
                params,
                params_lock: Arc::default(),
                inputs,
                outputs,
                hosted: None,
                notify,
                paused: false,
                errors: 0,
                last_error: None,
            };
            state.nodes.insert(node.into(), record);
            id
        };
        let ipc = self.ipc.lock().unwrap();
        let mut binder = Binder {
            ipc: &ipc,
            runtime: &self.name,
            node,
            guard: Arc::new(NodeGuard { runtime: Arc::downgrade(self), node: node.into(), id }),
            inputs: Vec::new(),
            outputs: Vec::new(),
        };
        // On failure the binder is dropped, and its guard removes the reservation.
        let ports = P::bind(&mut binder)?;
        let inputs = binder.inputs.iter().map(|(_, input)| input.clone()).collect();
        if let Some(record) = self.state.lock().unwrap().nodes.get_mut(node) {
            record.inputs = binder.inputs;
            record.outputs = binder.outputs;
        }
        drop(ipc);
        self.changed();
        Ok((ports, inputs))
    }

    /// Called when the last port handle of a node is dropped.
    pub(crate) fn forget(&self, node: &str, id: u64) {
        let mut state = self.state.lock().unwrap();
        let record = match state.nodes.get(node) {
            Some(record) if record.id == id => state.nodes.remove(node),
            _ => None,
        };
        drop(state);
        if record.is_some() {
            self.changed();
        }
        drop(record);
    }

    pub(crate) fn remove(&self, node: &str) -> Result {
        let record = self.state.lock().unwrap().nodes.remove(node);
        let record = record.ok_or_else(|| Error::Unknown(format!("node {node:?} in runtime {:?}", self.name)))?;
        if let Some(hosted) = &record.hosted {
            // A run already handed to a worker thread becomes a no-op.
            hosted.slot.paused.store(true, Ordering::Release);
            hosted.executor.remove(&hosted.slot);
        }
        if let Some(notify) = &record.notify {
            let _ = notify(node, Notice::Removed);
        }
        // Dropping the record closes the node's ports, unless the application holds them.
        drop(record);
        self.changed();
        Ok(())
    }

    pub(crate) fn connect(&self, from: &str, to: &str) -> Result {
        let output = self.absolute(from)?;
        let (input, executor) = self.input(to)?;
        input.connect(&self.ipc.lock().unwrap(), &output)?;
        executor.inspect(|executor| _ = executor.refresh());
        self.changed();
        Ok(())
    }

    pub(crate) fn disconnect(&self, from: &str, to: &str) -> Result {
        let output = self.absolute(from)?;
        let (input, executor) = self.input(to)?;
        input.disconnect(&output);
        executor.inspect(|executor| _ = executor.refresh());
        self.changed();
        Ok(())
    }

    /// Have every input let go of the outputs that are gone, and let the inputs' threads know.
    pub(crate) fn drop_gone(&self) {
        let inputs: Vec<Wired> = {
            let state = self.state.lock().unwrap();
            let per_node = |record: &Record| {
                let executor = record.hosted.as_ref().map(|hosted| hosted.executor.clone());
                record.inputs.iter().map(move |(_, input)| (input.clone(), executor.clone())).collect::<Vec<_>>()
            };
            state.nodes.values().flat_map(per_node).collect()
        };
        for (input, executor) in inputs {
            if input.drop_gone() {
                executor.inspect(|executor| _ = executor.refresh());
                self.changed();
            }
        }
    }

    pub(crate) fn set_paused(&self, node: &str, paused: bool) -> Result {
        let hosted = self.hosted(node)?;
        let notify = self.state.lock().unwrap().nodes.get(node).and_then(|record| record.notify.clone());
        match (hosted, notify) {
            (Some(hosted), _) => {
                hosted.slot.paused.store(paused, Ordering::Release);
                // On resume the thread looks at the node again and runs it for what queued up.
                hosted.executor.refresh();
            }
            // What a pause means for a node the application runs is up to the application.
            (None, Some(notify)) => {
                notify(node, Notice::Paused(paused))?;
                if let Some(record) = self.state.lock().unwrap().nodes.get_mut(node) {
                    record.paused = paused;
                }
            }
            (None, None) => {
                return Err(Error::Mismatch(format!("node {node:?} is held by the application, which is not asked")));
            }
        }
        self.changed();
        Ok(())
    }

    pub(crate) fn set_params(&self, node: &str, params: Value) -> Result {
        self.give_params(node, |_| params)
    }

    pub(crate) fn change_params(&self, node: &str, changes: Value) -> Result {
        self.give_params(node, |params| merged(params, changes))
    }

    /// Give a node the settings that `new` makes of the ones it has.
    fn give_params(&self, node: &str, new: impl FnOnce(Value) -> Value) -> Result {
        let unknown = || Error::Unknown(format!("node {node:?} in runtime {:?}", self.name));
        let lock = self.state.lock().unwrap().nodes.get(node).ok_or_else(unknown)?.params_lock.clone();
        // One set at a time for a node: a change worked out from its settings as they were
        // before another change, and made after it, would undo that other change. Each node
        // has a lock of its own, so that a node may change another's settings as it runs.
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        // The node may have gone while the lock was waited for, and another have its name: that
        // one has a lock of its own, and is not this node.
        let this = |record: &Record| Arc::ptr_eq(&record.params_lock, &lock);
        let (params, hosted, notify) = {
            let state = self.state.lock().unwrap();
            let record = state.nodes.get(node).filter(|record| this(record)).ok_or_else(unknown)?;
            (record.params.clone(), record.hosted.clone(), record.notify.clone())
        };
        let shown = match (&hosted, notify) {
            (Some(hosted), _) => hosted.slot.set_params(new(params))?,
            (None, Some(notify)) => notify(node, Notice::Params(new(params)))?,
            (None, None) => {
                return Err(Error::Mismatch(format!("node {node:?} takes no parameters")));
            }
        };
        // Nor are the settings it took those of a node that has its name by now.
        if let Some(record) = self.state.lock().unwrap().nodes.get_mut(node).filter(|record| this(record)) {
            record.params = shown;
        }
        // The node's trigger may have changed.
        hosted.inspect(|hosted| _ = hosted.executor.refresh());
        self.changed();
        Ok(())
    }

    /// How the runtime runs a node; `None` if the application does.
    /// The numbers inside what an output of this runtime sends.
    pub(crate) fn layout(&self, node: &str, port: &str) -> Result<Layout> {
        let state = self.state.lock().unwrap();
        let outputs = state.nodes.get(node).into_iter().flat_map(|record| &record.outputs);
        let output = outputs.into_iter().find(|(spec, _)| spec.name == port);
        let unknown = || Error::Unknown(format!("output {node}/{port} in runtime {:?}", self.name));
        output.map(|(_, output)| output.layout()).ok_or_else(unknown)
    }

    fn hosted(&self, node: &str) -> Result<Option<Hosted>> {
        let state = self.state.lock().unwrap();
        let record =
            state.nodes.get(node).ok_or_else(|| Error::Unknown(format!("node {node:?} in runtime {:?}", self.name)))?;
        Ok(record.hosted.clone())
    }

    pub(crate) fn shut_down(&self) {
        *self.shutdown.0.lock().unwrap() = true;
        self.shutdown.1.notify_all();
    }

    /// `node/port` in this runtime, or `runtime/node/port` anywhere, as a full output name.
    fn absolute(&self, output: &str) -> Result<String> {
        match output.split('/').count() {
            2 => Ok(format!("{}/{output}", self.name)),
            3 => Ok(output.into()),
            _ => Err(Error::Name(format!("{output:?} is not `node/port` or `runtime/node/port`"))),
        }
    }

    /// Look up an input of this runtime by `node/port` or `runtime/node/port`.
    fn input(&self, name: &str) -> Result<Wired> {
        let unknown = || Error::Unknown(format!("input {name:?} in runtime {:?}", self.name));
        let parts: Vec<&str> = name.split('/').collect();
        let (node, port) = match parts[..] {
            [node, port] => (node, port),
            [runtime, node, port] if runtime == self.name => (node, port),
            _ => return Err(unknown()),
        };
        let state = self.state.lock().unwrap();
        let record = state.nodes.get(node).ok_or_else(unknown)?;
        let (_, input) = record.inputs.iter().find(|(spec, _)| spec.name == port).ok_or_else(unknown)?;
        Ok((input.clone(), record.hosted.as_ref().map(|hosted| hosted.executor.clone())))
    }

    pub(crate) fn inventory(&self) -> Inventory {
        // Describing a node asks iceoryx2 about other processes, so it is done
        // on a copy of the records, not under the lock.
        let (nodes, factories) = {
            let state = self.state.lock().unwrap();
            (state.nodes.clone(), state.factories.clone())
        };
        let pools = self.pools.iter();
        let factories = factories.iter();
        Inventory {
            runtime: self.name.clone(),
            pid: std::process::id(),
            revision: self.revision.load(Ordering::Acquire),
            taken_ns: now_ns(),
            pools: pools
                .map(|(name, pool)| PoolInfo { name: name.clone(), threads: pool.threads, shared: pool.shared })
                .collect(),
            factories: factories
                .map(|(kind, factory)| FactoryInfo {
                    kind: kind.clone(),
                    ports: factory.ports.clone(),
                    params: factory.params.clone(),
                    local: factory.local,
                    info: factory.info.lock().unwrap().clone(),
                    group: factory.group.clone(),
                })
                .collect(),
            nodes: nodes.iter().map(|(name, record)| record.info(name)).collect(),
        }
    }
}

impl Record {
    fn info(&self, name: &str) -> NodeInfo {
        let slot = self.hosted.as_ref().map(|hosted| &hosted.slot);
        let paused = slot.map_or(self.paused, |slot| slot.paused.load(Ordering::Acquire));
        let state = match slot.and_then(|slot| slot.stopped.get()) {
            Some(panic) => NodeState::Stopped(panic.clone()),
            None if paused => NodeState::Paused,
            None => NodeState::Running,
        };
        NodeInfo {
            name: name.into(),
            kind: self.kind.clone(),
            group: self.group.clone(),
            placement: self.placement.clone(),
            state,
            params: self.params.clone(),
            runs: slot.map_or(0, |slot| slot.runs.load(Ordering::Relaxed)),
            errors: slot.map_or(self.errors, |slot| slot.errors.load(Ordering::Relaxed)),
            last_error: slot.map_or(self.last_error.clone(), |slot| slot.last_error.lock().unwrap().clone()),
            inputs: self
                .inputs
                .iter()
                .map(|(spec, input)| InputInfo { port: spec.clone(), sources: input.info() })
                .collect(),
            outputs: self.outputs.iter().map(|(spec, output)| output.info(spec)).collect(),
        }
    }
}

/// Names become parts of iceoryx2 service names, so they stay short and plain.
pub(crate) fn check_name(what: &str, name: &str) -> Result {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    if (1..=32).contains(&name.len()) && !name.starts_with('_') && name.chars().all(plain) {
        return Ok(());
    }
    Err(Error::Name(format!(
        "{what} name {name:?} must be 1 to 32 letters, digits, '_', '-' or '.', and not start with '_'"
    )))
}
