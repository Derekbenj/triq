//! From what the runtimes report to what is drawn: one card per node, one wire
//! per live connection. Nothing here talks to a runtime or to the screen.
use eframe::egui::{Color32, Pos2, Vec2, ecolor::Hsva, pos2, vec2};
use egui_snarl::{InPinId, OutPinId, Snarl};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use triq::{Direction, Inventory, MessageType, NodeInfo, PortSpec};

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
const COLUMN: f32 = 320.0;
const GAP: f32 = 36.0;

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

/// Where a card goes among those already placed: one column right of the
/// furthest card feeding it, below whatever is in that column.
fn place(placed: &[(Pos2, &Card)], card: &Card) -> Pos2 {
    let feeders = placed.iter().filter(|(_, other)| other.feeds(card)).map(|(place, _)| place.x + COLUMN);
    let x = feeders.fold(LEFT, f32::max);
    let column = placed.iter().filter(|(place, _)| (place.x - x).abs() < COLUMN / 2.0);
    let y = column.map(|(place, other)| place.y + size(other).y + GAP).fold(TOP, f32::max);
    pos2(x, y)
}

/// Roughly how large a card is drawn; used to lay cards out and to frame the view.
pub fn size(card: &Card) -> Vec2 {
    vec2(250.0, 56.0 + 30.0 * card.node.inputs.len().max(card.node.outputs.len()) as f32)
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

/// Samples per second through every port, from two readings of the counters
/// taken `elapsed` apart. A port that is new has no rate yet, and neither has
/// one whose counter went back because its program started again.
pub fn rates(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
    elapsed: Duration,
) -> BTreeMap<String, f64> {
    let rate = |(port, now): (&String, &u64)| {
        let passed = now.checked_sub(*before.get(port)?)?;
        Some((port.clone(), passed as f64 / elapsed.as_secs_f64()))
    };
    after.iter().filter_map(rate).collect()
}

/// Whether an output may be wired to an input: iceoryx2 will insist on the same message type.
pub fn compatible(output: &PortSpec, input: &PortSpec) -> bool {
    output.direction == Direction::Output && input.direction == Direction::Input && output.message == input.message
}

/// One colour per message type, the same on every machine and in every session.
pub fn color(message: &MessageType) -> Color32 {
    let text = format!("{message}{}", message.size);
    let hash =
        text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    Hsva::new((hash % 360) as f32 / 360.0, 0.62, 0.86, 1.0).into()
}
