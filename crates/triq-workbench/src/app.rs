//! The window: the machine on the left, the graph in the middle, details on the right.
//!
//! Nothing drawn here is the truth. Every frame the drawing is brought in line
//! with what the runtimes last reported, and everything the user does becomes
//! a request to a runtime. A wire appears when its owner says it exists.
use crate::feed::{Feed, Request};
use crate::graph::{self, Card};
use eframe::egui::{self, Color32, Rect, Stroke, Ui, emath::TSTransform};
use egui_snarl::ui::{PinInfo, PinPlacement, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireStyle};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use serde_json::Value;
use triq::{FactoryInfo, Inventory, NodeState, Placement, PortSpec};

pub struct Workbench {
    feed: Feed,
    snarl: Snarl<Card>,
    focus: Focus,
    /// Bring every card into view on the next frame that has any.
    fit: bool,
}

/// What the right-hand panel is about.
enum Focus {
    Nothing,
    /// A node, by `runtime/node`, with its settings as the user is editing them.
    Node {
        key: String,
        settings: String,
    },
    /// A node about to be created.
    Form(Form),
}

struct Form {
    runtime: String,
    kind: String,
    name: String,
    /// `None` for a thread of its own.
    pool: Option<String>,
    settings: String,
}

impl Workbench {
    pub fn new(feed: Feed) -> Self {
        Self { feed, snarl: Snarl::new(), focus: Focus::Nothing, fit: true }
    }

    /// The drawing: one card per node, one wire per live connection.
    pub fn graph(&self) -> &Snarl<Card> {
        &self.snarl
    }

    /// Ask for a connection, as dragging a wire from an output pin to an input pin does.
    pub fn wire(&mut self, from: OutPinId, to: InPinId) {
        wire(&self.feed, &self.snarl, from, to);
    }

    /// Ask for a connection to be removed.
    pub fn unwire(&mut self, from: OutPinId, to: InPinId) {
        unwire(&self.feed, &self.snarl, from, to);
    }

    /// Draw one frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let inventories = self.feed.inventories();
        graph::sync(&mut self.snarl, &inventories);
        if let Focus::Node { key, .. } = &self.focus
            && !self.snarl.nodes().any(|card| card.key() == *key)
        {
            self.focus = Focus::Nothing;
        }

        egui::Panel::top("status").show(ui, |ui| self.status(ui, &inventories));
        egui::Panel::left("machine").default_size(230.0).show(ui, |ui| self.machine(ui, &inventories));
        egui::Panel::right("details").default_size(330.0).show(ui, |ui| self.details(ui, &inventories));
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui, &inventories));
    }

    fn status(&mut self, ui: &mut Ui, inventories: &[Inventory]) {
        ui.horizontal(|ui| {
            let nodes: usize = inventories.iter().map(|inventory| inventory.nodes.len()).sum();
            ui.weak(format!("{} runtimes, {nodes} nodes", inventories.len()));
            if ui.small_button("Tidy").on_hover_text("Lay the nodes out again, by what feeds what").clicked() {
                graph::tidy(&mut self.snarl);
                self.fit = true;
            }
            self.fit |= ui.small_button("Fit").on_hover_text("Bring every node into view").clicked();
            if let Some(problem) = self.feed.problem() {
                ui.colored_label(ui.visuals().error_fg_color, problem);
                if ui.small_button("Dismiss").clicked() {
                    self.feed.dismiss();
                }
            }
        });
    }

    /// Every runtime with its pools, the kinds of node it offers, and its nodes.
    fn machine(&mut self, ui: &mut Ui, inventories: &[Inventory]) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if inventories.is_empty() {
                ui.weak("No runtime is running on this machine.");
            }
            for inventory in inventories {
                ui.strong(&inventory.runtime);
                for pool in &inventory.pools {
                    let shared = if pool.shared { ", shared" } else { "" };
                    ui.weak(format!("pool {}: {} threads{shared}", pool.name, pool.threads));
                }
                for factory in &inventory.factories {
                    if ui.button(format!("New {}", factory.kind)).clicked() {
                        self.focus = Focus::Form(Form::new(inventory, factory));
                    }
                }
                for node in &inventory.nodes {
                    let key = format!("{}/{}", inventory.runtime, node.name);
                    let focused = matches!(&self.focus, Focus::Node { key: focus, .. } if *focus == key);
                    if ui.selectable_label(focused, &node.name).clicked() {
                        self.focus = Focus::node(key, &node.params);
                    }
                }
                ui.separator();
            }
        });
    }

    fn details(&mut self, ui: &mut Ui, inventories: &[Inventory]) {
        egui::ScrollArea::vertical().show(ui, |ui| match &mut self.focus {
            Focus::Nothing => {
                ui.weak("Select a node to see it here, or pick a kind of node on the left to create one.");
                ui.weak("Drag from an output to an input to connect them. Right-click a pin to disconnect it.");
            }
            Focus::Node { key, settings } => {
                if let Some(card) = self.snarl.nodes().find(|card| card.key() == *key) {
                    node_details(ui, &self.feed, card, settings);
                }
            }
            Focus::Form(form) => {
                if form.show(ui, &self.feed, inventories) {
                    self.focus = Focus::Nothing;
                }
            }
        });
    }

    fn canvas(&mut self, ui: &mut Ui, inventories: &[Inventory]) {
        let style = SnarlStyle {
            collapsible: Some(false),
            pin_size: Some(11.0),
            pin_placement: Some(PinPlacement::Edge),
            wire_width: Some(2.5),
            wire_style: Some(WireStyle::Bezier5),
            ..SnarlStyle::new()
        };
        // The first cards to appear are framed, and again whenever the user asks.
        let frame = (self.fit && self.snarl.nodes().next().is_some()).then(|| framing(&self.snarl, ui.max_rect()));
        self.fit = frame.is_none();
        let mut viewer = Viewer { feed: &self.feed, focus: &mut self.focus, inventories, frame };
        SnarlWidget::new().id_salt("triq-graph").style(style).show(&mut self.snarl, &mut viewer, ui);
    }
}

impl eframe::App for Workbench {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl Focus {
    fn node(key: String, params: &Value) -> Self {
        Self::Node { key, settings: pretty(params) }
    }
}

impl Form {
    fn new(inventory: &Inventory, factory: &FactoryInfo) -> Self {
        // The first name of the form `kind1`, `kind2`, ... that is free in that runtime.
        let taken = |name: &String| inventory.nodes.iter().any(|node| node.name == *name);
        let name = (1..).map(|number| format!("{}{number}", factory.kind)).find(|name| !taken(name));
        Self {
            runtime: inventory.runtime.clone(),
            kind: factory.kind.clone(),
            name: name.unwrap_or_default(),
            pool: None,
            settings: pretty(&factory.params),
        }
    }

    /// Returns true when the form is done with, created or cancelled.
    fn show(&mut self, ui: &mut Ui, feed: &Feed, inventories: &[Inventory]) -> bool {
        let offered = inventories.iter().find(|inventory| inventory.runtime == self.runtime).and_then(|inventory| {
            let factory = inventory.factories.iter().find(|factory| factory.kind == self.kind)?;
            Some((inventory, factory))
        });
        let Some((inventory, factory)) = offered else {
            return true; // the runtime or its factory went away
        };
        ui.heading(format!("Create a {} node", self.kind));
        ui.weak(format!("in {}", self.runtime));
        ui.add_space(8.0);

        let name = ui.label("Name");
        ui.text_edit_singleline(&mut self.name).labelled_by(name.id);
        if factory.local {
            ui.weak("The application that offers this kind runs its nodes itself.");
        } else {
            ui.label("Runs on");
            let chosen = self.pool.as_ref().map_or("a thread of its own".to_owned(), |pool| format!("pool {pool}"));
            egui::ComboBox::from_id_salt("runs-on").selected_text(chosen).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.pool, None, "a thread of its own");
                for pool in &inventory.pools {
                    ui.selectable_value(&mut self.pool, Some(pool.name.clone()), format!("pool {}", pool.name));
                }
            });
        }
        ui.label("Settings");
        ui.add(egui::TextEdit::multiline(&mut self.settings).code_editor().desired_rows(4));
        let params = serde_json::from_str::<Value>(&self.settings);
        if let Err(error) = &params {
            ui.colored_label(ui.visuals().error_fg_color, format!("Not valid JSON: {error}"));
        }
        if !factory.info.is_null() {
            ui.label("From the factory");
            ui.monospace(pretty(&factory.info));
        }

        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            let ready = params.is_ok() && !self.name.is_empty();
            if ui.add_enabled(ready, egui::Button::new("Create")).clicked()
                && let Ok(params) = params
            {
                let placement = self.pool.as_deref().map_or(Placement::Dedicated, Placement::pool);
                let (runtime, kind, name) = (self.runtime.clone(), self.kind.clone(), self.name.clone());
                feed.ask(Request::Spawn { runtime, kind, name, placement, params });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
    }
}

fn node_details(ui: &mut Ui, feed: &Feed, card: &Card, settings: &mut String) {
    let node = &card.node;
    ui.heading(&node.name);
    ui.weak(format!("in {}", card.runtime));
    ui.add_space(8.0);
    egui::Grid::new("facts").num_columns(2).show(ui, |ui| {
        let mut fact = |name: &str, value: String| {
            ui.weak(name);
            ui.label(value);
            ui.end_row();
        };
        if !node.kind.is_empty() {
            fact("Kind", node.kind.clone());
        }
        fact("Runs on", runs_on(&node.placement));
        fact("State", state(&node.state));
        if node.placement.is_some() {
            fact("Runs", node.runs.to_string());
        }
        if let Some(error) = &node.last_error {
            fact("Errors", format!("{}, last: {error}", node.errors));
        }
    });
    ui.horizontal(|ui| actions(ui, feed, card));

    if !node.params.is_null() {
        ui.separator();
        ui.strong("Settings");
        ui.add(egui::TextEdit::multiline(settings).code_editor().desired_rows(3));
        match serde_json::from_str::<Value>(settings) {
            Ok(params) => {
                if ui.button("Apply").clicked() {
                    feed.ask(Request::SetParams { runtime: card.runtime.clone(), node: node.name.clone(), params });
                }
            }
            Err(error) => _ = ui.colored_label(ui.visuals().error_fg_color, format!("Not valid JSON: {error}")),
        }
    }

    for (index, input) in node.inputs.iter().enumerate() {
        ui.separator();
        ui.label(port(&input.port));
        if input.sources.is_empty() {
            ui.weak("not connected");
        }
        for source in &input.sources {
            let link = match (&source.problem, source.connected) {
                (Some(problem), _) => problem.clone(),
                (None, true) => format!("{} received, {} lost, {} µs", source.received, source.lost, source.latency_us),
                (None, false) => "waiting for the output".to_owned(),
            };
            ui.weak(format!("from {}", source.output));
            ui.horizontal(|ui| {
                ui.weak(link);
                if ui.small_button("Disconnect").clicked() {
                    feed.ask(Request::Disconnect { from: source.output.clone(), to: card.input(index) });
                }
            });
        }
    }
    for output in &node.outputs {
        ui.separator();
        ui.label(port(&output.port));
        ui.weak(format!("{} sent to {} inputs", output.sent, output.subscribers));
        if output.undelivered > 0 {
            ui.weak(format!("{} could not be delivered", output.undelivered));
        }
    }
}

/// The buttons that act on a node. True if one was pressed.
fn actions(ui: &mut Ui, feed: &Feed, card: &Card) -> bool {
    let (runtime, node) = (card.runtime.clone(), card.node.name.clone());
    // Only nodes that a runtime runs can be paused; an application runs its own as it likes.
    let toggle = match (&card.node.placement, &card.node.state) {
        (Some(_), NodeState::Running) => {
            Some(("Pause", Request::Pause { runtime: runtime.clone(), node: node.clone() }))
        }
        (Some(_), NodeState::Paused) => {
            Some(("Resume", Request::Resume { runtime: runtime.clone(), node: node.clone() }))
        }
        _ => None,
    };
    let mut pressed = false;
    if let Some((label, request)) = toggle
        && ui.button(label).clicked()
    {
        feed.ask(request);
        pressed = true;
    }
    if ui.button("Remove").clicked() {
        feed.ask(Request::Remove { runtime, node });
        pressed = true;
    }
    pressed
}

/// The view that shows every card, no larger than life.
fn framing(snarl: &Snarl<Card>, canvas: Rect) -> TSTransform {
    let card = |(pos, card): (egui::Pos2, &Card)| Rect::from_min_size(pos, graph::size(card));
    let cards = snarl.nodes_pos().map(card).fold(Rect::NOTHING, Rect::union).expand(30.0);
    let scale = (canvas.width() / cards.width()).min(canvas.height() / cards.height()).clamp(0.2, 1.0);
    TSTransform::new(canvas.center().to_vec2() - cards.center().to_vec2() * scale, scale)
}

fn wire(feed: &Feed, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node], &snarl[to.node]);
    let (output, input) = (&source.node.outputs[from.output].port, &sink.node.inputs[to.input].port);
    if !graph::compatible(output, input) {
        feed.report(format!(
            "{} and {} carry different messages: {} and {}",
            source.output(from.output),
            sink.input(to.input),
            message(output),
            message(input)
        ));
        return;
    }
    feed.ask(Request::Connect { from: source.output(from.output).to_owned(), to: sink.input(to.input) });
}

fn unwire(feed: &Feed, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node], &snarl[to.node]);
    feed.ask(Request::Disconnect { from: source.output(from.output).to_owned(), to: sink.input(to.input) });
}

/// How the graph widget asks what to draw and reports what the user did.
struct Viewer<'a> {
    feed: &'a Feed,
    focus: &'a mut Focus,
    inventories: &'a [Inventory],
    /// A view to switch to this frame.
    frame: Option<TSTransform>,
}

impl SnarlViewer<Card> for Viewer<'_> {
    fn title(&mut self, card: &Card) -> String {
        card.node.name.clone()
    }

    fn current_transform(&mut self, to_global: &mut TSTransform, _: &mut Snarl<Card>) {
        if let Some(frame) = self.frame.take() {
            *to_global = frame;
        }
    }

    fn show_header(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<Card>) {
        let card = &snarl[node];
        ui.horizontal(|ui| {
            ui.strong(&card.node.name);
            ui.weak(format!("{} · {}", card.runtime, state(&card.node.state)));
        });
    }

    fn node_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        snarl: &Snarl<Card>,
    ) -> egui::Frame {
        match &*self.focus {
            Focus::Node { key, .. } if *key == snarl[node].key() => {
                default.stroke(Stroke::new(2.0, Color32::from_rgb(90, 160, 250)))
            }
            _ => default,
        }
    }

    fn inputs(&mut self, card: &Card) -> usize {
        card.node.inputs.len()
    }

    fn outputs(&mut self, card: &Card) -> usize {
        card.node.outputs.len()
    }

    // A pin gets one short row; the counters are a hover away and in the details panel.
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        let input = &card.node.inputs[pin.id.input];
        let received: u64 = input.sources.iter().map(|source| source.received).sum();
        let lost: u64 = input.sources.iter().map(|source| source.lost).sum();
        ui.label(port(&input.port)).on_hover_text(format!("{received} received, {lost} lost"));
        let waiting = card.waiting(pin.id.input);
        if !waiting.is_empty() {
            ui.weak("waiting").on_hover_text(format!("waiting for {}", waiting.join(", ")));
        }
        PinInfo::circle().with_fill(graph::color(&input.port.message))
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let output = &snarl[pin.id.node].node.outputs[pin.id.output];
        ui.label(port(&output.port)).on_hover_text(format!("{} sent to {} inputs", output.sent, output.subscribers));
        PinInfo::circle().with_fill(graph::color(&output.port.message))
    }

    /// A wire being dragged only snaps to an input it could be connected to.
    fn can_snap(&mut self, from: &OutPin, to: &InPin, snarl: &Snarl<Card>) -> bool {
        let output = &snarl[from.id.node].node.outputs[from.id.output].port;
        let input = &snarl[to.id.node].node.inputs[to.id.input].port;
        graph::compatible(output, input)
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Card>) {
        wire(self.feed, snarl, from.id, to.id);
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Card>) {
        unwire(self.feed, snarl, from.id, to.id);
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<Card>) {
        pin.remotes.iter().for_each(|from| unwire(self.feed, snarl, *from, pin.id));
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<Card>) {
        pin.remotes.iter().for_each(|to| unwire(self.feed, snarl, pin.id, *to));
    }

    fn node_clicked(&mut self, node: NodeId, snarl: &Snarl<Card>) {
        let card = &snarl[node];
        *self.focus = Focus::node(card.key(), &card.node.params);
    }

    fn has_node_menu(&mut self, _: &Card) -> bool {
        true
    }

    fn show_node_menu(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<Card>) {
        if actions(ui, self.feed, &snarl[node]) {
            ui.close();
        }
    }

    fn has_graph_menu(&mut self, _: egui::Pos2, _: &mut Snarl<Card>) -> bool {
        self.inventories.iter().any(|inventory| !inventory.factories.is_empty())
    }

    /// Right-clicking the background offers every kind of node any runtime can create.
    fn show_graph_menu(&mut self, _: egui::Pos2, ui: &mut Ui, _: &mut Snarl<Card>) {
        for inventory in self.inventories {
            for factory in &inventory.factories {
                if ui.button(format!("New {} in {}", factory.kind, inventory.runtime)).clicked() {
                    *self.focus = Focus::Form(Form::new(inventory, factory));
                    ui.close();
                }
            }
        }
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// `name: Type`, as written beside a pin.
fn port(spec: &PortSpec) -> String {
    format!("{}: {}", spec.name, message(spec))
}

fn message(spec: &PortSpec) -> String {
    format!("{}{}", spec.message.name, if spec.message.slice { "[]" } else { "" })
}

fn state(state: &NodeState) -> String {
    match state {
        NodeState::Running => "running".to_owned(),
        NodeState::Paused => "paused".to_owned(),
        NodeState::Stopped(panic) => format!("stopped by a panic: {panic}"),
    }
}

fn runs_on(placement: &Option<Placement>) -> String {
    match placement {
        None => "the application that holds it".to_owned(),
        Some(Placement::Dedicated) => "a thread of its own".to_owned(),
        Some(Placement::Pool(pool)) => format!("pool {pool}"),
    }
}
