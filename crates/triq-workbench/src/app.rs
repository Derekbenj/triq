//! The window, and the four windows within it: the runtimes on this machine,
//! the graph of their nodes, the details of what is selected, and charts of
//! numbers over time. Each is under a bar with its name on it. A window is
//! taken by that name and put down beside another, in its place, or in with
//! it as a group (see `panes.rs`), closed by the cross beside the name, and
//! opened again from the row along the top. `egui_tiles` does the arranging.
//!
//! How the windows and the charts are arranged, what is on the charts, where
//! the cards are and which colours were chosen is kept from one run to the
//! next.
//!
//! Nothing drawn here is the truth. Every frame the drawing is brought in line
//! with what the runtimes last reported, and everything the user does becomes
//! a request to a runtime. A wire appears when its owner says it exists.
//!
//! Unless the user is drafting. Then what they do is held back as a draft
//! (see `draft.rs`), the windows show the machine as it would be, and the
//! changes are made together when the draft is applied.
//!
//! Nodes can be put together in groups, as in Blender: a group is drawn as
//! one card, and the graph can show the inside of one instead of the whole
//! machine (see `graph.rs`). Which group a node is in is its runtime's to
//! say, like everything else about it: the workbench asks, and draws what is
//! reported. How the graph itself is drawn and handled is in `app/cards.rs`,
//! and the details window in `app/details.rs`.
mod cards;
mod details;

use crate::charts::{Charts, Recorded};
use crate::colors::Colors;
use crate::draft::{Draft, Mark, Marks};
use crate::feed::{Feed, Request};
use crate::graph::{self, Card, Groups, Kind, Place, Places};
use crate::settings::Editing;
use cards::Viewer;
use details::{Filled, Form};
use eframe::egui::color_picker::{Alpha, color_edit_button_srgba};
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, Rect, Stroke, Ui, UiBuilder, emath::TSTransform};
use egui_tiles::{Behavior, Linear, LinearDir, SimplificationOptions, Tabs, TileId, Tiles, Tree, UiResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use triq::{Inventory, NodeInfo};
use triq_snarl::ui::{NodeLayout, PinPlacement, SnarlStyle, SnarlWidget, WireStyle};
use triq_snarl::{InPinId, OutPinId, Snarl};
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
    /// What is selected: a node, a group, or a node about to be created.
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
    /// Which group each node is in, by that.
    groups: Groups,
    /// The changes that are held back, while the user is drafting.
    draft: Option<Draft>,
    /// What the draft does to each card and wire.
    marks: Marks,
    /// The machine as every window shows it this frame: as the runtimes
    /// reported it, or while drafting as it would be with the draft's changes made.
    inventories: Arc<Vec<Inventory>>,
    /// The groups as every window shows them this frame: as they are, or as drafted.
    grouping: Groups,
    rates: Arc<Rates>,
    snarl: Snarl<Card>,
    /// The group whose inside the graph shows, as `outer/inner` if it is
    /// inside another. `None` for the whole machine.
    inside: Option<String>,
    /// The nodes and groups whose cards Shift has selected in the graph, by what the cards are called.
    selected: Vec<String>,
    /// Where every card is, and where the cards of programs that are not running were.
    places: Places,
    colors: Colors,
    charts: Charts,
    focus: Focus,
    /// Something was clicked that the details are now about: their window is to be seen.
    reveal: bool,
    /// What was asked for that changes what the graph shows, to be done once the frame is drawn.
    wish: Option<Wish>,
    /// Tab was pressed over the graph: see [`Workbench::keys`].
    tabbed: bool,
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
        settings: Editing,
    },
    /// A group, by where it is among the groups, as `outer/inner`, with
    /// what it is called there as the user is editing it.
    Group {
        name: String,
        renamed: String,
    },
    /// A node about to be created.
    Form(Form),
}

impl Focus {
    /// Turn to a node, which is drawn with the settings `drawn`. `real` is what the runtimes
    /// report: what the node has there is what a change to its settings is a change from.
    fn node(key: String, real: &[Inventory], drawn: &Value) -> Self {
        let has = node(real, &key).map_or(Value::Null, |(_, node)| node.params.clone());
        Self::Node { key, settings: Editing::new(has, drawn) }
    }

    /// Turn to a group. If the details are about it already, they stay as
    /// they are, with its name as the user is editing it.
    fn group(&mut self, group: &str) {
        if !matches!(self, Self::Group { name, .. } if name == group) {
            *self = Self::Group { name: group.to_owned(), renamed: graph::called(group).to_owned() };
        }
    }
}

/// Something asked for that changes what the graph shows: by a key, a menu
/// or a button. It is done once the frame is drawn.
enum Wish {
    /// Show the inside of a group.
    Enter(String),
    /// Show what is around the group that is shown: one group out.
    Leave,
    /// Put these cards, of nodes and of groups, together in a new group.
    Group(Vec<String>),
    /// Put these cards into a group that is there.
    Insert(Vec<String>, String),
    /// Take a group apart: what was in it is where the group was.
    Ungroup(String),
    /// Move these cards out of the group that is shown, into the one around it.
    Separate(Vec<String>),
    /// Call a group by another name.
    Rename(String, String),
}

/// Where what the user asks of a runtime goes: to the runtime, or, while drafting, into the draft.
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

/// Samples per second through each port, by `runtime/node/port`.
type Rates = BTreeMap<String, f64>;

impl Workbench {
    pub fn new(feed: Feed) -> Self {
        let contents = Contents {
            feed,
            real: Arc::default(),
            groups: Groups::new(),
            draft: None,
            marks: Marks::default(),
            inventories: Arc::default(),
            grouping: Groups::new(),
            rates: Arc::default(),
            snarl: Snarl::new(),
            inside: None,
            selected: Vec::new(),
            places: Places::new(),
            colors: Colors::default(),
            charts: Charts::default(),
            focus: Focus::Nothing,
            reveal: false,
            wish: None,
            tabbed: false,
            fit: true,
            canvas: None,
        };
        Self { windows: arranged(), contents }
    }

    /// The drawing: the cards, and a wire for every live connection between them.
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
        cards::wire(&mut Asking { feed, draft: draft.as_mut(), real }, snarl, from, to);
    }

    /// Ask for a connection to be removed.
    pub fn unwire(&mut self, from: OutPinId, to: InPinId) {
        let Contents { feed, draft, real, snarl, .. } = &mut self.contents;
        cards::unwire(&mut Asking { feed, draft: draft.as_mut(), real }, snarl, from, to);
    }

    /// What is held back for the runtimes while drafting, in the order it will be asked for.
    pub fn drafted(&self) -> Vec<Request> {
        self.contents.draft.as_ref().map_or(Vec::new(), |draft| draft.changes(&self.contents.real))
    }

    /// Which group each node is in, by `runtime/node`: as they are, not as a draft would have them.
    pub fn groups(&self) -> &Groups {
        &self.contents.groups
    }

    /// The group whose inside the graph shows, if it is not showing the whole machine.
    pub fn inside(&self) -> Option<&str> {
        self.contents.inside.as_deref()
    }

    /// How the windows and the charts are arranged, what is on the charts,
    /// where the cards are and which colours were chosen, to be handed to
    /// [`Workbench::arrange`] by the next run.
    pub fn layout(&self) -> String {
        let Contents { charts, places, colors, .. } = &self.contents;
        let (cards, colors) = (graph::remembered(places), colors.chosen());
        let kept = serde_json::json!({ "windows": self.windows, "charts": charts, "cards": cards, "colors": colors });
        kept.to_string()
    }

    /// Be as [`Workbench::layout`] said the workbench was. A number on a chart,
    /// a card in the graph and a runtime are known by what they are called:
    /// the number is drawn, the card is where it was, and the runtime has its
    /// colour, when something of those names is running, and they wait until
    /// then. What cannot be read as a layout changes nothing.
    pub fn arrange(&mut self, layout: &str) {
        let Ok(Kept { windows, charts, cards, colors }) = serde_json::from_str(layout) else { return };
        // Whatever wrote it, a workbench has its four windows.
        if Window::ALL.iter().all(|window| windows.tiles.find_pane(window).is_some()) {
            (self.windows, self.contents.charts) = (windows, charts);
            // A card that is on the drawing already goes where it was, too.
            for (_, drawn) in self.contents.snarl.nodes_ids_data_mut() {
                if let Some(Place { at: [x, y], .. }) = cards.get(&drawn.value.key()) {
                    drawn.pos = egui::pos2(*x, *y);
                }
            }
            (self.contents.places, self.contents.colors) = (cards, Colors::default());
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
        // What was asked for shows in the frame after this one.
        if let Some(wish) = self.contents.wish.take() {
            self.contents.grant(wish);
            ui.ctx().request_repaint();
        }
        // What was clicked has its details shown: their window is opened, in front of what shares its place.
        if std::mem::take(&mut self.contents.reveal)
            && let Some(place) = self.windows.tiles.find_pane(&Window::Details)
        {
            self.windows.set_visible(place, true);
            self.windows.make_active(|tile, _| tile == place);
        }
    }

    /// Keep the Tab key for the graph while the pointer is over it. Elsewhere
    /// Tab moves from one button to the next; over the graph it shows the
    /// inside of a group and leaves it again, as it does in Blender. Called
    /// with what was typed before a frame makes anything of it.
    pub fn keys(&mut self, window: &egui::Context, typed: &mut egui::RawInput) {
        let pointer = window.pointer_latest_pos();
        let over = self.contents.canvas.zip(pointer).is_some_and(|(canvas, pointer)| canvas.contains(pointer));
        if !over || window.text_edit_focused() {
            return;
        }
        let tab = |event: &egui::Event| matches!(event, egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.is_none());
        let before = typed.events.len();
        typed.events.retain(|event| !tab(event));
        self.contents.tabbed |= typed.events.len() < before;
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
        // Whether what was asked for shows in what is taken next. A group that was just asked
        // for is reported a moment later, and until then it is not given up as gone.
        let settled = self.feed.settled();
        (self.real, self.rates) = (self.feed.inventories(), self.feed.rates());
        self.groups = graph::groups(&self.real);
        // Changes that were applied and could not be made are a draft again.
        let unmade = self.feed.unmade();
        if !unmade.is_empty() {
            let draft = self.draft.get_or_insert_default();
            unmade.into_iter().for_each(|change| draft.record(change, &self.real));
        }
        // While drafting, the windows show the machine as it would be.
        (self.inventories, self.marks) = match &self.draft {
            Some(draft) => (Arc::new(draft.over(&self.real)), draft.marks(&self.real)),
            None => (self.real.clone(), Marks::default()),
        };
        self.grouping = graph::groups(&self.inventories);
        // A group that has no node left has no inside to show: the graph shows the group around it.
        let emptied = |group: &String| !self.grouping.values().any(|of| graph::within(of, group));
        while settled && let Some(empty) = self.inside.take_if(|group| emptied(group)) {
            (self.inside, self.fit) = (graph::parent(&empty).map(str::to_owned), true);
        }
        // A port whose last connection the draft cuts is offered as one that is connected to nothing.
        let cut = self.marks.cut();
        let cards = graph::cards_with(&self.inventories, &self.grouping, self.inside.as_deref(), &cut);
        graph::show(&mut self.snarl, cards, &mut self.places, now());
        self.colors.update(self.real.iter().map(|inventory| inventory.runtime.as_str()));
        // What the details were about may be gone.
        let gone = match &self.focus {
            Focus::Node { key, .. } => node(&self.inventories, key).is_none(),
            Focus::Group { name, .. } => !self.grouping.values().any(|group| graph::within(group, name)),
            Focus::Nothing | Focus::Form(_) => false,
        };
        if gone && settled {
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
        column(ui, NARROWEST_LIST, |ui| {
            if inventories.is_empty() {
                ui.weak("No runtime is running on this machine.");
            }
            for inventory in inventories.iter() {
                ui.horizontal_wrapped(|ui| {
                    paint(ui, &mut self.colors, &inventory.runtime);
                    ui.strong(&inventory.runtime);
                    // Kept in one piece: where the row is too short for it, it goes under the name.
                    let process = egui::RichText::new(format!("process {}", inventory.pid)).weak();
                    ui.add(egui::Label::new(process).extend());
                });
                for pool in &inventory.pools {
                    let shared = if pool.shared { ", shared" } else { "" };
                    ui.weak(format!("pool {}: {} threads{shared}", pool.name, pool.threads));
                }
                for factory in &inventory.factories {
                    let mut new = ui.button(format!("New {}", factory.kind));
                    // A kind may be several nodes that are created together, as a group.
                    if let Some(group) = &factory.group {
                        new = new.on_hover_text(format!("A group of {} nodes, wired to each other", group.nodes.len()));
                    }
                    if new.clicked() {
                        (self.focus, self.reveal) = (Focus::Form(Form::new(inventory, factory)), true);
                    }
                }
                for node in &inventory.nodes {
                    let key = format!("{}/{}", inventory.runtime, node.name);
                    let focused = matches!(&self.focus, Focus::Node { key: focus, .. } if *focus == key);
                    if ui.selectable_label(focused, &node.name).clicked() {
                        (self.focus, self.reveal) = (Focus::node(key, &self.real, &node.params), true);
                    }
                }
                ui.separator();
            }
        });
    }

    fn details(&mut self, ui: &mut Ui) {
        let (inventories, rates) = (self.inventories.clone(), self.rates.clone());
        // What the buttons for groups are about, and the groups on the graph that it could be moved into.
        let (chosen, in_a_group) = (self.chosen(), self.inside.is_some());
        let here = self.snarl.nodes().filter_map(|card| match &card.kind {
            Kind::Group { path, .. } => Some(path.clone()),
            _ => None,
        });
        let here: Vec<String> = here.collect();
        // What the keys are called on this machine.
        let [make, into, apart] = [Modifiers::NONE, Modifiers::SHIFT, Modifiers::ALT]
            .map(|more| ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND | more, Key::G)));
        let held = ui.ctx().format_modifiers(Modifiers::COMMAND);
        let Contents { feed, draft, real, grouping, marks, focus, inside, wish, .. } = self;
        let mut asking = Asking { feed, draft: draft.as_mut(), real };
        // A node of a group that was clicked there, to turn to; and what the form asked for.
        let (mut turn_to, mut created) = (None, None);
        column(ui, NARROWEST_DETAILS, |ui| {
            match focus {
                Focus::Nothing => {
                    if let Some(draft) = &mut asking.draft {
                        details::held_back(ui, draft, asking.real);
                        ui.separator();
                    }
                    ui.weak(
                        "Click a node to see where it runs, or pick a kind of node among the runtimes to create one.",
                    );
                    ui.weak("Drag from an output to an input to connect them. Right-click a wire or a pin to disconnect it.");
                    ui.weak("Pause a node with the button on its card. Its details have the rest.");
                    ui.weak("Click a group to pause, start again or remove all of its nodes together.");
                    ui.weak(
                        "A node that is clicked keeps its colour, with all that feeds it and all that it feeds. The rest goes grey.",
                    );
                    ui.weak(format!(
                        "Hold Shift and click nodes, or drag across them, to select several. A click beside the \
                         nodes lets go of them all, and a click on one with {held} held lets go of that one."
                    ));
                    ui.weak(format!(
                        "{make} makes a group of what is selected, which is drawn as one node, and {into} puts it \
                         into the group that was clicked last. Tab, or a double click, shows what is inside a group, \
                         and Tab leaves it again. Inside, P moves what is selected one group out. {apart} takes a \
                         group apart."
                    ));
                    ui.weak(
                        "Inside a group, Group Input and Group Output have the ports outside it: those it is \
                         connected with, and those of the nodes around it that nothing is connected to yet.",
                    );
                }
                Focus::Node { key, settings } => {
                    if let Some((home, node)) = node(&inventories, key) {
                        details::node_details(ui, &mut asking, marks, home, node, settings, &rates);
                    }
                }
                Focus::Group { name, renamed } => {
                    let told = details::Told {
                        path: name,
                        nodes: grouping.iter().filter(|(_, of)| graph::within(of, name)).collect(),
                        mark: marks.card(&graph::group_key(name)),
                        shown: inside.as_deref() == Some(name.as_str()),
                    };
                    turn_to = details::group_details(ui, &mut asking, &inventories, &told, renamed, wish);
                }
                Focus::Form(form) => match form.show(ui, &mut asking, &inventories) {
                    Filled::Open => {}
                    Filled::Created { nodes } => created = Some(nodes),
                    Filled::Closed => *focus = Focus::Nothing,
                },
            }
            if !chosen.is_empty() && matches!(focus, Focus::Node { .. } | Focus::Group { .. }) {
                details::grouping(ui, &chosen, &here, in_a_group, [&make, "P"], wish);
            }
        });
        if let Some((key, (_, node))) = turn_to.and_then(|key| Some((key.clone(), node(&inventories, &key)?))) {
            *focus = Focus::node(key, real, &node.params);
        }
        // What is created is where the graph is looking: in the group whose inside it shows. A
        // group that its runtime offers as a kind is a group already, and is put there as one.
        if let (Some(nodes), Some(shown)) = (created, self.inside.clone()) {
            let into = |group: String| if group.is_empty() { shown.clone() } else { graph::join(Some(&shown), &group) };
            self.regroup(nodes.into_iter().map(|(node, group)| (node, Some(into(group)))).collect());
        }
    }

    /// The graph, under the buttons that lay it out, bring it into view and begin a draft.
    fn graph(&mut self, ui: &mut Ui) {
        egui::Panel::top("graph-controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Tidy").on_hover_text("Lay the nodes out again, by what feeds what").clicked() {
                    graph::tidy(&mut self.snarl);
                    // Laid out afresh, the cards are where they belong now: where they were put is forgotten.
                    // Inside a group that goes for the cards there, and the others stay where they are.
                    let shown: Vec<String> = self.snarl.nodes().map(Card::key).collect();
                    self.places.retain(|card, _| self.inside.is_some() && !shown.contains(card));
                    self.fit = true;
                }
                self.fit |= ui.small_button("Fit").on_hover_text("Bring every node into view").clicked();
                ui.separator();
                self.drafting(ui);
                // Inside a group, the way out is at the far end, where Blender has it.
                if let Some(group) = &self.inside {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let about = "Show the whole machine again. Tab does the same";
                        if ui.small_button("Exit Group").on_hover_text(about).clicked() {
                            self.wish = Some(Wish::Leave);
                        }
                        ui.label(format!("inside {group}"));
                    });
                }
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
        let rates = self.rates.clone();
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
        if let Some(wish) = self.typed(ui, canvas) {
            self.wish = Some(wish);
        }
        // With a card selected, only what feeds it and what it feeds keep their colour. A node that
        // is inside a group is the card of that group, where the graph shows the group from outside.
        let lit = self.card(self.inside.as_deref());
        let lit = lit.map(|card| graph::flow(&self.snarl, &card)).filter(|lit| !lit.is_empty());
        let double = ui.input(|input| input.pointer.button_double_clicked(egui::PointerButton::Primary));
        let mut viewer = Viewer {
            asking: Asking { feed: &self.feed, draft: self.draft.as_mut(), real: &self.real },
            marks: &self.marks,
            colors: &self.colors,
            dark: ui.visuals().dark_mode,
            focus: &mut self.focus,
            lit,
            rates: &rates,
            frame,
            carried,
            reveal: &mut self.reveal,
            double,
            wish: &mut self.wish,
        };
        // Text that could be selected would swallow the press that moves, selects or opens the menu of a card.
        ui.style_mut().interaction.selectable_labels = false;
        // The widget is known by a name of its own, so that what is selected in it can be asked for.
        let name = egui::Id::unique("triq-graph");
        SnarlWidget::new().id(name).style(style).show(&mut self.snarl, &mut viewer, ui);
        let selected = triq_snarl::ui::get_selected_nodes(name, ui.ctx());
        let cards = selected.into_iter().filter_map(|id| self.snarl.get_node(id)).map(Card::key);
        // The two cards that stand for the outside of a group are nothing to put in a group or move.
        self.selected = cards.filter(|card| !card.starts_with("//")).collect();
    }

    /// What the keys ask for while the pointer is over the graph. They are
    /// Blender's, with the command key of a Mac for its Ctrl: Cmd+G makes a
    /// group of the cards that are selected, Cmd+Shift+G puts them into the
    /// group that is selected, Tab shows the inside of that group, or else
    /// leaves the group that is shown, as Ctrl+Tab does, P moves the selected
    /// cards one group out, and Cmd+Alt+G takes the selected group apart.
    fn typed(&mut self, ui: &Ui, canvas: Rect) -> Option<Wish> {
        let tabbed = std::mem::take(&mut self.tabbed);
        if !ui.rect_contains_pointer(canvas) || ui.ctx().text_edit_focused() {
            return None;
        }
        let pressed = |modifiers, key| ui.input_mut(|input| input.consume_key(modifiers, key));
        // On a Mac the command key, and Ctrl as well: other machines have the one key for both.
        let with_ctrl = |more, key| pressed(Modifiers::CTRL | more, key) || pressed(Modifiers::COMMAND | more, key);
        let (shown, inside) = (self.inside.as_deref(), self.inside.is_some());
        // The group that is selected is the one the details are about. Tab steps into it if its card is on the graph.
        let group = match &self.focus {
            Focus::Group { name, .. } => Some(name.clone()),
            _ => None,
        };
        let enter = group.clone().filter(|group| graph::parent(group) == shown);
        let cards = self.chosen();
        // The keys that ask for more are looked for first: with Alt or Shift, it is also the key without.
        if with_ctrl(Modifiers::ALT, Key::G) {
            group.map(Wish::Ungroup)
        } else if with_ctrl(Modifiers::SHIFT, Key::G) {
            enter.filter(|_| !cards.is_empty()).map(|group| Wish::Insert(cards, group))
        } else if with_ctrl(Modifiers::NONE, Key::G) {
            (!cards.is_empty()).then_some(Wish::Group(cards))
        } else if with_ctrl(Modifiers::NONE, Key::Tab) {
            inside.then_some(Wish::Leave)
        } else if tabbed || pressed(Modifiers::NONE, Key::Tab) {
            enter.map(Wish::Enter).or(inside.then_some(Wish::Leave))
        } else if inside && pressed(Modifiers::NONE, Key::P) {
            (!cards.is_empty()).then_some(Wish::Separate(cards))
        } else {
            None
        }
    }

    /// The cards that the keys and the buttons for groups are about: those
    /// that Shift selected, or else the card of what the details are about,
    /// if that is on the graph.
    fn chosen(&self) -> Vec<String> {
        if !self.selected.is_empty() {
            return self.selected.clone();
        }
        let shown = self.inside.as_deref();
        let own = match &self.focus {
            Focus::Node { key, .. } => (self.grouping.get(key).map(String::as_str) == shown).then(|| key.clone()),
            Focus::Group { name, .. } => (graph::parent(name) == shown).then(|| graph::group_key(name)),
            Focus::Nothing | Focus::Form(_) => None,
        };
        own.into_iter().collect()
    }

    /// The card that shows what the details are about, where the graph shows
    /// the inside of `shown`: the card of a node or a group, or that of the
    /// group there which it is inside of. `None` if it is not in there.
    fn card(&self, shown: Option<&str>) -> Option<String> {
        match &self.focus {
            Focus::Node { key, .. } => match self.grouping.get(key).map(String::as_str) {
                group if group == shown => Some(key.clone()),
                group => graph::child(group?, shown).map(graph::group_key),
            },
            Focus::Group { name, .. } => graph::child(name, shown).map(graph::group_key),
            Focus::Nothing | Focus::Form(_) => None,
        }
    }

    /// Do what was asked for.
    fn grant(&mut self, wish: Wish) {
        // Each node that is to be in another group than it is in, with that group.
        let mut moves = Vec::new();
        match wish {
            Wish::Enter(group) => (self.inside, self.fit) = (Some(group), true),
            Wish::Leave => {
                // One group out, the group that was left is the one that is selected.
                if let Some(group) = self.inside.take() {
                    self.inside = graph::parent(&group).map(str::to_owned);
                    self.focus.group(&group);
                }
                self.fit = true;
            }
            Wish::Group(cards) => {
                // The new group is inside the group that is shown, if one is.
                let group = self.free((1..).map(|number| format!("group{number}")), self.inside.as_deref());
                // Its card goes where its cards are: in the middle of them.
                let at: Vec<[f32; 2]> =
                    cards.iter().filter_map(|card| self.places.get(card)).map(|place| place.at).collect();
                if !at.is_empty() {
                    let middle = [0, 1].map(|axis| at.iter().map(|at| at[axis]).sum::<f32>() / at.len() as f32);
                    self.places.insert(graph::group_key(&group), Place { at: middle, seen: now() });
                }
                cards.iter().for_each(|card| moves.extend(self.moved(card, Some(&group))));
                self.focus.group(&group);
                self.reveal = true;
            }
            Wish::Insert(cards, group) => {
                // A group is not put inside itself, nor inside a group that is inside it.
                let fits = |card: &&String| card.strip_prefix('/').is_none_or(|inner| !graph::within(&group, inner));
                cards.iter().filter(fits).for_each(|card| moves.extend(self.moved(card, Some(&group))));
            }
            Wish::Ungroup(group) => {
                // What was in the group is where the group was: its nodes, and the groups inside it.
                let out = graph::parent(&group);
                let nodes = self.grouping.iter().filter(|(_, of)| **of == group);
                moves.extend(nodes.map(|(node, _)| (node.clone(), out.map(str::to_owned))));
                let inner = self.grouping.values().filter_map(|of| graph::child(of, Some(&group)));
                let inner: std::collections::BTreeSet<&str> = inner.collect();
                inner.into_iter().for_each(|inner| moves.extend(self.moved(&graph::group_key(inner), out)));
            }
            Wish::Separate(cards) => {
                // One group out: into the group that the shown one is inside of, or into none.
                let Some(shown) = &self.inside else { return };
                cards.iter().for_each(|card| moves.extend(self.moved(card, graph::parent(shown))));
            }
            Wish::Rename(group, name) => self.rename(&group, &name),
        }
        self.regroup(moves);
    }

    /// Put each of some nodes, by `runtime/node`, in a group, or with `None`
    /// in none. Its runtime is asked for that; while drafting, it is held back.
    fn regroup(&mut self, moves: Vec<(String, Option<String>)>) {
        let mut asking = Asking { feed: &self.feed, draft: self.draft.as_mut(), real: &self.real };
        for (node, group) in moves {
            if let Some((runtime, node)) = node.split_once('/') {
                let (runtime, node, group) = (runtime.to_owned(), node.to_owned(), group.unwrap_or_default());
                asking.ask(Request::SetGroup { runtime, node, group });
            }
        }
    }

    /// The nodes that a card stands for, each with the group it is in once
    /// the card is moved into `group`, or with `None` out of every group. A
    /// card that is a group keeps what it is called, unless a group of that
    /// name is there already.
    fn moved(&self, card: &str, group: Option<&str>) -> Vec<(String, Option<String>)> {
        let Some(inner) = card.strip_prefix('/') else {
            return vec![(card.to_owned(), group.map(str::to_owned))];
        };
        // A group that is there already stays as it is.
        if graph::parent(inner) == group {
            return Vec::new();
        }
        let name = graph::called(inner);
        let to =
            self.free(std::iter::once(name.to_owned()).chain((2..).map(|number| format!("{name}{number}"))), group);
        let nodes = self.grouping.iter().filter(|(_, of)| graph::within(of, inner));
        nodes.map(|(node, of)| (node.clone(), graph::replant(of, inner, Some(&to)))).collect()
    }

    /// The first of some names that no group inside `group` has, or with
    /// `None` no group that is inside no other: the new group, by where it is.
    fn free(&self, mut names: impl Iterator<Item = String>, group: Option<&str>) -> String {
        let taken = |path: &String| self.grouping.values().any(|of| graph::within(of, path));
        let free = names.by_ref().map(|name| graph::join(group, &name)).find(|path| !taken(path));
        free.unwrap_or_default()
    }

    /// Call a group by another name, unless a group beside it has that name:
    /// every node in it, and in the groups inside it, is put in the group of
    /// the new name. The name is plain, as every name that a runtime keeps
    /// is: what is not a letter, a digit, `-`, `_` or `.` becomes `_`, and
    /// it does not begin with one.
    fn rename(&mut self, group: &str, name: &str) {
        let plain = |letter: char| if letter.is_ascii_alphanumeric() || "-_.".contains(letter) { letter } else { '_' };
        let name: String = name.trim().chars().map(plain).take(32).collect();
        let name = name.trim_start_matches('_').to_owned();
        let to = graph::join(graph::parent(group), &name);
        if name.is_empty() || self.grouping.values().any(|taken| graph::within(taken, &to)) {
            return;
        }
        let renamed = |of: &str| graph::replant(of, group, Some(&to)).unwrap_or_default();
        let nodes = self.grouping.iter().filter(|(_, of)| graph::within(of, group));
        self.regroup(nodes.map(|(node, of)| (node.clone(), Some(renamed(of)))).collect());
        // Its cards, and those of the groups inside it, go by the new name and are where they were.
        let places: Vec<(String, Place)> = self.places.iter().map(|(card, place)| (card.clone(), *place)).collect();
        for (card, place) in places {
            for kind in ["//in/", "//out/", "/"] {
                if let Some(of) = card.strip_prefix(kind).filter(|of| graph::within(of, group)) {
                    self.places.insert(format!("{kind}{}", renamed(of)), place);
                    break;
                }
            }
        }
        if let Some(shown) = self.inside.as_mut().filter(|shown| graph::within(shown, group)) {
            *shown = renamed(shown);
        }
        self.focus = Focus::Group { name: to, renamed: name };
    }

    fn charts(&mut self, ui: &mut Ui) {
        // A chart ends at the present, and the present moves on. While there is a
        // line to slide along, the window is drawn again as soon as it can be.
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
        if !self.charts.is_empty() {
            ui.ctx().request_repaint();
        }
        let recorded = Recorded { history: &self.feed.history(), signals: self.feed.signals() };
        self.charts.show(ui, recorded, &self.colors, now);
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

    fn raw_input_hook(&mut self, window: &egui::Context, typed: &mut egui::RawInput) {
        self.keys(window, typed);
    }
}

/// The least width that the list of runtimes is laid out in, and the details. See [`column`].
const NARROWEST_LIST: f32 = 170.0;
const NARROWEST_DETAILS: f32 = 220.0;

/// What a window holds, in a column that scrolls up and down. It is laid out
/// in the width the window has: what is written wraps, and what is to be
/// filled in has less room. Below `narrowest` nothing is squeezed any
/// further: a window that is made narrower than that cuts off what does not fit.
fn column(ui: &mut Ui, narrowest: f32, contents: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.set_width(ui.available_width().max(narrowest));
        contents(ui);
    });
}

/// The present, in seconds since 1970.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// A node of a machine by `runtime/node`, with what its runtime reports.
fn node<'a>(machine: &'a [Inventory], key: &str) -> Option<(&'a Inventory, &'a NodeInfo)> {
    let (runtime, name) = key.split_once('/')?;
    let home = machine.iter().find(|inventory| inventory.runtime == runtime)?;
    Some((home, home.nodes.iter().find(|node| node.name == name)?))
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

/// How far the view can be zoomed out: far enough to see a large machine whole.
const SMALLEST: f32 = 0.05;

/// The view that shows every card, no larger than life.
fn framing(snarl: &Snarl<Card>, canvas: Rect) -> TSTransform {
    let card = |(pos, card): (egui::Pos2, &Card)| Rect::from_min_size(pos, graph::size(card));
    let cards = snarl.nodes_pos().map(card).fold(Rect::NOTHING, Rect::union).expand(40.0);
    let scale = (canvas.width() / cards.width()).min(canvas.height() / cards.height()).clamp(SMALLEST, 1.0);
    TSTransform::new(canvas.center().to_vec2() - cards.center().to_vec2() * scale, scale)
}
