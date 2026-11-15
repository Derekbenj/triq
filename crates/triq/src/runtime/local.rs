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

impl Runtime {
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
