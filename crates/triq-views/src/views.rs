//! The list of views and their panes.
//!
//! What is in a pane is the business of the view: its [`Kind`] draws it.
//! Where the pane is, is the user's. A view is listed on the left as soon as
//! its node exists, and has a pane, which is taken by its name and put down
//! beside another, in its place, or in with it as a group (see `panes.rs`),
//! resized by the line between two, and closed. A view that is closed is
//! still listed, to be opened again or dragged to where it should be.
use crate::panes;
use egui::{Sense, Ui};
use egui_tiles::{Behavior, Container, SimplificationOptions, Tabs, Tile, TileId, Tiles, Tree, UiResponse};
use serde::{Deserialize, Serialize};
use triq::Runtime;

/// One kind of view, as the window sees it: the views of that kind there
/// are, and how each is drawn. The nodes behind the views are the kind's own
/// business: it offers them on the runtime, and runs them or lets the runtime.
pub trait Kind {
    /// The views of this kind there are now, by the names of their nodes.
    /// Called once per frame, before any view is drawn.
    fn names(&mut self) -> Vec<String>;

    /// Draw the view of that name in the room `ui` has.
    fn show(&mut self, ui: &mut Ui, name: &str);
}

/// The views of one program: every kind it has, a list of the views there
/// are, and a pane for each. [`crate::run`] makes one and is the whole
/// program; a program that has a window already makes its own and draws it
/// with [`Views::show`].
pub struct Views {
    kinds: Vec<Box<dyn Kind>>,
    /// A pane for every view there is or was, and how those that are shown share the room.
    panes: Tree<Place>,
    /// The views there were when the window was last drawn.
    there: Vec<String>,
    window: egui::Context,
    /// The runtime the views are nodes of. Declared last, so it goes after them.
    runtime: Runtime,
}

/// The pane of one view.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Place {
    /// The name of the view's node. A pane outlasts the node, out of sight:
    /// should a node of that name come back, its view is where the other was.
    view: String,
    /// Whether the user has the view open. It is shown while it is open and its node exists.
    open: bool,
}

impl Views {
    /// `runtime` is the program's own: the views are nodes of it. `window` is what draws them.
    pub fn new(runtime: Runtime, window: &egui::Context) -> Self {
        let panes = Tree::empty(egui::Id::unique("views"));
        Self { kinds: Vec::new(), panes, there: Vec::new(), window: window.clone(), runtime }
    }

    /// The runtime to offer a kind of view on.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The window, for a kind whose views ask to be drawn when they have something new.
    pub fn window(&self) -> &egui::Context {
        &self.window
    }

    /// One more kind of view.
    pub fn add(&mut self, kind: impl Kind + 'static) {
        self.kinds.push(Box::new(kind));
    }

    /// How the views are arranged, to be handed to [`Views::arrange`] by the
    /// next run of the program. Only the views there are now are in it.
    /// `None` while there are none: what was kept before is then worth more.
    pub fn layout(&self) -> Option<String> {
        let mut kept = self.panes.clone();
        let gone = |tile: &Tile<Place>| matches!(tile, Tile::Pane(place) if !self.there.contains(&place.view));
        let gone: Vec<TileId> = kept.tiles.iter().filter(|(_, tile)| gone(tile)).map(|(pane, _)| *pane).collect();
        for pane in gone {
            kept.remove_recursively(pane);
        }
        let any = kept.tiles.tiles().any(|tile| tile.is_pane());
        any.then(|| serde_json::to_string(&kept).ok()).flatten()
    }

    /// Arrange the views as [`Views::layout`] said they were. A view takes its
    /// place when its node exists; one that the layout does not know is put
    /// beside the others. What cannot be read as a layout changes nothing.
    pub fn arrange(&mut self, layout: &str) {
        if let Ok(panes) = serde_json::from_str(layout) {
            self.panes = panes;
        }
    }

    /// Draw one frame: the list of views on the left, and their panes in the rest.
    pub fn show(&mut self, ui: &mut Ui) {
        // The views there are, each with the kind that draws it.
        let mut views = Vec::new();
        for (kind, drawn_by) in self.kinds.iter_mut().enumerate() {
            views.extend(drawn_by.names().into_iter().map(|view| (view, kind)));
        }
        // A view that is new has a pane from now on, open.
        for (view, _) in &views {
            if self.pane(view).is_none() {
                self.place(view.clone());
            }
        }
        egui::Panel::left("views").default_size(170.0).show(ui, |ui| self.list(ui, &views));
        // A pane is shown while its view is open and there.
        let there = |place: &Place| views.iter().any(|(view, _)| *view == place.view);
        let shown = self.panes.tiles.iter().filter_map(|(pane, tile)| match tile {
            Tile::Pane(place) => Some((*pane, place.open && there(place))),
            Tile::Container(_) => None,
        });
        for (pane, shown) in shown.collect::<Vec<_>>() {
            self.panes.set_visible(pane, shown);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if views.is_empty() {
                ui.weak(
                    "Views appear here. This program's runtime offers kinds of view: \
                     in the workbench, create one and drag an output to its input.",
                );
            }
            let idle = panes::idle_bars(&self.panes.tiles);
            let mut drawn = Drawn { kinds: &mut self.kinds, views: &views, idle, closed: None };
            let carried = self.panes.dragged_id(ui.ctx());
            self.panes.ui(&mut drawn, ui);
            // A view that is closed is only out of sight. Its pane keeps its place, for when it is opened again.
            if let Some(place) = drawn.closed {
                for pane in panes::panes_in(&self.panes.tiles, place) {
                    self.open(pane, false);
                }
            }
            if panes::tidy(&mut self.panes, carried) {
                ui.ctx().request_repaint();
            }
        });
        self.there = views.into_iter().map(|(view, _)| view).collect();
    }

    /// The pane of a view, and whether the user has it open.
    fn pane(&self, view: &str) -> Option<(TileId, bool)> {
        self.panes.tiles.iter().find_map(|(pane, tile)| match tile {
            Tile::Pane(place) if place.view == view => Some((*pane, place.open)),
            _ => None,
        })
    }

    fn open(&mut self, pane: TileId, open: bool) {
        if let Some(Tile::Pane(place)) = self.panes.tiles.get_mut(pane) {
            place.open = open;
        }
    }

    /// Give a new view its pane. The views share the room as a grid, until
    /// the user arranges them another way: then a new one goes beside what is there.
    fn place(&mut self, view: String) {
        let pane = self.panes.tiles.insert_pane(Place { view, open: true });
        let root = self.panes.root;
        self.panes.root = Some(match root.and_then(|root| Some((root, self.panes.tiles.get_mut(root)?))) {
            Some((root, Tile::Container(Container::Grid(grid)))) => {
                grid.add_child(pane);
                root
            }
            Some((root, _)) => self.panes.tiles.insert_grid_tile(vec![root, pane]),
            None => pane,
        });
    }

    /// Every view by name. One that is open is marked. A click opens or closes
    /// a view, and it is taken from here to be put down among the panes.
    fn list(&mut self, ui: &mut Ui, views: &[(String, usize)]) {
        ui.strong("Views");
        if views.is_empty() {
            ui.weak("There are no views yet.");
        }
        for (view, _) in views {
            let Some((pane, open)) = self.pane(view) else { continue };
            let entry = ui.add(egui::Button::selectable(open, view).sense(Sense::click_and_drag()));
            let entry = entry.on_hover_text("Click to open or close this view. Drag it to where it should be.");
            if entry.clicked() {
                self.open(pane, !open);
                self.panes.make_active(|tile, _| tile == pane);
            }
            // Taken from the list, a view is open, and is carried like a pane that is taken by its name.
            if entry.drag_started() {
                self.open(pane, true);
                ui.ctx().set_dragged_id(pane.egui_id(self.panes.id()));
            }
        }
    }
}

/// How the panes are drawn: each is a view, under a bar that has its name on it.
struct Drawn<'a> {
    kinds: &'a mut [Box<dyn Kind>],
    /// The views there are, each with the kind that draws it.
    views: &'a [(String, usize)],
    /// The bars of groups of which only one view is shown: they are not drawn.
    idle: Vec<TileId>,
    /// The place whose cross was pressed, to be closed once everything is drawn.
    closed: Option<TileId>,
}

impl Behavior<Place> for Drawn<'_> {
    /// What is in a pane is the view's business: its kind draws it, in the room the pane has.
    fn pane_ui(&mut self, ui: &mut Ui, _: TileId, place: &mut Place) -> UiResponse {
        if let Some((view, kind)) = self.views.iter().find(|(view, _)| *view == place.view) {
            egui::CentralPanel::default().show(ui, |ui| self.kinds[*kind].show(ui, view));
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, place: &Place) -> egui::WidgetText {
        place.view.as_str().into()
    }

    /// The name of a group on a bar lists the views in it that are shown.
    fn tab_title_for_tile(&mut self, tiles: &Tiles<Place>, place: TileId) -> egui::WidgetText {
        let shown = panes::panes_in(tiles, place).into_iter().filter(|pane| tiles.is_visible(*pane));
        let names: Vec<&str> =
            shown.filter_map(|pane| tiles.get_pane(&pane)).map(|place| place.view.as_str()).collect();
        names.join(", ").into()
    }

    fn on_tab_button(&mut self, _: &mut Tiles<Place>, _: TileId, name: egui::Response) -> egui::Response {
        name.on_hover_text(
            "Drag the name to put the view beside another, in with it, or onto its middle to share its place",
        )
    }

    fn is_tab_bar_visible(&self, place: TileId, _: &Tabs) -> bool {
        !self.idle.contains(&place)
    }

    fn is_tab_closable(&self, _: &Tiles<Place>, _: TileId) -> bool {
        true
    }

    /// Nothing is taken out of the arrangement: see where `closed` is read.
    fn on_tab_close(&mut self, _: &mut Tiles<Place>, place: TileId) -> bool {
        self.closed = Some(place);
        false
    }

    fn simplification_options(&self) -> SimplificationOptions {
        panes::rules()
    }
}
