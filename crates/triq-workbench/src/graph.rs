//! From what the runtimes report to what is drawn: cards with pins down
//! their sides, and wires between the pins. Nothing here talks to a runtime
//! or to the screen.
//!
//! A card is a node, with a pin for each of its ports. Nodes can be put
//! together in a group, and a group is drawn as one card: its pins are the
//! ports of its nodes that are connected to nothing, or to something outside
//! the group. The inside of a group is a drawing of its own: of its nodes,
//! of the groups inside it, each as one card again, and of two more cards
//! that stand for the outside. Whatever the group hears from outside comes
//! from the one, and whatever it sends there goes to the other. A port that
//! is connected to nothing is on neither: it leads nowhere yet. A pin on one
//! of the two knows what it stands for out there, so that it can be wired
//! from inside the group (see `app/cards.rs`).
use eframe::egui::{Pos2, Vec2, pos2, vec2};
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use triq::{Direction, InputInfo, Inventory, NodeInfo, OutputInfo, PortSpec};

/// One box on the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub kind: Kind,
    /// The pins down its left side, where wires arrive.
    pub left: Vec<Pin>,
    /// The pins down its right side, where wires leave.
    pub right: Vec<Pin>,
}

/// What a card is the card of.
// Most cards are a node's, and there are not many cards: boxing the node would save nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// One node of one runtime.
    Node { runtime: String, node: NodeInfo },
    /// A group, drawn as one card: where it is among the groups, as
    /// `outer/inner`, and every node in it and in the groups inside it,
    /// each with its runtime.
    Group { path: String, members: Vec<(String, NodeInfo)> },
    /// Inside a group: the card that all it hears from outside comes from.
    Entry { group: String },
    /// Inside a group: the card that all it sends to the outside goes to.
    Exit { group: String },
}

/// A port of a node, as a pin: on the card of its node, on the card of the
/// group its node is in, or inside that group on a card that stands for the outside.
#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub runtime: String,
    pub node: String,
    pub port: Port,
    /// On the card of a group, and on a card that stands for the outside of
    /// one: the ports outside the group that this one is connected with,
    /// each `runtime/node/port`. For an input they are the outputs it hears
    /// from there, and for an output the inputs that hear it there.
    pub outside: Vec<String>,
}

/// What a runtime reports of a port.
#[derive(Debug, Clone, PartialEq)]
pub enum Port {
    Input(InputInfo),
    Output(OutputInfo),
}

impl Pin {
    pub fn spec(&self) -> &PortSpec {
        match &self.port {
            Port::Input(input) => &input.port,
            Port::Output(output) => &output.port,
        }
    }

    /// What the port is called: `runtime/node/port`.
    pub fn name(&self) -> String {
        format!("{}/{}/{}", self.runtime, self.node, self.spec().name)
    }

    /// `runtime/node`: the node it is a port of.
    pub fn owner(&self) -> String {
        format!("{}/{}", self.runtime, self.node)
    }

    /// The outputs an input is connected to. An output has none.
    fn heard(&self) -> impl Iterator<Item = &str> {
        let sources = match &self.port {
            Port::Input(input) => &input.sources[..],
            Port::Output(_) => &[],
        };
        sources.iter().map(|source| source.output.as_str())
    }

    /// Whether a wire runs from this pin to another: from an output to an
    /// input that is connected to it; or, inside a group, between a port and
    /// the pin that stands for it on a card for the outside.
    fn feeds(&self, to: &Pin) -> bool {
        let name = self.name();
        name == to.name() || to.heard().any(|output| output == name)
    }
}

/// What the card of a group is called across syncs. It begins with a slash,
/// which no `runtime/node` does.
pub fn group_key(group: &str) -> String {
    format!("/{group}")
}

/// What the three cards of a group are called: its own, and inside it the
/// two that stand for the outside, which begin with two slashes.
pub fn group_keys(group: &str) -> [String; 3] {
    [group_key(group), format!("//in/{group}"), format!("//out/{group}")]
}

/// The group that a group is inside of. `None` for one that is inside no other.
pub fn parent(group: &str) -> Option<&str> {
    group.rsplit_once('/').map(|(parent, _)| parent)
}

/// What a group is called among the groups beside it: the last part of `outer/inner`.
pub fn called(group: &str) -> &str {
    group.rsplit('/').next().unwrap_or(group)
}

/// A group by the group it is inside of, if any, and what it is called there.
pub fn join(parent: Option<&str>, name: &str) -> String {
    parent.map_or(name.to_owned(), |parent| format!("{parent}/{name}"))
}

/// Whether a group is `group`, or is inside it.
pub fn within(inner: &str, group: &str) -> bool {
    inner.strip_prefix(group).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Of the groups directly inside `level`, or for `None` of those that are
/// inside no other: the one that `group` is, or is inside of.
pub fn child<'a>(group: &'a str, level: Option<&str>) -> Option<&'a str> {
    let below = match level {
        Some(level) => group.strip_prefix(level)?.strip_prefix('/')?,
        None => group,
    };
    let name = below.split('/').next()?;
    Some(&group[..group.len() - below.len() + name.len()])
}

/// Where a node is once the group `from` has become `to`, if the node was in
/// `inner`, which is `from` or a group inside it. With `to` as `None` the
/// group is no more: what was in it is in no group, and the groups that
/// were inside it are inside no other.
pub fn replant(inner: &str, from: &str, to: Option<&str>) -> Option<String> {
    let rest = &inner[from.len()..];
    match to {
        Some(to) => Some(format!("{to}{rest}")),
        None => rest.strip_prefix('/').map(str::to_owned),
    }
}

impl Card {
    /// The card of a node: a pin for each of its ports.
    pub fn of(runtime: &str, node: &NodeInfo) -> Self {
        let pin = |port| Pin { runtime: runtime.to_owned(), node: node.name.clone(), port, outside: Vec::new() };
        Self {
            kind: Kind::Node { runtime: runtime.to_owned(), node: node.clone() },
            left: node.inputs.iter().cloned().map(Port::Input).map(pin).collect(),
            right: node.outputs.iter().cloned().map(Port::Output).map(pin).collect(),
        }
    }

    /// What identifies a card across syncs: `runtime/node` for a node.
    pub fn key(&self) -> String {
        match &self.kind {
            Kind::Node { runtime, node } => format!("{runtime}/{}", node.name),
            Kind::Group { path, .. } => group_key(path),
            Kind::Entry { group } => group_keys(group)[1].clone(),
            Kind::Exit { group } => group_keys(group)[2].clone(),
        }
    }

    /// What is written at the top of the card.
    pub fn name(&self) -> &str {
        match &self.kind {
            Kind::Node { node, .. } => &node.name,
            Kind::Group { path, .. } => called(path),
            Kind::Entry { .. } => "Group Input",
            Kind::Exit { .. } => "Group Output",
        }
    }

    /// The node, if this is the card of one.
    pub fn node(&self) -> Option<&NodeInfo> {
        match &self.kind {
            Kind::Node { node, .. } => Some(node),
            _ => None,
        }
    }

    /// The runtime of a node's card; and of a group's, if all its nodes run in one.
    pub fn runtime(&self) -> Option<&str> {
        match &self.kind {
            Kind::Node { runtime, .. } => Some(runtime),
            Kind::Group { members, .. } => {
                let (first, _) = members.first()?;
                members.iter().all(|(runtime, _)| runtime == first).then_some(first)
            }
            Kind::Entry { .. } | Kind::Exit { .. } => None,
        }
    }

    /// `runtime/node/port` of the port that a pin on the right stands for.
    pub fn output(&self, index: usize) -> String {
        self.right[index].name()
    }

    /// `runtime/node/port` of the port that a pin on the left stands for.
    pub fn input(&self, index: usize) -> String {
        self.left[index].name()
    }

    /// Outputs an input wants but cannot hear: they do not exist yet, or their program stopped.
    pub fn waiting(&self, index: usize) -> Vec<String> {
        let Port::Input(input) = &self.left[index].port else { return Vec::new() };
        input.sources.iter().filter(|source| !source.connected).map(|source| source.output.clone()).collect()
    }

    /// What is written beside a pin: `port: Type` on the card of its node,
    /// and `node.port: Type` on a card that shows the ports of several.
    pub fn written(&self, pin: &Pin) -> String {
        let spec = pin.spec();
        match &self.kind {
            Kind::Node { .. } => format!("{}: {}", spec.name, spec.message),
            _ => format!("{}.{}: {}", pin.node, spec.name, spec.message),
        }
    }

    /// Whether a wire runs from this card to another.
    fn feeds(&self, other: &Card) -> bool {
        other.left.iter().any(|to| self.right.iter().any(|from| from.feeds(to)))
    }

    /// Whether another card has the same pins: the same ports of the same nodes.
    fn same_pins(&self, other: &Card) -> bool {
        fn ports(card: &Card) -> impl Iterator<Item = (&String, &String, &PortSpec)> {
            card.left.iter().chain(&card.right).map(|pin| (&pin.runtime, &pin.node, pin.spec()))
        }
        [self.left.len(), self.right.len()] == [other.left.len(), other.right.len()] && ports(self).eq(ports(other))
    }
}

/// Which group each node is in, by `runtime/node`. A node has room for one.
/// A group that is inside another is written with the outer one first:
/// `outer/inner`. A group is there for as long as a node says it is in it.
pub type Groups = BTreeMap<String, String>;

/// Which group each node of a machine is in, by what its runtime reports. A
/// node that is in none is not listed.
pub fn groups(inventories: &[Inventory]) -> Groups {
    let nodes = inventories.iter().flat_map(|inventory| inventory.nodes.iter().map(move |node| (inventory, node)));
    let grouped = nodes.filter(|(_, node)| !node.group.is_empty());
    grouped.map(|(inventory, node)| (format!("{}/{}", inventory.runtime, node.name), node.group.clone())).collect()
}

/// The cards to draw of a machine whose nodes are grouped so.
///
/// Outside every group, with `inside` as `None`: a card for each node that is
/// in no group, and one for each group that is inside no other. Inside a
/// group: a card for each node that is in just that group, one for each
/// group directly inside it, and the two that stand for the outside.
///
/// A group shows the ports of its nodes, and of the nodes in the groups
/// inside it, that are connected to nothing or to something outside it.
/// What is connected only within the group is its own affair. The two cards
/// that stand for the outside have the ports that are connected to
/// something outside, and not those that are connected to nothing.
pub fn cards(inventories: &[Inventory], groups: &Groups, inside: Option<&str>) -> Vec<Card> {
    let nodes = || {
        let runtimes = inventories.iter();
        runtimes.flat_map(|inventory| inventory.nodes.iter().map(move |node| (inventory.runtime.as_str(), node)))
    };
    let group_of = |runtime: &str, node: &str| groups.get(&format!("{runtime}/{node}")).map(String::as_str);
    // Whether a node is in a group, or in a group inside it.
    let is_in = |of: Option<&str>, group: &str| of.is_some_and(|of| within(of, group));
    // The nodes of a group, in order of name: so its pins are, in whatever order the runtimes are listed.
    let members = |group: &str| -> Vec<(&str, &NodeInfo)> {
        let mut members: Vec<_> =
            nodes().filter(|(runtime, node)| is_in(group_of(runtime, &node.name), group)).collect();
        members.sort_by_key(|(runtime, node)| (*runtime, &node.name));
        members
    };
    // The group of the node that an output belongs to.
    let sent_from = |output: &str| {
        let mut parts = output.split('/');
        group_of(parts.next()?, parts.next()?)
    };
    // For each output, every input that hears it, with the group of its node.
    let mut heard_by: HashMap<&str, Vec<(String, Option<&str>)>> = HashMap::new();
    for (runtime, node) in nodes() {
        for input in &node.inputs {
            let name = format!("{runtime}/{}/{}", node.name, input.port.name);
            for source in &input.sources {
                heard_by.entry(&source.output).or_default().push((name.clone(), group_of(runtime, &node.name)));
            }
        }
    }
    // The ports of a group that lead outside it, on the left what it hears and on the right what
    // it sends: those that are connected to something outside, and if `unconnected`, to nothing.
    let shown = |group: &str, unconnected: bool| {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for (runtime, node) in members(group) {
            let pin = |port, outside| Pin { runtime: runtime.to_owned(), node: node.name.clone(), port, outside };
            for input in &node.inputs {
                let outputs = input.sources.iter().map(|source| &source.output);
                let outside: Vec<String> = outputs.filter(|output| !is_in(sent_from(output), group)).cloned().collect();
                if !outside.is_empty() || (unconnected && input.sources.is_empty()) {
                    left.push(pin(Port::Input(input.clone()), outside));
                }
            }
            for output in &node.outputs {
                let heard = heard_by.get(output.service.as_str());
                let inputs = heard.into_iter().flatten().filter(|(_, by)| !is_in(*by, group));
                let outside: Vec<String> = inputs.map(|(input, _)| input.clone()).collect();
                if !outside.is_empty() || (unconnected && heard.is_none()) {
                    right.push(pin(Port::Output(output.clone()), outside));
                }
            }
        }
        (left, right)
    };

    // What is drawn here: the nodes that are in just this group, and a card for each group directly inside it.
    let here = nodes().filter(|(runtime, node)| group_of(runtime, &node.name) == inside);
    let mut cards: Vec<Card> = here.map(|(runtime, node)| Card::of(runtime, node)).collect();
    let inner: BTreeSet<&str> =
        nodes().filter_map(|(runtime, node)| child(group_of(runtime, &node.name)?, inside)).collect();
    for path in inner {
        let members = members(path).into_iter().map(|(runtime, node)| (runtime.to_owned(), node.clone())).collect();
        let (left, right) = shown(path, true);
        cards.push(Card { kind: Kind::Group { path: path.to_owned(), members }, left, right });
    }
    // Inside a group, what it hears from outside and what it sends there has a card each.
    if let Some(group) = inside {
        let (heard, sent) = shown(group, false);
        if !heard.is_empty() {
            cards.push(Card { kind: Kind::Entry { group: group.to_owned() }, left: Vec::new(), right: heard });
        }
        if !sent.is_empty() {
            cards.push(Card { kind: Kind::Exit { group: group.to_owned() }, left: sent, right: Vec::new() });
        }
    }
    cards
}

/// Where a card is on the drawing, and when it was last there, in seconds since 1970.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub at: [f32; 2],
    pub seen: u64,
}

/// Where cards are and were, by what they are called. A node is known by its
/// name: when a program runs again, its cards are where they were.
pub type Places = BTreeMap<String, Place>;

/// How many cards are remembered from one run of the workbench to the next.
/// Programs come and go under names that are never used again, and their
/// cards are not kept for ever.
pub const REMEMBERED: usize = 500;

/// What is kept of `places` from one run to the next: the cards seen last.
pub fn remembered(places: &Places) -> Places {
    let mut newest: Vec<(&String, &Place)> = places.iter().collect();
    newest.sort_by_key(|(_, place)| Reverse(place.seen));
    newest.into_iter().take(REMEMBERED).map(|(key, place)| (key.clone(), *place)).collect()
}

/// Make the drawing match the machine, a card for every node. Cards keep the places the user gave them.
pub fn sync(snarl: &mut Snarl<Card>, inventories: &[Inventory]) {
    sync_with(snarl, inventories, &mut Places::new(), 0);
}

/// Make the drawing match the machine, a card for every node, with a memory
/// of where cards were: see [`show`].
pub fn sync_with(snarl: &mut Snarl<Card>, inventories: &[Inventory], places: &mut Places, now: u64) {
    show(snarl, cards(inventories, &Groups::new(), None), places, now);
}

/// Make the drawing show these cards, and the wires between them. A card that
/// is new to the drawing goes where `places` has it, if it was there before.
/// `places` is brought up to date with where every card is at `now`, in
/// seconds since 1970.
pub fn show(snarl: &mut Snarl<Card>, cards: Vec<Card>, places: &mut Places, now: u64) {
    // Where every card is, noted before any of them goes.
    for (place, card) in snarl.nodes_pos() {
        places.insert(card.key(), Place { at: [place.x, place.y], seen: now });
    }
    let mut wanted: BTreeMap<String, Card> = cards.into_iter().map(|card| (card.key(), card)).collect();

    // A card goes when what it stands for is gone, or when it no longer has the pins it had:
    // a different node took its name, or a group came to show other ports.
    let stale: Vec<_> = snarl
        .nodes_ids_data()
        .filter(|(_, drawn)| wanted.get(&drawn.value.key()).is_none_or(|card| !card.same_pins(&drawn.value)))
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        snarl.remove_node(id);
    }
    for (_, drawn) in snarl.nodes_ids_data_mut() {
        if let Some(card) = wanted.remove(&drawn.value.key()) {
            drawn.value = card;
        }
    }

    // What is left is new. Cards that were somewhere before go there first, so that the others are laid out around them.
    let (back, fresh): (Vec<Card>, Vec<Card>) = wanted.into_values().partition(|card| places.contains_key(&card.key()));
    for card in back.into_iter().chain(sources_first(fresh)) {
        let place = match places.get(&card.key()) {
            Some(Place { at: [x, y], .. }) => pos2(*x, *y),
            None => place(&snarl.nodes_pos().collect::<Vec<_>>(), &card),
        };
        places.insert(card.key(), Place { at: [place.x, place.y], seen: now });
        snarl.insert_node(place, card);
    }

    // A wire for every connection whose two ends are on the drawing. Inside a group, a wire
    // also joins each port that leads outside to the pin that stands for it on a card for the outside.
    let mut leaving: HashMap<String, Vec<OutPinId>> = HashMap::new();
    for (node, drawn) in snarl.nodes_ids_data() {
        for (output, pin) in drawn.value.right.iter().enumerate() {
            leaving.entry(pin.name()).or_default().push(OutPinId { node, output });
        }
    }
    let mut wires: HashSet<(OutPinId, InPinId)> = HashSet::new();
    for (node, drawn) in snarl.nodes_ids_data() {
        for (input, pin) in drawn.value.left.iter().enumerate() {
            let own = pin.name();
            let from = pin.heard().chain([own.as_str()]).filter_map(|name| leaving.get(name)).flatten();
            wires.extend(from.map(|from| (*from, InPinId { node, input })));
        }
    }
    let drawn: HashSet<(OutPinId, InPinId)> = snarl.wires().collect();
    for (from, to) in drawn.difference(&wires) {
        snarl.disconnect(*from, *to);
    }
    for (from, to) in wires.difference(&drawn) {
        snarl.connect(*from, *to);
    }
}

/// The cards that wires join to the card called `key`, also by way of other
/// cards, and that card itself. Which way a wire carries data does not
/// matter. Nothing, if no card on the drawing is called that.
pub fn joined(snarl: &Snarl<Card>, key: &str) -> BTreeSet<String> {
    let called = snarl.nodes_ids_data().filter(|(_, drawn)| drawn.value.key() == key);
    let mut reached: HashSet<NodeId> = called.map(|(id, _)| id).collect();
    let wires: Vec<[NodeId; 2]> = snarl.wires().map(|(from, to)| [from.node, to.node]).collect();
    // Each round takes in the cards one wire further out, until there is none.
    loop {
        let before = reached.len();
        for ends in &wires {
            if ends.iter().any(|end| reached.contains(end)) {
                reached.extend(ends);
            }
        }
        if reached.len() == before {
            break;
        }
    }
    reached.into_iter().map(|id| snarl[id].key()).collect()
}

const LEFT: f32 = 40.0;
const TOP: f32 = 40.0;
/// The room left between cards: across for the wires, down to tell the cards apart.
const GAP: Vec2 = vec2(90.0, 36.0);
/// How tall the row of a pin is drawn.
pub const ROW: f32 = 32.0;
/// How wide the space beside a pin is, where its rate is written.
pub const ASIDE: f32 = 48.0;

/// Lay every card out afresh, each one column right of whatever feeds it.
pub fn tidy(snarl: &mut Snarl<Card>) {
    let cards = sources_first(snarl.nodes().cloned().collect());
    let mut placed: Vec<(Pos2, &Card)> = Vec::new();
    for card in &cards {
        placed.push((place(&placed, card), card));
    }
    let places: HashMap<String, Pos2> = placed.into_iter().map(|(place, card)| (card.key(), place)).collect();
    for (_, drawn) in snarl.nodes_ids_data_mut() {
        drawn.pos = places[&drawn.value.key()];
    }
}

/// Each card after the cards that feed it, as far as loops allow.
fn sources_first(mut cards: Vec<Card>) -> Vec<Card> {
    let mut ordered = Vec::with_capacity(cards.len());
    while !cards.is_empty() {
        let unfed = |card: &Card| !cards.iter().any(|other| other.key() != card.key() && other.feeds(card));
        let next = cards.iter().position(unfed).unwrap_or(0);
        ordered.push(cards.remove(next));
    }
    ordered
}

/// Where a card goes among those already placed: right of every card that
/// feeds it, below whatever is already there. The card that stands for what
/// comes from outside a group goes left of all it feeds.
fn place(placed: &[(Pos2, &Card)], card: &Card) -> Pos2 {
    let feeders = placed.iter().filter(|(_, other)| other.feeds(card));
    let mut x = feeders.map(|(place, other)| place.x + size(other).x + GAP.x).fold(LEFT, f32::max);
    if matches!(card.kind, Kind::Entry { .. }) {
        let fed = placed.iter().filter(|(_, other)| card.feeds(other));
        x = fed.map(|(place, _)| place.x - size(card).x - GAP.x).fold(f32::INFINITY, f32::min).min(x);
    }
    let beside = placed.iter().filter(|(place, other)| place.x < x + size(card).x && x < place.x + size(other).x);
    let y = beside.map(|(place, other)| place.y + size(other).y + GAP.y).fold(TOP, f32::max);
    pos2(x, y)
}

/// Roughly how large a card is drawn; used to lay cards out and to frame the view.
pub fn size(card: &Card) -> Vec2 {
    // A letter is about this wide. Beside what is written by a pin there is room for its rate.
    const LETTER: f32 = 7.5;
    let widest =
        |pins: &[Pin]| pins.iter().map(|pin| LETTER * card.written(pin).len() as f32 + ASIDE).fold(0.0, f32::max);
    // The header holds the name, the state and a button.
    let header = LETTER * (card.name().len() + 16) as f32;
    let rows = card.left.len().max(card.right.len());
    vec2((widest(&card.left) + widest(&card.right) + 40.0).max(header), 46.0 + (ROW + 4.0) * rows as f32)
}

/// One reading of a counter of samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// How many samples so far.
    pub count: u64,
    /// When the newest of them was sent, in nanoseconds since 1970. Zero if the runtime does not say.
    pub sent_ns: u64,
    /// When the runtime read the counter, in nanoseconds since 1970 by its own clock.
    pub taken_ns: u64,
}

/// The seconds that the samples counted between two readings were sent over:
/// from the sending of the newest sample at the first reading to that of the
/// newest at the second. Timed instead by the readings themselves, a stream
/// of sixty a second would have 29, 30 or 31 samples in half a second and
/// seem to waver between 58 and 62. A runtime too old to say when samples
/// were sent is timed by its readings, and so is a stretch with nothing new.
pub fn seconds_between(before: &Reading, now: &Reading) -> f64 {
    let sent = before.sent_ns > 0 && now.sent_ns > before.sent_ns;
    let ns = if sent { now.sent_ns - before.sent_ns } else { now.taken_ns.saturating_sub(before.taken_ns) };
    ns as f64 / 1e9
}

/// The counters a runtime reports: one for each output, by `runtime/node/port`,
/// and one for each output an input hears, by the input and then the output.
fn readings(inventory: &Inventory) -> BTreeMap<(String, &str), Reading> {
    let taken_ns = inventory.taken_ns;
    let mut readings = BTreeMap::new();
    for node in &inventory.nodes {
        for input in &node.inputs {
            let port = format!("{}/{}/{}", inventory.runtime, node.name, input.port.name);
            // The input itself counts nothing, but it is there: with no output to hear, its rate is zero.
            readings.insert((port.clone(), ""), Reading { count: 0, sent_ns: 0, taken_ns });
            for source in &input.sources {
                let reading = Reading { count: source.received, sent_ns: source.sent_ns, taken_ns };
                readings.insert((port.clone(), source.output.as_str()), reading);
            }
        }
        for output in &node.outputs {
            let reading = Reading { count: output.sent, sent_ns: output.sent_ns, taken_ns };
            readings.insert((output.service.clone(), ""), reading);
        }
    }
    readings
}

/// Samples per second through every port, from two readings of the machine:
/// zero for a port nothing passes through. An input's rate is the sum over
/// the outputs it hears. A port that is new has no rate yet, and neither has
/// anything in a program that started again in between.
pub fn rates(before: &[Inventory], after: &[Inventory]) -> BTreeMap<String, f64> {
    let mut rates = BTreeMap::new();
    for now in after {
        let earlier = before.iter().find(|earlier| earlier.runtime == now.runtime && earlier.pid == now.pid);
        let Some(earlier) = earlier.filter(|earlier| earlier.taken_ns < now.taken_ns) else { continue };
        let was = readings(earlier);
        for ((port, source), reading) in readings(now) {
            let before = was.get(&(port.clone(), source));
            if let Some((before, passed)) =
                before.and_then(|before| Some((before, reading.count.checked_sub(before.count)?)))
            {
                *rates.entry(port).or_insert(0.0) += passed as f64 / seconds_between(before, &reading);
            }
        }
    }
    rates
}

/// Whether an output may be wired to an input: iceoryx2 will insist on the same message type.
pub fn compatible(output: &PortSpec, input: &PortSpec) -> bool {
    output.direction == Direction::Output && input.direction == Direction::Input && output.message == input.message
}
