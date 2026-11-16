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
use egui_tiles::{Container, SimplificationOptions, Tabs, Tile, TileId, Tiles, Tree};

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

/// Call after the tree was drawn, with what was being dragged then, if
/// anything. True if the arrangement was changed, and is to be drawn again.
pub fn tidy<Pane>(tree: &mut Tree<Pane>, carried: Option<TileId>) -> bool {
    carried.is_some_and(|carried| keep_grouped(tree, carried)) | undouble(tree)
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
