//! The window, and the four windows within it: the runtimes on this machine,
//! the graph of their nodes, the details of what is selected, and charts of
//! numbers over time. Each is under a bar with its name on it. A window is
//! taken by that name and put down beside another, in its place, or in with
//! it as a group (see `panes.rs`), closed by the cross beside the name, and
//! opened again from the row along the top. `egui_tiles` does the arranging.
//!
//! How the windows and the charts are arranged, what is on the charts, where
//! the cards are and which colours were chosen is kept from one run to the next.
//!
//! Nothing drawn here is the truth. Every frame the drawing is brought in line
//! with what the runtimes last reported, and everything the user does becomes
//! a request to a runtime. A wire appears when its owner says it exists.
//!
//! Unless the user is drafting. Then what they do is held back as a draft
//! (see `draft.rs`), the windows show the machine as it would be, and the
//! requests are made together when the draft is applied. What is only
//! drafted is drawn grey, with a glow around it.
//!
//! A node wears the colour of the runtime it runs in: its card, its pins and
//! the wires that leave it. When a card is clicked, every card that no wire
//! joins to it goes grey, until a click beside the cards.
use crate::charts::{Charts, Recorded};
use crate::colors::Colors;
use crate::draft::{Draft, Mark, Marks};
use crate::feed::{Feed, Request};
use crate::graph::{self, Card, Place, Places};
use eframe::egui::color_picker::{Alpha, color_edit_button_srgba};
use eframe::egui::{self, Color32, Rect, Role, Sense, Shape, Stroke, Ui, UiBuilder, WidgetInfo, emath::TSTransform};
use egui_snarl::ui::{
    NodeLayout, PinInfo, PinPlacement, PinWireInfo, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireStyle,
};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use egui_tiles::{Behavior, Linear, LinearDir, SimplificationOptions, Tabs, TileId, Tiles, Tree, UiResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use triq::{FactoryInfo, Inventory, NodeState, Placement, PoolInfo, PortSpec};
use triq_views::panes;

pub struct Workbench {
    /// The four windows: which of them are open, and how they share the room.
    windows: Tree<Window>,
    contents: Contents,
}

/// What the arrangement is kept under, between one run and the next.
const LAYOUT: &str = "layout";

/// What is kept: [`Workbench::layout`], read back.
#[derive(Deserialize)]
struct Kept {
    windows: Tree<Window>,
    charts: Charts,
    /// Where cards were. A layout from before cards were remembered has none.
    #[serde(default)]
    cards: Places,
    /// The colours that were chosen for runtimes, by name.
    #[serde(default)]
    colors: BTreeMap<String, Color32>,
}

/// One of the windows within the window.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum Window {
    /// Every runtime on the machine: its pools, the kinds of node it offers, and its nodes.
    Runtimes,
    /// The nodes, and the wires between them.
    Graph,
    /// What is selected: a node, or one about to be created.
    Details,
    /// Numbers over time.
    Charts,
}

impl Window {
    const ALL: [Window; 4] = [Window::Runtimes, Window::Graph, Window::Details, Window::Charts];

    fn name(self) -> &'static str {
        match self {
            Window::Runtimes => "Runtimes",
            Window::Graph => "Graph",
            Window::Details => "Details",
            Window::Charts => "Charts",
        }
    }
}

/// What the windows show.
struct Contents {
    feed: Feed,
    /// What the runtimes last reported, as it stood when this frame began.
    real: Arc<Vec<Inventory>>,
    /// The changes that are held back, while the user is drafting.
    draft: Option<Draft>,
    /// What the draft does to each card and wire.
    marks: Marks,
    /// The machine as every window shows it this frame: as the runtimes
    /// reported it, or while drafting as it would be with the draft's changes made.
    inventories: Arc<Vec<Inventory>>,
    rates: Arc<Rates>,
    snarl: Snarl<Card>,
    /// Where every card is, and where the cards of programs that are not running were.
    places: Places,
    colors: Colors,
    charts: Charts,
    focus: Focus,
    /// Bring every card into view on the next frame that has any.
    fit: bool,
    /// Where in the window the graph was drawn last.
    canvas: Option<Rect>,
}

/// What the details are about.
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
        let contents = Contents {
            feed,
            real: Arc::default(),
            draft: None,
            marks: Marks::default(),
            inventories: Arc::default(),
            rates: Arc::default(),
            snarl: Snarl::new(),
            places: Places::new(),
            colors: Colors::default(),
            charts: Charts::default(),
            focus: Focus::Nothing,
            fit: true,
            canvas: None,
        };
        Self { windows: arranged(), contents }
    }

    /// The drawing: one card per node, one wire per live connection.
    pub fn graph(&self) -> &Snarl<Card> {
        &self.contents.snarl
    }

    /// Which colour each runtime has.
    pub fn colors(&self) -> &Colors {
        &self.contents.colors
    }

    /// Choose the colour of a runtime, as the picker beside its name does, or
    /// with `None` give it one of the workbench's choosing again.
    pub fn paint(&mut self, runtime: &str, color: Option<Color32>) {
        self.contents.colors.choose(runtime, color);
    }

    /// Ask for a connection, as dragging a wire from an output pin to an input pin does.
    pub fn wire(&mut self, from: OutPinId, to: InPinId) {
        let Contents { feed, draft, real, snarl, .. } = &mut self.contents;
        wire(&mut Asking { feed, draft: draft.as_mut(), real }, snarl, from, to);
    }

    /// Ask for a connection to be removed.
    pub fn unwire(&mut self, from: OutPinId, to: InPinId) {
        let Contents { feed, draft, real, snarl, .. } = &mut self.contents;
        unwire(&mut Asking { feed, draft: draft.as_mut(), real }, snarl, from, to);
    }

    /// The changes that are held back while drafting, in the order they will be asked for.
    pub fn drafted(&self) -> Vec<Request> {
        self.contents.draft.as_ref().map_or(Vec::new(), |draft| draft.changes(&self.contents.real))
    }

    /// How the windows and the charts are arranged, what is on the charts,
    /// where the cards are and which colours were chosen, to be handed to
    /// [`Workbench::arrange`] by the next run.
    pub fn layout(&self) -> String {
        let Contents { charts, places, colors, .. } = &self.contents;
        let (cards, colors) = (graph::remembered(places), colors.chosen());
        serde_json::json!({ "windows": self.windows, "charts": charts, "cards": cards, "colors": colors }).to_string()
    }

    /// Be as [`Workbench::layout`] said the workbench was. A number on a chart,
    /// a card in the graph and a runtime are known by what they are called:
    /// the number is drawn, the card is where it was and the runtime has its
    /// colour when something of those names is running, and they wait until
    /// then. What cannot be read as a layout changes nothing.
    pub fn arrange(&mut self, layout: &str) {
        let Ok(Kept { windows, charts, cards, colors }) = serde_json::from_str(layout) else { return };
        // Whatever wrote it, a workbench has its four windows and a chart.
        if Window::ALL.iter().all(|window| windows.tiles.find_pane(window).is_some()) && charts.has_a_chart() {
            (self.windows, self.contents.charts) = (windows, charts);
            // A card that is on the drawing already goes where it was, too.
            for (_, drawn) in self.contents.snarl.nodes_ids_data_mut() {
                if let Some(Place { at: [x, y], .. }) = cards.get(&drawn.value.key()) {
                    drawn.pos = egui::pos2(*x, *y);
                }
            }
            self.contents.places = cards;
            self.contents.colors = Colors::default();
            for (runtime, color) in colors {
                self.contents.colors.choose(&runtime, Some(color));
            }
        }
    }

    /// As it was when it last closed, if anything was kept of that.
    pub fn remembered(mut self, kept: Option<&dyn eframe::Storage>) -> Self {
        if let Some(layout) = kept.and_then(|kept| kept.get_string(LAYOUT)) {
            self.arrange(&layout);
        }
        self
    }

    /// Draw one frame: a row with the name of every window, and under it the windows that are open.
    pub fn show(&mut self, ui: &mut Ui) {
        self.contents.refresh();
        egui::Panel::top("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                for window in Window::ALL {
                    let open = self.open(window);
                    let about = if open { "Close this window" } else { "Open this window, where it was" };
                    if ui.selectable_label(open, window.name()).on_hover_text(about).clicked()
                        && let Some(place) = self.windows.tiles.find_pane(&window)
                    {
                        // A window that is closed keeps its place. Opened, it is in front there.
                        self.windows.set_visible(place, !open);
                        self.windows.make_active(|tile, _| tile == place);
                    }
                }
                ui.separator();
                let inventories = &self.contents.inventories;
                let nodes: usize = inventories.iter().map(|inventory| inventory.nodes.len()).sum();
                ui.weak(format!("{} runtimes, {nodes} nodes", inventories.len()));
                self.contents.problem(ui);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            if !Window::ALL.into_iter().any(|window| self.open(window)) {
                ui.weak("Every window is closed. Their names above open them.");
            }
            let idle = panes::idle_bars(&self.windows.tiles);
            let mut shown = Shown { contents: &mut self.contents, idle, closed: None };
            let carried = self.windows.dragged_id(ui.ctx());
            self.windows.ui(&mut shown, ui);
            // A window that is closed is only out of sight. It keeps its place, for when it is opened again.
            if let Some(place) = shown.closed {
                for window in panes::panes_in(&self.windows.tiles, place) {
                    self.windows.set_visible(window, false);
                }
            }
            if panes::tidy(&mut self.windows, carried) {
                ui.ctx().request_repaint();
            }
        });
    }

    fn open(&self, window: Window) -> bool {
        self.windows.tiles.find_pane(&window).is_some_and(|place| self.windows.is_visible(place))
    }
}

/// The windows as they are to begin with: the runtimes, the graph and the
/// details side by side, and the charts under the three of them.
fn arranged() -> Tree<Window> {
    let mut tiles = Tiles::default();
    // Each is put under its bar here, so that the room it is given below is the room of the bar and all.
    let [runtimes, graph, details, charts] = Window::ALL.map(|window| {
        let window = tiles.insert_pane(window);
        tiles.insert_tab_tile(vec![window])
    });
    let mut row = Linear::new(LinearDir::Horizontal, vec![runtimes, graph, details]);
    for (window, share) in [(runtimes, 0.16), (graph, 0.58), (details, 0.26)] {
        row.shares.set_share(window, share);
    }
    let row = tiles.insert_container(row);
    let all = tiles.insert_container(Linear::new_binary(LinearDir::Vertical, [row, charts], 0.6));
    Tree::new(egui::Id::unique("windows"), all, tiles)
}

/// How the windows are drawn: each under a bar that has its name on it.
struct Shown<'a> {
    contents: &'a mut Contents,
    /// The bars of groups of which only one window is open: they are not shown.
    idle: Vec<TileId>,
    /// The place whose cross was pressed, to be closed once everything is drawn.
    closed: Option<TileId>,
}

impl Behavior<Window> for Shown<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, _: TileId, window: &mut Window) -> UiResponse {
        let contents = &mut *self.contents;
        // What a window remembers, such as how far it is scrolled and where the view of the graph
        // is, goes by the window's name, so that it is not forgotten when the window is put elsewhere.
        let known = UiBuilder::new().scope_id(egui::Id::unique(("window", window.name())));
        ui.scope_builder(known, |ui| match window {
            Window::Runtimes => _ = egui::CentralPanel::default().show(ui, |ui| contents.runtimes(ui)),
            Window::Graph => contents.graph(ui),
            Window::Details => _ = egui::CentralPanel::default().show(ui, |ui| contents.details(ui)),
            Window::Charts => contents.charts(ui),
        });
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, window: &Window) -> egui::WidgetText {
        window.name().into()
    }

    /// The name of a group on a bar lists the windows in it that are open.
    fn tab_title_for_tile(&mut self, tiles: &Tiles<Window>, place: TileId) -> egui::WidgetText {
        let open = panes::panes_in(tiles, place).into_iter().filter(|window| tiles.is_visible(*window));
        let names: Vec<&str> = open.filter_map(|window| tiles.get_pane(&window)).map(|window| window.name()).collect();
        names.join(", ").into()
    }

    fn is_tab_bar_visible(&self, place: TileId, _: &Tabs) -> bool {
        !self.idle.contains(&place)
    }

    fn on_tab_button(&mut self, _: &mut Tiles<Window>, _: TileId, name: egui::Response) -> egui::Response {
        name.on_hover_text(
            "Drag the name to put the window beside another, or onto the middle of one to share its place",
        )
    }

    fn is_tab_closable(&self, _: &Tiles<Window>, _: TileId) -> bool {
        true
    }

    /// Nothing is taken out of the arrangement: see where `closed` is read.
    fn on_tab_close(&mut self, _: &mut Tiles<Window>, place: TileId) -> bool {
        self.closed = Some(place);
        false
    }

    fn simplification_options(&self) -> SimplificationOptions {
        panes::rules()
    }
}

impl Contents {
    /// Take what the runtimes last reported, and bring the drawing in line with it.
    fn refresh(&mut self) {
        (self.real, self.rates) = (self.feed.inventories(), self.feed.rates());
        // Changes that were applied and could not be made are a draft again.
        let unmade = self.feed.unmade();
        if !unmade.is_empty() {
            let draft = self.draft.get_or_insert_default();
            unmade.into_iter().for_each(|change| draft.record(change, &self.real));
        }
        (self.inventories, self.marks) = match &self.draft {
            Some(draft) => (Arc::new(draft.over(&self.real)), draft.marks(&self.real)),
            None => (self.real.clone(), Marks::default()),
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        graph::sync_with(&mut self.snarl, &self.inventories, &mut self.places, now);
        self.colors.update(self.real.iter().map(|inventory| inventory.runtime.as_str()));
        if let Focus::Node { key, .. } = &self.focus
            && !self.snarl.nodes().any(|card| card.key() == *key)
        {
            self.focus = Focus::Nothing;
        }
    }

    /// The most recent thing that went wrong, until it is dismissed.
    fn problem(&mut self, ui: &mut Ui) {
        if let Some(problem) = self.feed.problem() {
            ui.colored_label(ui.visuals().error_fg_color, problem);
            if ui.small_button("Dismiss").clicked() {
                self.feed.dismiss();
            }
        }
    }

    /// Every runtime with its pools, the kinds of node it offers, and its nodes.
    fn runtimes(&mut self, ui: &mut Ui) {
        let inventories = self.inventories.clone();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if inventories.is_empty() {
                ui.weak("No runtime is running on this machine.");
            }
            for inventory in inventories.iter() {
                ui.horizontal(|ui| {
                    paint(ui, &mut self.colors, &inventory.runtime);
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

    fn details(&mut self, ui: &mut Ui) {
        let (inventories, rates) = (self.inventories.clone(), self.rates.clone());
        let Contents { feed, draft, real, marks, focus, snarl, .. } = self;
        let mut asking = Asking { feed, draft: draft.as_mut(), real };
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match focus {
            Focus::Nothing => {
                if let Some(draft) = &mut asking.draft {
                    held_back(ui, draft, asking.real);
                    ui.separator();
                }
                ui.weak("Click a node to see where it runs, or pick a kind of node among the runtimes to create one.");
                ui.weak(
                    "Drag from an output to an input to connect them. Right-click a wire or a pin to disconnect it.",
                );
                ui.weak("Right-click a node to pause or remove it.");
                ui.weak("A node that is clicked keeps its colour, with all that wires join to it. The rest goes grey.");
            }
            Focus::Node { key, settings } => {
                let card = snarl.nodes().find(|card| card.key() == *key);
                let home =
                    inventories.iter().find(|inventory| card.is_some_and(|card| card.runtime == inventory.runtime));
                if let (Some(card), Some(home)) = (card, home) {
                    node_details(ui, &mut asking, marks, card, home, settings, &rates);
                }
            }
            Focus::Form(form) => {
                if form.show(ui, &mut asking, &inventories) {
                    *focus = Focus::Nothing;
                }
            }
        });
    }

    /// The graph, under the buttons that lay it out and bring it into view.
    fn graph(&mut self, ui: &mut Ui) {
        egui::Panel::top("graph-controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Tidy").on_hover_text("Lay the nodes out again, by what feeds what").clicked() {
                    graph::tidy(&mut self.snarl);
                    // Laid out afresh, the cards are where they belong now: where they were put is forgotten.
                    self.places.clear();
                    self.fit = true;
                }
                self.fit |= ui.small_button("Fit").on_hover_text("Bring every node into view").clicked();
                ui.separator();
                self.drafting(ui);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }

    /// The button that begins a draft; or, while there is one, how much it
    /// holds, and the buttons that apply and discard it.
    fn drafting(&mut self, ui: &mut Ui) {
        let Some(draft) = &self.draft else {
            let about = "Hold back what is changed in the graph and the details, to make the changes together";
            if ui.small_button("Draft").on_hover_text(about).clicked() {
                self.draft = Some(Draft::default());
            }
            return;
        };
        let changes = draft.changes(&self.real);
        let held = ui.label(format!("Draft: {}", counted(changes.len())));
        if !changes.is_empty() {
            held.on_hover_ui(|ui| changes.iter().for_each(|change| _ = ui.label(change.to_string())));
        }
        let about = "Ask the runtimes for the changes, in the order they were made here";
        if ui.small_button("Apply draft").on_hover_text(about).clicked() {
            self.feed.apply(changes);
            self.draft = None;
        } else if ui
            .small_button("Discard draft")
            .on_hover_text("Forget the changes. The machine stays as it is")
            .clicked()
        {
            self.draft = None;
        }
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let (inventories, rates) = (self.inventories.clone(), self.rates.clone());
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
        let canvas = ui.max_rect();
        let frame = (self.fit && self.snarl.nodes().next().is_some()).then(|| framing(&self.snarl, canvas));
        self.fit &= frame.is_none();
        // The graph's window may have been moved or resized. What was in the middle of it still is.
        let carried = self.canvas.replace(canvas).map_or(egui::Vec2::ZERO, |before| canvas.center() - before.center());
        let (colors, dark) = (&self.colors, ui.visuals().dark_mode);
        let (inventories, rates) = (&inventories[..], &*rates);
        // With a card selected, only what wires join to it keeps its colour.
        let lit = match &self.focus {
            Focus::Node { key, .. } => Some(graph::joined(&self.snarl, key)).filter(|lit| !lit.is_empty()),
            _ => None,
        };
        let asking = Asking { feed: &self.feed, draft: self.draft.as_mut(), real: &self.real };
        let (focus, marks) = (&mut self.focus, &self.marks);
        let mut viewer = Viewer { asking, marks, colors, dark, focus, lit, inventories, rates, frame, carried };
        // Text that could be selected would swallow the press that moves, selects or opens the menu of a card.
        ui.style_mut().interaction.selectable_labels = false;
        SnarlWidget::new().id_salt("triq-graph").style(style).show(&mut self.snarl, &mut viewer, ui);
    }

    fn charts(&mut self, ui: &mut Ui) {
        // A chart ends at the present, and the present moves on. While there is a
        // line to slide along, the window is drawn again as soon as it can be.
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
        if !self.charts.is_empty() {
            ui.ctx().request_repaint();
        }
        let recorded = Recorded { history: &self.feed.history(), signals: self.feed.signals() };
        self.charts.show(ui, recorded, now);
    }
}

impl eframe::App for Workbench {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        self.show(ui);
    }

    /// Called now and then, and when the window closes.
    fn save(&mut self, kept: &mut dyn eframe::Storage) {
        kept.set_string(LAYOUT, self.layout());
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
    fn show(&mut self, ui: &mut Ui, asking: &mut Asking<'_>, inventories: &[Inventory]) -> bool {
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
                asking.ask(Request::Spawn { runtime, kind, name, placement, params });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
    }
}

/// `home` is what the node's runtime last reported, or while drafting that with the draft's changes made.
fn node_details(
    ui: &mut Ui,
    asking: &mut Asking<'_>,
    marks: &Marks,
    card: &Card,
    home: &Inventory,
    settings: &mut String,
    rates: &Rates,
) {
    let node = &card.node;
    ui.heading(&node.name);
    if let Some(mark) = marks.card(&card.key()) {
        ui.weak(format!("{}, when the draft is applied", awaits(mark)));
    }
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
    ui.horizontal(|ui| actions(ui, asking, card));

    if !node.params.is_null() {
        ui.separator();
        ui.strong("Settings");
        ui.add(egui::TextEdit::multiline(settings).code_editor().desired_rows(3));
        match serde_json::from_str::<Value>(settings) {
            Ok(params) => {
                if ui.button("Apply").clicked() {
                    asking.ask(Request::SetParams { runtime: card.runtime.clone(), node: node.name.clone(), params });
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
            let drafted = marks.wire(&source.output, &card.input(index));
            let link = match (&source.problem, source.connected) {
                _ if drafted == Some(Mark::New) => "to be connected, when the draft is applied".to_owned(),
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
                if drafted == Some(Mark::Going) {
                    ui.weak("to be disconnected");
                } else if ui.small_button("Disconnect").clicked() {
                    asking.ask(Request::Disconnect { from: source.output.clone(), to: card.input(index) });
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
fn actions(ui: &mut Ui, asking: &mut Asking<'_>, card: &Card) -> bool {
    let mut pressed = false;
    if let Some((word, request)) = toggle(card)
        && ui.button(word).clicked()
    {
        asking.ask(request);
        pressed = true;
    }
    if ui.button("Remove").clicked() {
        asking.ask(Request::Remove { runtime: card.runtime.clone(), node: card.node.name.clone() });
        pressed = true;
    }
    pressed
}

/// Where what the user asks for goes: to the runtimes, or, while drafting, into the draft.
struct Asking<'a> {
    feed: &'a Feed,
    draft: Option<&'a mut Draft>,
    /// What the runtimes last reported.
    real: &'a [Inventory],
}

impl Asking<'_> {
    fn ask(&mut self, request: Request) {
        match &mut self.draft {
            Some(draft) => draft.record(request, self.real),
            None => self.feed.ask(request),
        }
    }
}

/// The changes a draft holds, in the order they will be asked for. Each can be taken back.
fn held_back(ui: &mut Ui, draft: &mut Draft, real: &[Inventory]) {
    ui.heading("Draft");
    let changes = draft.changes(real);
    if changes.is_empty() {
        ui.weak("What is changed in the graph and here is held back, until the draft is applied.");
    }
    for (number, change) in changes.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("×").on_hover_text("Take this change back").clicked() {
                draft.take_back(change);
            }
            ui.label(format!("{}. {change}", number + 1));
        });
    }
}

/// How many changes, in words.
fn counted(changes: usize) -> String {
    match changes {
        0 => "no changes".to_owned(),
        1 => "1 change".to_owned(),
        several => format!("{several} changes"),
    }
}

/// What is to happen to something that a draft marks.
fn awaits(mark: Mark) -> &'static str {
    match mark {
        Mark::New => "to be created",
        Mark::Changed => "to be changed",
        Mark::Going => "to be removed",
    }
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

fn wire(asking: &mut Asking<'_>, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node], &snarl[to.node]);
    let (output, input) = (&source.node.outputs[from.output].port, &sink.node.inputs[to.input].port);
    if !graph::compatible(output, input) {
        asking.feed.report(format!(
            "{} and {} carry different messages: {} and {}",
            source.output(from.output),
            sink.input(to.input),
            output.message,
            input.message
        ));
        return;
    }
    asking.ask(Request::Connect { from: source.output(from.output).to_owned(), to: sink.input(to.input) });
}

fn unwire(asking: &mut Asking<'_>, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node], &snarl[to.node]);
    asking.ask(Request::Disconnect { from: source.output(from.output).to_owned(), to: sink.input(to.input) });
}

/// How the graph widget asks what to draw and reports what the user did.
struct Viewer<'a> {
    asking: Asking<'a>,
    /// What the draft does to each card and wire.
    marks: &'a Marks,
    colors: &'a Colors,
    /// Whether the window is drawn light on dark.
    dark: bool,
    focus: &'a mut Focus,
    /// The cards that keep their colour while one is selected: that one, and
    /// all that wires join to it. `None` while none is: then every card does.
    lit: Option<BTreeSet<String>>,
    inventories: &'a [Inventory],
    rates: &'a Rates,
    /// A view to switch to this frame.
    frame: Option<TSTransform>,
    /// How far the graph has moved in the window since the last frame. The view moves along.
    carried: egui::Vec2,
}

impl Viewer<'_> {
    /// Whether a card is greyed: another is selected, and no wire joins this one to it.
    fn dimmed(&self, card: &Card) -> bool {
        self.lit.as_ref().is_some_and(|lit| !lit.contains(&card.key()))
    }

    /// What the draft does to a card.
    fn mark(&self, card: &Card) -> Option<Mark> {
        self.marks.card(&card.key())
    }

    /// The colour of a card's runtime. A card that is only drafted is no
    /// runtime's yet, and a card that is greyed is grey.
    fn color(&self, card: &Card) -> Color32 {
        match self.mark(card) {
            Some(Mark::New) => drafted(self.dark),
            _ if self.dimmed(card) => grey(self.dark),
            _ => self.colors.of(&card.runtime, self.dark),
        }
    }

    /// A pin, and any wire that leaves it, in the colour of the node's
    /// runtime. `port` is what the pin is called, `runtime/node/port`: a wire
    /// that the draft draws or cuts has a glow around the pins at its ends.
    fn pin(&self, card: &Card, port: &str) -> Pin {
        let pin = PinInfo::circle().with_fill(self.color(card)).with_wire_color(self.color(card));
        Pin { pin, glows: self.marks.port(port).then(|| glow(self.dark)) }
    }
}

/// A pin as it is drawn, with a glow around it or without.
struct Pin {
    pin: PinInfo,
    glows: Option<egui::Shadow>,
}

impl SnarlPin for Pin {
    fn draw(self, snarl: &SnarlStyle, style: &egui::Style, rect: Rect, painter: &egui::Painter) -> PinWireInfo {
        if let Some(glow) = self.glows {
            painter.add(glow.as_shape(rect, egui::CornerRadius::same(u8::MAX)));
        }
        self.pin.draw(snarl, style, rect, painter)
    }
}

/// The grey of what is only drafted: brighter than the grey of a card that
/// is greyed, and with a glow around it.
fn drafted(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 215 } else { 60 })
}

/// The glow around a card that the draft creates, changes or removes, and
/// around the pins of a wire that it draws or cuts.
fn glow(dark: bool) -> egui::Shadow {
    egui::Shadow { offset: [0, 0], blur: 12, spread: 4, color: drafted(dark).gamma_multiply(0.7) }
}

/// The grey of a card that no wire joins to the one that is selected.
fn grey(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 105 } else { 150 })
}

/// A dot in the colour of a runtime. It stands for the runtime, and is named after it.
fn dot(ui: &mut Ui, color: Color32, runtime: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, ui.spacing().interact_size.y), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, runtime));
    ui.painter().circle_filled(rect.center(), 5.0, color);
    response.on_hover_text(format!("runs in {runtime}"));
}

/// The colour of a runtime, as a button that opens a picker: the same that
/// chooses the colour of a line on a chart. Beside it, once a colour was
/// chosen, is a button that gives the runtime one of the palette again.
fn paint(ui: &mut Ui, colors: &mut Colors, runtime: &str) {
    let mut color = colors.of(runtime, ui.visuals().dark_mode);
    let well = ui.scope(|ui| {
        ui.spacing_mut().interact_size = egui::vec2(16.0, 14.0);
        color_edit_button_srgba(ui, &mut color, Alpha::Opaque)
    });
    if well.inner.on_hover_text("Choose the colour of this runtime").changed() {
        colors.choose(runtime, Some(color));
    }
    let chosen = colors.chosen().contains_key(runtime);
    if chosen && ui.small_button("auto").on_hover_text("Give it a colour of the workbench's choosing again").clicked() {
        colors.choose(runtime, None);
    }
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
        to_global.translation += self.carried;
        if let Some(frame) = self.frame.take() {
            *to_global = frame;
        }
    }

    fn show_header(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<Card>) {
        let card = &snarl[node];
        ui.horizontal(|ui| {
            dot(ui, self.color(card), &card.runtime);
            // The name of a card that is greyed does not stand out either.
            if self.dimmed(card) {
                ui.label(&card.node.name)
            } else {
                ui.strong(&card.node.name)
            };
            // A node that does not exist yet neither runs nor can be paused.
            let mark = self.mark(card);
            if mark != Some(Mark::New) {
                match &card.node.state {
                    NodeState::Running => ui.weak("running"),
                    NodeState::Paused => ui.colored_label(ui.visuals().warn_fg_color, "paused"),
                    NodeState::Stopped(_) => ui.colored_label(ui.visuals().error_fg_color, "stopped"),
                };
                if let Some((word, request)) = toggle(card)
                    && sign(ui, word, card.node.state == NodeState::Paused).clicked()
                {
                    self.asking.ask(request);
                }
            }
            if let Some(mark) = mark {
                ui.label(awaits(mark));
            }
        });
    }

    /// The outline of a card is the colour of its runtime, heavier when the card
    /// is selected. A card that the draft creates, changes or removes has a glow around it.
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
        let frame = default.stroke(Stroke::new(if selected { 3.5 } else { 1.8 }, self.color(card))).corner_radius(9);
        if self.mark(card).is_some() { frame.shadow(glow(self.dark)) } else { frame }
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
        let card = &snarl[node];
        let tint = if self.dimmed(card) {
            0.08
        } else if self.dark {
            0.28
        } else {
            0.20
        };
        default.fill(default.fill.lerp_to_gamma(self.color(card), tint)).corner_radius(9)
    }

    fn has_node_style(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], snarl: &Snarl<Card>) -> bool {
        self.dimmed(&snarl[node])
    }

    /// What is written on a card that is greyed is grey too.
    fn apply_node_style(&mut self, style: &mut egui::Style, _: NodeId, _: &[InPin], _: &[OutPin], _: &Snarl<Card>) {
        style.visuals.override_text_color = Some(grey(self.dark));
    }

    /// A wire that is only drafted is grey as what is drafted is, and one
    /// that the draft cuts is faint. Any other is greyed with the cards at its ends.
    fn connected_wire_color(&mut self, from: OutPinId, to: InPinId, snarl: &Snarl<Card>, sender: Color32) -> Color32 {
        let (source, sink) = (&snarl[from.node], &snarl[to.node]);
        match self.marks.wire(source.output(from.output), &sink.input(to.input)) {
            Some(Mark::New) => drafted(self.dark),
            Some(_) => sender.gamma_multiply(0.3),
            None if self.dimmed(source) => grey(self.dark),
            None => sender,
        }
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
        self.pin(card, &card.input(pin.id.input))
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        let output = &card.node.outputs[pin.id.output];
        ui.label(port(&output.port)).on_hover_text(format!("{} sent to {} inputs", output.sent, output.subscribers));
        aside(ui, rate(self.rates, &output.service));
        self.pin(card, &output.service)
    }

    /// A wire being dragged only snaps to an input it could be connected to.
    fn can_snap(&mut self, from: &OutPin, to: &InPin, snarl: &Snarl<Card>) -> bool {
        let output = &snarl[from.id.node].node.outputs[from.id.output].port;
        let input = &snarl[to.id.node].node.inputs[to.id.input].port;
        graph::compatible(output, input)
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Card>) {
        wire(&mut self.asking, snarl, from.id, to.id);
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Card>) {
        unwire(&mut self.asking, snarl, from.id, to.id);
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<Card>) {
        pin.remotes.iter().for_each(|from| unwire(&mut self.asking, snarl, *from, pin.id));
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<Card>) {
        pin.remotes.iter().for_each(|to| unwire(&mut self.asking, snarl, pin.id, *to));
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
        if actions(ui, &mut self.asking, &snarl[node]) {
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
