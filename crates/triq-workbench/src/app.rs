//! The window. One tab is the graph: the machine on the left, the graph in the
//! middle, details on the right. The other is charts of what connections do over time.
//!
//! Nothing drawn here is the truth. Every frame the drawing is brought in line
//! with what the runtimes last reported, and everything the user does becomes
//! a request to a runtime. A wire appears when its owner says it exists.
//!
//! A node wears the colour of the runtime it runs in: its card, its pins and
//! the wires that leave it.
use crate::charts::Charts;
use crate::colors::Colors;
use crate::feed::{Feed, Request};
use crate::graph::{self, Card};
use eframe::egui::{self, Color32, Rect, Role, Sense, Shape, Stroke, Ui, WidgetInfo, emath::TSTransform};
use egui_snarl::ui::{NodeLayout, PinInfo, PinPlacement, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireStyle};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use triq::{FactoryInfo, Inventory, NodeState, Placement, PoolInfo, PortSpec};

pub struct Workbench {
    feed: Feed,
    snarl: Snarl<Card>,
    colors: Colors,
    charts: Charts,
    tab: Tab,
    focus: Focus,
    /// Bring every card into view on the next frame that has any.
    fit: bool,
}

/// How far behind the present the right edge of a chart is, in seconds. The
/// machine is read ten times a second, so the newest point is up to a tenth
/// of a second old, and a little more by the time it has been fetched. Drawn
/// this far behind, a line always has a point beyond the edge to run on to.
const BEHIND: f64 = 0.15;

/// What fills the window under the top row.
#[derive(PartialEq)]
enum Tab {
    Graph,
    Charts,
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
        Self {
            feed,
            snarl: Snarl::new(),
            colors: Colors::default(),
            charts: Charts::default(),
            tab: Tab::Graph,
            focus: Focus::Nothing,
            fit: true,
        }
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
        self.colors.update(inventories.iter().map(|inventory| inventory.runtime.as_str()));
        if let Focus::Node { key, .. } = &self.focus
            && !self.snarl.nodes().any(|card| card.key() == *key)
        {
            self.focus = Focus::Nothing;
        }

        egui::Panel::top("status").show(ui, |ui| self.status(ui, &inventories));
        if self.tab == Tab::Charts {
            // A chart ends a moment before the present, and the present moves on. While
            // there is a line to slide along, the window is drawn again as soon as it can be.
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
            if !self.charts.is_empty() {
                ui.ctx().request_repaint();
            }
            return self.charts.show(ui, &self.feed.history(), now - BEHIND);
        }
        egui::Panel::left("machine").default_size(230.0).show(ui, |ui| self.machine(ui, &inventories));
        let rates = self.feed.rates();
        egui::Panel::right("details").default_size(330.0).show(ui, |ui| self.details(ui, &inventories, &rates));
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui, &inventories, &rates));
    }

    /// The top row: the tabs, and what goes with the one that is open.
    fn status(&mut self, ui: &mut Ui, inventories: &[Inventory]) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Graph, "Graph");
            ui.selectable_value(&mut self.tab, Tab::Charts, "Charts");
            ui.separator();
            if self.tab == Tab::Charts {
                self.charts.controls(ui);
            } else {
                let nodes: usize = inventories.iter().map(|inventory| inventory.nodes.len()).sum();
                ui.weak(format!("{} runtimes, {nodes} nodes", inventories.len()));
                if ui.small_button("Tidy").on_hover_text("Lay the nodes out again, by what feeds what").clicked() {
                    graph::tidy(&mut self.snarl);
                    self.fit = true;
                }
                self.fit |= ui.small_button("Fit").on_hover_text("Bring every node into view").clicked();
            }
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
                ui.horizontal(|ui| {
                    dot(ui, self.colors.of(&inventory.runtime, ui.visuals().dark_mode), &inventory.runtime);
                    ui.strong(&inventory.runtime);
                    ui.weak(format!("process {}", inventory.pid));
                });
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

    fn details(&mut self, ui: &mut Ui, inventories: &[Inventory], rates: &Rates) {
        egui::ScrollArea::vertical().show(ui, |ui| match &mut self.focus {
            Focus::Nothing => {
                ui.weak("Click a node to see where it runs, or pick a kind of node on the left to create one.");
                ui.weak(
                    "Drag from an output to an input to connect them. Right-click a wire or a pin to disconnect it.",
                );
                ui.weak("Right-click a node to pause or remove it.");
            }
            Focus::Node { key, settings } => {
                let card = self.snarl.nodes().find(|card| card.key() == *key);
                let home =
                    inventories.iter().find(|inventory| card.is_some_and(|card| card.runtime == inventory.runtime));
                if let (Some(card), Some(home)) = (card, home) {
                    node_details(ui, &self.feed, card, home, settings, rates);
                }
            }
            Focus::Form(form) => {
                if form.show(ui, &self.feed, inventories) {
                    self.focus = Focus::Nothing;
                }
            }
        });
    }

    fn canvas(&mut self, ui: &mut Ui, inventories: &[Inventory], rates: &Rates) {
        let style = SnarlStyle {
            collapsible: Some(false),
            header_drag_space: Some(egui::vec2(3.0, 3.0)),
            pin_size: Some(12.0),
            pin_placement: Some(PinPlacement::Edge),
            pin_stroke: Some(Stroke::new(1.6, ui.visuals().strong_text_color())),
            wire_width: Some(3.2),
            wire_smoothness: Some(1.6),
            wire_style: Some(WireStyle::Bezier5),
            node_layout: Some(NodeLayout::coil().with_min_pin_row_height(graph::ROW).with_equal_pin_rows()),
            min_scale: Some(SMALLEST),
            ..SnarlStyle::new()
        };
        // The first cards to appear are framed, and again whenever the user asks.
        let frame = (self.fit && self.snarl.nodes().next().is_some()).then(|| framing(&self.snarl, ui.max_rect()));
        self.fit &= frame.is_none();
        let (feed, colors, dark) = (&self.feed, &self.colors, ui.visuals().dark_mode);
        let mut viewer = Viewer { feed, colors, dark, focus: &mut self.focus, inventories, rates, frame };
        // Text that could be selected would swallow the press that moves, selects or opens the menu of a card.
        ui.style_mut().interaction.selectable_labels = false;
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

/// `home` is what the node's runtime last reported.
fn node_details(ui: &mut Ui, feed: &Feed, card: &Card, home: &Inventory, settings: &mut String, rates: &Rates) {
    let node = &card.node;
    ui.heading(&node.name);
    ui.add_space(8.0);
    egui::Grid::new("facts").num_columns(2).show(ui, |ui| {
        let mut fact = |name: &str, value: String| {
            ui.weak(name);
            ui.label(value);
            ui.end_row();
        };
        fact("Runs in", format!("{}, process {}", home.runtime, home.pid));
        fact("Runs on", runs_on(&node.placement, &home.pools));
        if !node.kind.is_empty() {
            fact("Kind", node.kind.clone());
        }
        fact("State", state(&node.state));
        if node.placement.is_some() {
            fact("Times run", node.runs.to_string());
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
        ui.horizontal(|ui| {
            ui.label(port(&input.port));
            ui.weak(rate(rates, &card.input(index)));
        });
        if input.sources.is_empty() {
            ui.weak("not connected");
        }
        for source in &input.sources {
            let link = match (&source.problem, source.connected) {
                (Some(problem), _) => problem.clone(),
                (None, true) => format!(
                    "{} received, {} lost, {} µs latency, {} µs longest gap",
                    source.received, source.lost, source.latency_us, source.gap_us
                ),
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
        ui.horizontal(|ui| {
            ui.label(port(&output.port));
            ui.weak(rate(rates, &output.service));
        });
        ui.weak(format!("{} sent to {} inputs", output.sent, output.subscribers));
        if output.undelivered > 0 {
            ui.weak(format!("{} could not be delivered", output.undelivered));
        }
    }
}

/// How a node is paused or started again: the word for it and the request.
/// `None` for a node that can be neither.
fn toggle(card: &Card) -> Option<(&'static str, Request)> {
    let (runtime, node) = (card.runtime.clone(), card.node.name.clone());
    // A node that came from a factory can be paused: by its runtime, or by the
    // application that offered the factory. Ports an application merely holds cannot.
    let pausable = card.node.placement.is_some() || !card.node.kind.is_empty();
    match &card.node.state {
        NodeState::Running if pausable => Some(("Pause", Request::Pause { runtime, node })),
        NodeState::Paused if pausable => Some(("Resume", Request::Resume { runtime, node })),
        _ => None,
    }
}

/// The buttons that act on a node. True if one was pressed.
fn actions(ui: &mut Ui, feed: &Feed, card: &Card) -> bool {
    let mut pressed = false;
    if let Some((word, request)) = toggle(card)
        && ui.button(word).clicked()
    {
        feed.ask(request);
        pressed = true;
    }
    if ui.button("Remove").clicked() {
        feed.ask(Request::Remove { runtime: card.runtime.clone(), node: card.node.name.clone() });
        pressed = true;
    }
    pressed
}

/// How far the view can be zoomed out: far enough to see a large machine whole.
const SMALLEST: f32 = 0.05;

/// The view that shows every card, no larger than life.
fn framing(snarl: &Snarl<Card>, canvas: Rect) -> TSTransform {
    let card = |(pos, card): (egui::Pos2, &Card)| Rect::from_min_size(pos, graph::size(card));
    let cards = snarl.nodes_pos().map(card).fold(Rect::NOTHING, Rect::union).expand(40.0);
    let scale = (canvas.width() / cards.width()).min(canvas.height() / cards.height()).clamp(SMALLEST, 1.0);
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
            output.message,
            input.message
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
    colors: &'a Colors,
    /// Whether the window is drawn light on dark.
    dark: bool,
    focus: &'a mut Focus,
    inventories: &'a [Inventory],
    rates: &'a Rates,
    /// A view to switch to this frame.
    frame: Option<TSTransform>,
}

impl Viewer<'_> {
    fn color(&self, card: &Card) -> Color32 {
        self.colors.of(&card.runtime, self.dark)
    }

    /// A pin, and any wire that leaves it, in the colour of the node's runtime.
    fn pin(&self, card: &Card) -> PinInfo {
        PinInfo::circle().with_fill(self.color(card)).with_wire_color(self.color(card))
    }
}

/// A dot in the colour of a runtime. It stands for the runtime, and is named after it.
fn dot(ui: &mut Ui, color: Color32, runtime: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, ui.spacing().interact_size.y), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, runtime));
    ui.painter().circle_filled(rect.center(), 5.0, color);
    response.on_hover_text(format!("runs in {runtime}"));
}

/// The small button on a card that pauses its node or starts it again: two bars, or a triangle.
fn sign(ui: &mut Ui, word: &str, paused: bool) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, word));
    let look = ui.style().interact(&response);
    let (middle, ink) = (rect.center(), look.fg_stroke.color);
    ui.painter().rect_filled(rect, look.corner_radius, look.weak_bg_fill);
    if paused {
        let corners = [egui::vec2(-3.5, -5.0), egui::vec2(5.0, 0.0), egui::vec2(-3.5, 5.0)];
        ui.painter().add(Shape::convex_polygon(corners.map(|corner| middle + corner).to_vec(), ink, Stroke::NONE));
    } else {
        for bar in [-3.0, 3.0] {
            ui.painter().rect_filled(
                Rect::from_center_size(middle + egui::vec2(bar, 0.0), egui::vec2(3.0, 10.0)),
                0.0,
                ink,
            );
        }
    }
    response.on_hover_text(word)
}

/// What is written beside a pin, in a space of fixed width: a card keeps its
/// shape as data starts and stops.
fn aside(ui: &mut Ui, text: String) -> egui::Response {
    let space = ui.allocate_ui(egui::vec2(graph::ASIDE, 0.0), |ui| {
        ui.set_min_width(graph::ASIDE);
        ui.weak(text)
    });
    space.inner
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
            dot(ui, self.color(card), &card.runtime);
            ui.strong(&card.node.name);
            match &card.node.state {
                NodeState::Running => ui.weak("running"),
                NodeState::Paused => ui.colored_label(ui.visuals().warn_fg_color, "paused"),
                NodeState::Stopped(_) => ui.colored_label(ui.visuals().error_fg_color, "stopped"),
            };
            if let Some((word, request)) = toggle(card)
                && sign(ui, word, card.node.state == NodeState::Paused).clicked()
            {
                self.feed.ask(request);
            }
        });
    }

    /// The outline of a card is the colour of its runtime, heavier when the card is selected.
    fn node_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        snarl: &Snarl<Card>,
    ) -> egui::Frame {
        let card = &snarl[node];
        let selected = matches!(&*self.focus, Focus::Node { key, .. } if *key == card.key());
        default.stroke(Stroke::new(if selected { 3.5 } else { 1.8 }, self.color(card))).corner_radius(9)
    }

    /// The header is tinted with it.
    fn header_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        snarl: &Snarl<Card>,
    ) -> egui::Frame {
        let tint = if self.dark { 0.28 } else { 0.20 };
        default.fill(default.fill.lerp_to_gamma(self.color(&snarl[node]), tint)).corner_radius(9)
    }

    fn inputs(&mut self, card: &Card) -> usize {
        card.node.inputs.len()
    }

    fn outputs(&mut self, card: &Card) -> usize {
        card.node.outputs.len()
    }

    // A pin gets one short row: its name, and how fast data passes through it.
    // The counters are a hover away and in the details panel.
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        let input = &card.node.inputs[pin.id.input];
        let received: u64 = input.sources.iter().map(|source| source.received).sum();
        let lost: u64 = input.sources.iter().map(|source| source.lost).sum();
        ui.label(port(&input.port)).on_hover_text(format!("{received} received, {lost} lost"));
        let waiting = card.waiting(pin.id.input);
        if waiting.is_empty() {
            aside(ui, rate(self.rates, &card.input(pin.id.input)));
        } else {
            aside(ui, "waiting".to_owned()).on_hover_text(format!("waiting for {}", waiting.join(", ")));
        }
        self.pin(card)
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        let output = &card.node.outputs[pin.id.output];
        ui.label(port(&output.port)).on_hover_text(format!("{} sent to {} inputs", output.sent, output.subscribers));
        aside(ui, rate(self.rates, &output.service));
        self.pin(card)
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

    /// A click beside the cards lets go of the one that was selected.
    fn background_clicked(&mut self, _: &Snarl<Card>) {
        if matches!(self.focus, Focus::Node { .. }) {
            *self.focus = Focus::Nothing;
        }
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

/// Samples per second through each port, by `runtime/node/port`.
type Rates = BTreeMap<String, f64>;

/// `name: Type`, as written beside a pin.
fn port(spec: &PortSpec) -> String {
    format!("{}: {}", spec.name, spec.message)
}

/// How fast data is passing through a port, such as `120/s`, or `0/s` when
/// none is; nothing for a port so new that it is not known yet.
fn rate(rates: &Rates, name: &str) -> String {
    match rates.get(name) {
        Some(rate) if *rate >= 10.0 || *rate == 0.0 => format!("{rate:.0}/s"),
        Some(rate) => format!("{rate:.1}/s"),
        None => String::new(),
    }
}

fn state(state: &NodeState) -> String {
    match state {
        NodeState::Running => "running".to_owned(),
        NodeState::Paused => "paused".to_owned(),
        NodeState::Stopped(panic) => format!("stopped by a panic: {panic}"),
    }
}

/// The thread that runs a node, given the pools of its runtime.
fn runs_on(placement: &Option<Placement>, pools: &[PoolInfo]) -> String {
    match placement {
        None => "the application's own loop".to_owned(),
        Some(Placement::Dedicated) => "a thread of its own".to_owned(),
        Some(Placement::Pool(name)) => match pools.iter().find(|pool| pool.name == *name) {
            Some(pool) if pool.shared => format!("pool {name}: any of its {} threads", pool.threads),
            Some(pool) => format!("pool {name}: one of its {} threads", pool.threads),
            None => format!("pool {name}"),
        },
    }
}
