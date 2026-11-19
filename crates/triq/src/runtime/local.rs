//! Nodes the application runs itself, created on request.
//!
//! This is how a program that owns its main thread, such as a renderer or a
//! device driver, offers nodes made of things only that thread may touch.
//! The rest of the runtime does not depend on this file.
use super::{Factory, Inner, Notice, Notify, Runtime};
use crate::inventory::shown;
use crate::{Error, Ports, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak, mpsc};

/// The application's end of a local factory, returned by [`Runtime::local_factory`].
/// Call [`LocalFactory::try_next`] from the application's own loop.
/// Dropping it withdraws the factory.
pub struct LocalFactory<P: Ports, A> {
    requests: mpsc::Receiver<Local<P, A>>,
    runtime: Weak<Inner>,
    kind: String,
}

/// What a [`LocalFactory`] asks of the application.
pub enum Local<P, A> {
    /// A node was created. Keep `ports`; the node lives as long as they do.
    Spawn {
        name: String,
        params: A,
        ports: P,
    },
    /// New settings for a node.
    Params {
        name: String,
        params: A,
    },
    /// Stop doing the node's work until it is resumed.
    Pause {
        name: String,
    },
    Resume {
        name: String,
    },
    /// The node was removed. Drop its ports.
    Remove {
        name: String,
    },
}

impl<P: Ports, A> LocalFactory<P, A> {
    /// The next request, if there is one. Never waits.
    pub fn try_next(&self) -> Option<Local<P, A>> {
        self.requests.try_recv().ok()
    }
}

impl<P: Ports, A> Drop for LocalFactory<P, A> {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.state.lock().unwrap().factories.remove(&self.kind);
            runtime.changed();
        }
    }
}

/// The nodes of a local factory, kept by name: what most applications that
/// offer one would otherwise write themselves. Returned by [`Runtime::local_nodes`].
///
/// `S` is whatever else the application keeps for each node, such as the last
/// message it drew; a new node starts with the default.
pub struct LocalNodes<P: Ports, A, S = ()> {
    /// The nodes there are. Taking one out drops its ports, which ends the node.
    pub nodes: BTreeMap<String, LocalNode<P, A, S>>,
    factory: LocalFactory<P, A>,
}

/// One node the application runs.
pub struct LocalNode<P, A, S> {
    pub ports: P,
    /// Its settings as they stand.
    pub params: A,
    /// Whether it was asked to leave its work for now. Honouring that is up to the application.
    pub paused: bool,
    pub state: S,
}

impl<P: Ports, A, S: Default> LocalNodes<P, A, S> {
    /// Do what other programs asked for since the last call: add the nodes
    /// they created, note new settings and pauses, let go of the nodes they
    /// removed. Call it from the application's own loop.
    pub fn serve(&mut self) {
        while let Some(request) = self.factory.try_next() {
            match request {
                Local::Spawn { name, params, ports } => {
                    self.nodes.insert(name, LocalNode { ports, params, paused: false, state: S::default() });
                }
                Local::Params { name, params } => self.change(&name, |node| node.params = params),
                Local::Pause { name } => self.change(&name, |node| node.paused = true),
                Local::Resume { name } => self.change(&name, |node| node.paused = false),
                Local::Remove { name } => _ = self.nodes.remove(&name),
            }
        }
    }

    fn change(&mut self, name: &str, change: impl FnOnce(&mut LocalNode<P, A, S>)) {
        if let Some(node) = self.nodes.get_mut(name) {
            change(node);
        }
    }
}

impl Runtime {
    /// Offer nodes that the application itself runs, and keep them by name.
    /// For an application that wants each request as it comes, there is
    /// [`Runtime::local_factory`].
    pub fn local_nodes<P, A, S>(&self, kind: &str) -> Result<LocalNodes<P, A, S>>
    where
        P: Ports,
        A: Serialize + DeserializeOwned + Default + Send + 'static,
    {
        Ok(LocalNodes { nodes: BTreeMap::new(), factory: self.local_factory(kind)? })
    }

    /// Offer nodes that the application itself creates and runs.
    ///
    /// A spawn request binds the node's ports at once and queues them for the
    /// application, so the program asking never waits for the application's loop.
    pub fn local_factory<P, A>(&self, kind: &str) -> Result<LocalFactory<P, A>>
    where
        P: Ports,
        A: Serialize + DeserializeOwned + Default + Send + 'static,
    {
        let (queue, requests) = mpsc::channel();
        let notify: Notify = Arc::new({
            let queue = queue.clone();
            move |name, notice| {
                let name = name.to_owned();
                let (request, shown) = match notice {
                    Notice::Removed => (Local::Remove { name }, Value::Null),
                    Notice::Paused(true) => (Local::Pause { name }, Value::Null),
                    Notice::Paused(false) => (Local::Resume { name }, Value::Null),
                    Notice::Params(params) => {
                        let params: A = serde_json::from_value(params)?;
                        let shown = shown(&params)?;
                        (Local::Params { name, params }, shown)
                    }
                };
                queue
                    .send(request)
                    .map_err(|_| Error::Unknown("factory: its application stopped serving it".into()))?;
                Ok(shown)
            }
        });
        let factory = Factory {
            ports: P::spec(),
            params: shown(&A::default())?,
            info: Mutex::default(),
            local: true,
            group: None,
            spawn: Box::new({
                let kind = kind.to_owned();
                move |inner, name, _placement, params| {
                    let params: A = if params.is_null() { A::default() } else { serde_json::from_value(params)? };
                    let shown = shown(&params)?;
                    let (ports, _) = inner.bind::<P>(name, &kind, None, shown, Some(notify.clone()))?;
                    // If the application is gone, `ports` is dropped here and the node with it.
                    queue
                        .send(Local::Spawn { name: name.into(), params, ports })
                        .map_err(|_| Error::Unknown(format!("factory {kind:?}: its application stopped serving it")))
                }
            }),
        };
        self.inner.offer(kind, factory)?;
        Ok(LocalFactory { requests, runtime: Arc::downgrade(&self.inner), kind: kind.into() })
    }
}
