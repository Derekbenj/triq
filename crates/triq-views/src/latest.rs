//! A kind of view in one call, [`Latest::new`], for small messages.
//!
//! All there is to say about such a view is which message it listens to and
//! how the newest one is drawn. The rest is the same every time, and is here:
//! the kind is offered on the runtime as nodes with one input, called
//! `input`; the window runs those nodes itself, and reads each input as it
//! draws. That is right for a message that is cheap to read and to draw. A
//! large one, a camera's frame, wants a node that the runtime runs on a thread
//! of its own, and a [`Kind`] written for it.
use crate::Kind;
use egui::Ui;
use std::time::Duration;
use triq::prelude::*;
use triq::{LocalNodes, Payload};

/// Messages arrive whether or not anything else on screen changes, so a
/// window that draws a view is drawn again this soon.
const REFRESH: Duration = Duration::from_millis(16);

/// What a view of a [`Latest`] has for ports: one input, for the message it draws.
#[derive(Ports)]
pub struct Shown<M: Payload> {
    pub input: Input<M>,
}

/// How a message is drawn.
type Draw<M> = Box<dyn FnMut(&mut Ui, &M)>;

/// A kind of view that draws the newest message each of its views received.
pub struct Latest<M: Payload + Clone> {
    /// The views the window runs itself, each with the newest message it received, once there is one.
    views: LocalNodes<Shown<M>, (), Option<M>>,
    draw: Draw<M>,
}

impl<M: Payload + Clone> Latest<M> {
    /// Offer a kind of view on `runtime`, called `kind`, whose views draw the
    /// newest message of type `M` they received with `draw`.
    pub fn new(runtime: &Runtime, kind: &str, draw: impl FnMut(&mut Ui, &M) + 'static) -> triq::Result<Self> {
        Ok(Self { views: runtime.local_nodes(kind)?, draw: Box::new(draw) })
    }
}

impl<M: Payload + Clone> Kind for Latest<M> {
    /// Before it answers, it does what other programs asked for since the
    /// last frame: views created, paused, removed.
    fn names(&mut self) -> Vec<String> {
        self.views.serve();
        self.views.nodes.keys().cloned().collect()
    }

    fn show(&mut self, ui: &mut Ui, name: &str) {
        let Some(view) = self.views.nodes.get_mut(name) else { return };
        ui.ctx().request_repaint_after(REFRESH);
        // Only the newest message is worth drawing. A paused view reads nothing and keeps what it shows.
        match (!view.paused).then(|| view.ports.input.latest()) {
            Some(Ok(Some(message))) => view.state = Some(M::clone(&message)),
            Some(Err(error)) => _ = ui.colored_label(ui.visuals().error_fg_color, error.to_string()),
            _ => {}
        }
        if view.paused {
            ui.weak("paused");
        }
        match &view.state {
            Some(message) => (self.draw)(ui, message),
            None => _ = ui.weak("nothing received yet"),
        }
    }
}
