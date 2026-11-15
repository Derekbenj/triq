//! From what the runtimes report to what is drawn: one card per node, one wire
//! per live connection. Nothing here talks to a runtime or to the screen.
use eframe::egui::{Pos2, Vec2, pos2, vec2};
use egui_snarl::{InPinId, OutPinId, Snarl};
use std::collections::{BTreeMap, HashMap, HashSet};
use triq::{Direction, Inventory, NodeInfo, PortSpec};

/// One node of one runtime, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub runtime: String,
    pub node: NodeInfo,
}

impl Card {
    /// `runtime/node`: what identifies a card across syncs.
    pub fn key(&self) -> String {
        format!("{}/{}", self.runtime, self.node.name)
    }

    /// `runtime/node/port` of an output.
    pub fn output(&self, index: usize) -> &str {
        &self.node.outputs[index].service
    }

    /// `runtime/node/port` of an input.
    pub fn input(&self, index: usize) -> String {
        format!("{}/{}", self.key(), self.node.inputs[index].port.name)
    }

    /// Outputs an input wants but cannot hear: they do not exist yet, or their program stopped.
    pub fn waiting(&self, index: usize) -> Vec<String> {
        let sources = self.node.inputs[index].sources.iter();
        sources.filter(|source| !source.connected).map(|source| source.output.clone()).collect()
    }

    /// Whether any output of this card is wanted by an input of `other`.
    fn feeds(&self, other: &Card) -> bool {
        let wanted = other.node.inputs.iter().flat_map(|input| &input.sources);
        wanted.into_iter().any(|source| self.node.outputs.iter().any(|output| output.service == source.output))
    }

    fn ports(&self) -> impl Iterator<Item = &PortSpec> {
        self.node.inputs.iter().map(|input| &input.port).chain(self.node.outputs.iter().map(|output| &output.port))
    }
}

/// Make the drawing match the machine. Cards keep the places the user gave them.
pub fn sync(snarl: &mut Snarl<Card>, inventories: &[Inventory]) {
    let mut reported: BTreeMap<String, Card> = inventories
        .iter()
        .flat_map(|inventory| {
            inventory.nodes.iter().map(|node| Card { runtime: inventory.runtime.clone(), node: node.clone() })
        })
        .map(|card| (card.key(), card))
        .collect();

    // A card goes when its node is gone, or when a different node took its name.
    let stale: Vec<_> = snarl
        .nodes_ids_data()
        .filter(|(_, drawn)| reported.get(&drawn.value.key()).is_none_or(|card| !card.ports().eq(drawn.value.ports())))
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        snarl.remove_node(id);
    }
    for (_, drawn) in snarl.nodes_ids_data_mut() {
        if let Some(card) = reported.remove(&drawn.value.key()) {
            drawn.value = card;
        }
    }

    // What is left is new.
    for card in sources_first(reported.into_values().collect()) {
        let place = place(&snarl.nodes_pos().collect::<Vec<_>>(), &card);
        snarl.insert_node(place, card);
    }

    // One wire per connection whose output is on the drawing.
    let outputs: HashMap<String, OutPinId> = snarl
        .nodes_ids_data()
        .flat_map(|(node, drawn)| {
            let outputs = drawn.value.node.outputs.iter().enumerate();
            outputs.map(move |(output, info)| (info.service.clone(), OutPinId { node, output }))
        })
        .collect();
    let wanted: HashSet<(OutPinId, InPinId)> = snarl
        .nodes_ids_data()
        .flat_map(|(node, drawn)| {
            let inputs = drawn.value.node.inputs.iter().enumerate();
            inputs
                .flat_map(move |(input, info)| info.sources.iter().map(move |source| (source, InPinId { node, input })))
        })
        .filter_map(|(source, to)| Some((*outputs.get(&source.output)?, to)))
        .collect();
    let drawn: HashSet<(OutPinId, InPinId)> = snarl.wires().collect();
    for (from, to) in drawn.difference(&wanted) {
        snarl.disconnect(*from, *to);
    }
    for (from, to) in wanted.difference(&drawn) {
        snarl.connect(*from, *to);
    }
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
/// feeds it, below whatever is already there.
fn place(placed: &[(Pos2, &Card)], card: &Card) -> Pos2 {
    let feeders = placed.iter().filter(|(_, other)| other.feeds(card));
    let x = feeders.map(|(place, other)| place.x + size(other).x + GAP.x).fold(LEFT, f32::max);
    let beside = placed.iter().filter(|(place, other)| place.x < x + size(card).x && x < place.x + size(other).x);
    let y = beside.map(|(place, other)| place.y + size(other).y + GAP.y).fold(TOP, f32::max);
    pos2(x, y)
}

/// Roughly how large a card is drawn; used to lay cards out and to frame the view.
pub fn size(card: &Card) -> Vec2 {
    // A letter is about this wide. A port is written `name: Type`, with room for its rate beside it.
    const LETTER: f32 = 7.5;
    let written = |port: &PortSpec| LETTER * (port.name.len() + 2 + port.message.to_string().len()) as f32 + ASIDE;
    let widest = |ports: &mut dyn Iterator<Item = &PortSpec>| ports.map(written).fold(0.0, f32::max);
    let inputs = widest(&mut card.node.inputs.iter().map(|input| &input.port));
    let outputs = widest(&mut card.node.outputs.iter().map(|output| &output.port));
    // The header holds the name, the state and a button.
    let header = LETTER * (card.node.name.len() + 16) as f32;
    let rows = card.node.inputs.len().max(card.node.outputs.len());
    vec2((inputs + outputs + 40.0).max(header), 46.0 + (ROW + 4.0) * rows as f32)
}

/// How many samples each port has handled so far, by `runtime/node/port`:
/// what an output has sent, and what an input has received from all its sources.
pub fn counts(inventories: &[Inventory]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for inventory in inventories {
        for node in &inventory.nodes {
            for input in &node.inputs {
                let received = input.sources.iter().map(|source| source.received).sum();
                counts.insert(format!("{}/{}/{}", inventory.runtime, node.name, input.port.name), received);
            }
            for output in &node.outputs {
                counts.insert(output.service.clone(), output.sent);
            }
        }
    }
    counts
}

/// Samples per second through every port, from two readings of the machine.
/// Each runtime's counters are divided by the time that runtime says passed
/// between its own two readings. A port that is new has no rate yet, and
/// neither has anything in a program that started again in between.
pub fn rates(before: &[Inventory], after: &[Inventory]) -> BTreeMap<String, f64> {
    let mut rates = BTreeMap::new();
    for now in after {
        let earlier = before.iter().find(|earlier| earlier.runtime == now.runtime && earlier.pid == now.pid);
        let Some(earlier) = earlier.filter(|earlier| earlier.taken_ns < now.taken_ns) else { continue };
        let seconds = (now.taken_ns - earlier.taken_ns) as f64 / 1e9;
        let was = counts(std::slice::from_ref(earlier));
        for (port, count) in counts(std::slice::from_ref(now)) {
            if let Some(passed) = was.get(&port).and_then(|was| count.checked_sub(*was)) {
                rates.insert(port, passed as f64 / seconds);
            }
        }
    }
    rates
}

/// Whether an output may be wired to an input: iceoryx2 will insist on the same message type.
pub fn compatible(output: &PortSpec, input: &PortSpec) -> bool {
    output.direction == Direction::Output && input.direction == Direction::Input && output.message == input.message
}
