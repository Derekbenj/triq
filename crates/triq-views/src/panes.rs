//! How `egui_tiles` is asked to arrange panes: the views here, and the windows
//! and charts of the workbench, by the same rules. Anything can go into anything.
//!
//! A pane is put down on a half of another, or on its middle, and it matters
//! how high. High, where the highlight takes in the bar above the other pane,
//! it stays on that pane's level: a neighbour on a half, one more name on the
//! same bar on the middle. Low, where the highlight is all under that bar, it
//! goes in with the pane: the two are a group, side by side or one under the
//! other on a half, one in front of the other on the middle. A group has a
//! bar of its own that names what is in it, and is taken by that name, or
//! closed, like a pane. Groups go into groups.
//!
//! A pane is also let go on the name of another, on that one's bar. Then it is
//! put beside what the name is of, on its right, and the two are a group
//! where the other was. On the name of a group, it joins the group at its end.
//! That is for the middle of a name: nearer an end of it, the pane goes
//! between two names, and is one more name on the bar.
use egui::{Rect, Response, vec2};
use egui_tiles::{Container, LinearDir, SimplificationOptions, Tabs, Tile, TileId, Tiles, Tree};

/// Every pane is under a bar, also one that has its place to itself. A group
/// keeps its bar with nothing beside it too, and bars under bars stay as they are.
pub fn rules() -> SimplificationOptions {
    SimplificationOptions {
        all_panes_must_have_tabs: true,
        flatten_tabs_in_tabs: false,
        prune_single_child_tabs: false,
        ..SimplificationOptions::default()
    }
}

/// The panes in a place, in the order they are laid out in, also those that are closed.
pub fn panes_in<Pane>(tiles: &Tiles<Pane>, place: TileId) -> Vec<TileId> {
    match tiles.get(place) {
        Some(Tile::Pane(_)) => vec![place],
        Some(Tile::Container(holder)) => holder.children().flat_map(|part| panes_in(tiles, *part)).collect(),
        None => Vec::new(),
    }
}

/// What is carried by its name while a tree is drawn, and the name of another
/// that it is held over. Made before the tree is drawn, told of every name by
/// `Behavior::on_tab_button`, asked for the highlight by
/// `Behavior::paint_drag_preview`, and given to [`tidy`] once the tree is drawn.
pub struct Carried {
    what: Option<TileId>,
    /// What is carried and everything in it: nothing joins itself, or what is inside it.
    inside: Vec<TileId>,
    /// Whether it is let go in this frame.
    let_go: bool,
    /// The name it is over the middle of, and where it lands if it is let go there.
    over: Option<(TileId, Rect)>,
}

impl Carried {
    pub fn of<Pane>(tree: &Tree<Pane>, ctx: &egui::Context) -> Self {
        let what = tree.dragged_id(ctx);
        let inside = what.map_or(Vec::new(), |what| places_in(&tree.tiles, what));
        Self { what, inside, let_go: ctx.input(|input| input.pointer.any_released()), over: None }
    }

    /// Call for each name as its bar is drawn. `place` is what the name is of.
    pub fn name<Pane>(&mut self, tiles: &Tiles<Pane>, place: TileId, name: &Response) {
        // The ends of a name are left to what goes between two names.
        let middle = name.rect.shrink2(vec2(name.rect.width() / 4.0, 0.0));
        let pointer = name.ctx.input(|input| input.pointer.interact_pos());
        if self.what.is_none() || self.inside.contains(&place) || !pointer.is_some_and(|at| middle.contains(at)) {
            return;
        }
        // It lands at the end of a group, with a share as large as each in the group has, and
        // on the right half of anything else.
        let (members, sideways) = match tiles.get(place) {
            Some(Tile::Container(Container::Linear(group))) => {
                let shown = group.children.iter().filter(|member| tiles.is_visible(**member)).count();
                (shown, group.dir == LinearDir::Horizontal)
            }
            _ => (1, true),
        };
        // What a name is of comes to the front when something is carried over the name, and has
        // the room under the bar from the next frame on. Until then the name stands for that room.
        let under = tiles.rect(place).unwrap_or(name.rect);
        let share = members as f32 / (members + 1) as f32;
        let landing = match sideways {
            true => under.split_left_right_at_fraction(share).1,
            false => under.split_top_bottom_at_fraction(share).1,
        };
        self.over = Some((place, landing));
    }

    /// Paint the highlight that shows where what is carried will land. `around` and `landing`
    /// are where the tree has it going, which holds unless it is over a name.
    pub fn highlight(&self, visuals: &egui::Visuals, painter: &egui::Painter, around: Option<Rect>, landing: Rect) {
        let stroke = visuals.selection.stroke;
        let (around, landing) = self.over.map_or((around, landing), |(_, beside)| (None, beside));
        if let Some(around) = around {
            painter.rect_stroke(around, 1.0, stroke, egui::StrokeKind::Inside);
        }
        painter.rect(landing, 1.0, stroke.color.gamma_multiply(0.5), stroke, egui::StrokeKind::Inside);
    }
}

/// Call after the tree was drawn, with what was carried as it was drawn. True
/// if the arrangement was changed, and is to be drawn again.
pub fn tidy<Pane>(tree: &mut Tree<Pane>, carried: &Carried) -> bool {
    let Some(what) = carried.what else { return undouble(tree) };
    // The tree has put it down where it had it going. Let go on a name, it is taken from there.
    let joined = carried.let_go && carried.over.is_some_and(|(onto, _)| join(tree, what, onto));
    joined | keep_grouped(tree, what) | undouble(tree)
}

/// What was let go on a name is put with what the name is of: at the end of a
/// group, and on the right of anything else, in a new group of the two that
/// has the other's place.
fn join<Pane>(tree: &mut Tree<Pane>, carried: TileId, onto: TileId) -> bool {
    let group = match tree.tiles.get(onto) {
        Some(Tile::Container(Container::Linear(_))) => onto,
        Some(_) => {
            let holder = tree.tiles.parent_of(onto);
            let row = tree.tiles.insert_horizontal_tile(vec![onto]);
            put_in_place_of(tree, holder, onto, row);
            row
        }
        None => return false,
    };
    tree.move_tile_to_container(carried, group, usize::MAX, false);
    // The group is the one seen, of those that share its place.
    tree.make_active(|place, _| place == group);
    true
}

/// A place, and every place inside it.
fn places_in<Pane>(tiles: &Tiles<Pane>, place: TileId) -> Vec<TileId> {
    let inside: Vec<TileId> = match tiles.get(place) {
        Some(Tile::Container(holder)) => holder.children().flat_map(|part| places_in(tiles, *part)).collect(),
        _ => Vec::new(),
    };
    [vec![place], inside].concat()
}

/// A group is taken by its name, which carries what is in the group and
/// leaves the bar behind. Put down beside something, it is given a bar again.
fn keep_grouped<Pane>(tree: &mut Tree<Pane>, carried: TileId) -> bool {
    let arrangement =
        matches!(tree.tiles.get(carried), Some(Tile::Container(Container::Linear(_) | Container::Grid(_))));
    let holder = tree.tiles.parent_of(carried);
    if !arrangement || holder.is_some_and(|holder| bar(&tree.tiles, holder).is_some()) {
        return false;
    }
    let group = tree.tiles.insert_tab_tile(vec![carried]);
    put_in_place_of(tree, holder, carried, group);
    true
}

/// A bar right under another is one too many when either has a single name
/// on it: the two would say the same. One such pair is put right at a time.
fn undouble<Pane>(tree: &mut Tree<Pane>) -> bool {
    let tiles = &tree.tiles;
    // The bar that goes, and what takes its place.
    let pair = tiles.iter().find_map(|(outer, _)| {
        let names = &bar(tiles, *outer)?.children;
        names.iter().find_map(|inner| match bar(tiles, *inner)?.children[..] {
            [only] => Some((*inner, only)),
            _ if names.len() == 1 => Some((*outer, *inner)),
            _ => None,
        })
    });
    let Some((gone, stays)) = pair else { return false };
    let holder = tree.tiles.parent_of(gone);
    put_in_place_of(tree, holder, gone, stays);
    tree.tiles.remove(gone);
    true
}

/// The bars that are not to be shown: those with nothing behind their names
/// but one open pane, which has a bar of its own. A group of which all but
/// one are closed looks like that one alone, until the others are opened again.
pub fn idle_bars<Pane>(tiles: &Tiles<Pane>) -> Vec<TileId> {
    let idle = |place: &TileId| {
        let open: Vec<TileId> = panes_in(tiles, *place).into_iter().filter(|pane| tiles.is_visible(*pane)).collect();
        bar(tiles, *place).is_some_and(|names| open.len() == 1 && !names.children.contains(&open[0]))
    };
    tiles.tile_ids().filter(idle).collect()
}

fn bar<Pane>(tiles: &Tiles<Pane>, place: TileId) -> Option<&Tabs> {
    match tiles.get(place) {
        Some(Tile::Container(Container::Tabs(bar))) => Some(bar),
        _ => None,
    }
}

/// Where `old` was, in `holder` or as all there is, `new` is from now on.
fn put_in_place_of<Pane>(tree: &mut Tree<Pane>, holder: Option<TileId>, old: TileId, new: TileId) {
    match holder.and_then(|holder| tree.tiles.get_mut(holder)) {
        Some(Tile::Container(holder)) => _ = holder.replace_child(old, new),
        _ => tree.root = Some(new),
    }
}
