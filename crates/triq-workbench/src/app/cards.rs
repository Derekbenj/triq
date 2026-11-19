//! How the graph is drawn and handled: what is written on a card, the colours
//! of cards, pins and wires, and what a click, a drag or a menu there asks for.
//!
//! A node wears the colour of the runtime it runs in: its card, its pins and
//! the wires that leave it. The card of a group whose nodes run in several
//! runtimes is no one's colour, and each of its pins is its node's. When a
//! card is clicked, every card that no wire joins to it goes grey. What is
//! only drafted is a brighter grey, with a glow around it.
use super::details::{Form, actions, rate, toggle};
use super::{Asking, Focus, Rates, Wish, awaits};
use crate::colors::Colors;
use crate::draft::{Mark, Marks};
use crate::feed::Request;
use crate::graph::{self, Card, Kind, Port};
use eframe::egui::{self, Color32, Rect, Role, Sense, Shape, Stroke, Ui, WidgetInfo, emath::TSTransform};
use egui_snarl::ui::{PinInfo, PinWireInfo, SnarlPin, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use std::collections::BTreeSet;
use triq::{Direction, Inventory, NodeState};

/// How the graph widget asks what to draw and reports what the user did.
pub(super) struct Viewer<'a> {
    pub asking: Asking<'a>,
    /// What the draft does to each card and wire.
    pub marks: &'a Marks,
    pub colors: &'a Colors,
    /// Whether the window is drawn light on dark.
    pub dark: bool,
    pub focus: &'a mut Focus,
    /// The cards that keep their colour while one is selected: that one, and
    /// all that wires join to it. `None` while none is: then every card does.
    pub lit: Option<BTreeSet<String>>,
    pub inventories: &'a [Inventory],
    pub rates: &'a Rates,
    /// A view to switch to this frame.
    pub frame: Option<TSTransform>,
    /// How far the graph has moved in the window since the last frame. The view moves along.
    pub carried: egui::Vec2,
    /// The cards that Shift has selected, by what they are called.
    pub selected: &'a [String],
    /// Whether the graph shows the inside of a group.
    pub inside: bool,
    /// Whether the click of this frame, if there is one, is the second of a double click.
    pub double: bool,
    /// What was asked for that changes what the graph shows.
    pub wish: &'a mut Option<Wish>,
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

    /// The colour of a card: that of its runtime, or of none if it stands for
    /// nodes of several. A card that is only drafted is no runtime's yet, and
    /// a card that is greyed is grey.
    fn color(&self, card: &Card) -> Color32 {
        match self.mark(card) {
            Some(Mark::New) => drafted(self.dark),
            _ if self.dimmed(card) => grey(self.dark),
            _ => card.runtime().map_or(nobodys(self.dark), |runtime| self.colors.of(runtime, self.dark)),
        }
    }

    /// A pin, and any wire that leaves it, in the colour of the runtime of
    /// its node: grey as what is drafted is, if the node is yet to be
    /// created. A wire that the draft draws or cuts has a glow around the
    /// pins at its ends.
    fn pin(&self, card: &Card, pin: &graph::Pin) -> Pin {
        let unmade = self.marks.card(&pin.owner()) == Some(Mark::New);
        let color = match () {
            () if unmade => drafted(self.dark),
            () if self.dimmed(card) => grey(self.dark),
            () => self.colors.of(&pin.runtime, self.dark),
        };
        let glows = self.marks.port(&pin.name()).then(|| glow(self.dark));
        Pin { pin: PinInfo::circle().with_fill(color).with_wire_color(color), glows }
    }

    /// The row of a pin: what its port is called, and how fast data passes
    /// through it. The counters are a hover away and in the details.
    fn row(&self, ui: &mut Ui, card: &Card, pin: &graph::Pin) -> Pin {
        let written = ui.label(card.written(pin));
        match &pin.port {
            Port::Input(input) => {
                let received: u64 = input.sources.iter().map(|source| source.received).sum();
                let lost: u64 = input.sources.iter().map(|source| source.lost).sum();
                written.on_hover_text(format!("{received} received, {lost} lost"));
                let waiting: Vec<&str> = input
                    .sources
                    .iter()
                    .filter(|source| !source.connected)
                    .map(|source| source.output.as_str())
                    .collect();
                if waiting.is_empty() {
                    aside(ui, rate(self.rates, &pin.name()));
                } else {
                    aside(ui, "waiting".to_owned()).on_hover_text(format!("waiting for {}", waiting.join(", ")));
                }
            }
            Port::Output(output) => {
                written.on_hover_text(format!("{} sent to {} inputs", output.sent, output.subscribers));
                aside(ui, rate(self.rates, &output.service));
            }
        }
        self.pin(card, pin)
    }

    /// The name at the top of a card. On a card that is greyed it does not stand out.
    fn name(&self, ui: &mut Ui, card: &Card) {
        if self.dimmed(card) {
            ui.label(card.name())
        } else {
            ui.strong(card.name())
        };
    }

    /// The nodes a menu on a card is about: every node that is selected, if
    /// this card is one of them, and else this one alone.
    fn chosen(&self, key: String) -> Vec<String> {
        let nodes = self.selected.iter().filter(|key| !key.starts_with('/'));
        if self.selected.contains(&key) { nodes.cloned().collect() } else { vec![key] }
    }
}

/// A pin as it is drawn, with a glow around it or without.
pub(super) struct Pin {
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

/// The grey of a card that no wire joins to the one that is selected.
fn grey(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 105 } else { 150 })
}

/// The grey of what is only drafted: brighter than the grey of a card that
/// is greyed, and with a glow around it.
fn drafted(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 215 } else { 60 })
}

/// The colour of a card that is no one runtime's: a group of nodes from
/// several, and the cards that stand for the outside of a group.
fn nobodys(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 165 } else { 95 })
}

/// The glow around a card that the draft creates, changes or removes, and
/// around the pins of a wire that it draws or cuts.
fn glow(dark: bool) -> egui::Shadow {
    egui::Shadow { offset: [0, 0], blur: 12, spread: 4, color: drafted(dark).gamma_multiply(0.7) }
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

/// Ask for the connection that a wire from one pin to another would be.
pub(super) fn wire(asking: &mut Asking<'_>, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node].right[from.output], &snarl[to.node].left[to.input]);
    let (output, input) = (source.spec(), sink.spec());
    // Inside a group, the pins on the cards that stand for the outside are ports of nodes in it.
    if output.direction != Direction::Output || input.direction != Direction::Input {
        asking.feed.report("A wire runs from an output of one node to an input of another".to_owned());
    } else if !graph::compatible(output, input) {
        asking.feed.report(format!(
            "{} and {} carry different messages: {} and {}",
            source.name(),
            sink.name(),
            output.message,
            input.message
        ));
    } else {
        asking.ask(Request::Connect { from: source.name(), to: sink.name() });
    }
}

/// Ask for the connection that a wire stands for to be removed.
pub(super) fn unwire(asking: &mut Asking<'_>, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node].right[from.output], &snarl[to.node].left[to.input]);
    // Inside a group, a wire to a card that stands for the outside is no connection: its two ends are one port.
    if source.name() != sink.name() {
        asking.ask(Request::Disconnect { from: source.name(), to: sink.name() });
    }
}

impl SnarlViewer<Card> for Viewer<'_> {
    fn title(&mut self, card: &Card) -> String {
        card.name().to_owned()
    }

    fn current_transform(&mut self, to_global: &mut TSTransform, _: &mut Snarl<Card>) {
        to_global.translation += self.carried;
        if let Some(frame) = self.frame.take() {
            *to_global = frame;
        }
    }

    fn show_header(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<Card>) {
        let card = &snarl[node];
        let mark = self.mark(card);
        ui.horizontal(|ui| {
            match &card.kind {
                Kind::Node { runtime, node } => {
                    dot(ui, self.color(card), runtime);
                    self.name(ui, card);
                    // A node that does not exist yet neither runs nor can be paused.
                    if mark != Some(Mark::New) {
                        match &node.state {
                            NodeState::Running => ui.weak("running"),
                            NodeState::Paused => ui.colored_label(ui.visuals().warn_fg_color, "paused"),
                            NodeState::Stopped(_) => ui.colored_label(ui.visuals().error_fg_color, "stopped"),
                        };
                        if let Some((word, request)) = toggle(runtime, node)
                            && sign(ui, word, node.state == NodeState::Paused).clicked()
                        {
                            self.asking.ask(request);
                        }
                    }
                }
                Kind::Group { name, members } => {
                    self.name(ui, card);
                    ui.weak(if members.len() == 1 { "1 node".to_owned() } else { format!("{} nodes", members.len()) });
                    // A node in it that does not run shows on the outside.
                    let count =
                        |held: fn(&NodeState) -> bool| members.iter().filter(|(_, node)| held(&node.state)).count();
                    let paused = count(|state| *state == NodeState::Paused);
                    let stopped = count(|state| matches!(state, NodeState::Stopped(_)));
                    if paused > 0 {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("{paused} paused"));
                    }
                    if stopped > 0 {
                        ui.colored_label(ui.visuals().error_fg_color, format!("{stopped} stopped"));
                    }
                    let about = "Edit Group: show the nodes inside it. Tab does the same, and so does a double click";
                    if ui.small_button("Edit").on_hover_text(about).clicked() {
                        *self.wish = Some(Wish::Enter(name.clone()));
                    }
                }
                Kind::Entry { .. } | Kind::Exit { .. } => self.name(ui, card),
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
        let selected = match (&*self.focus, &card.kind) {
            (Focus::Node { key, .. }, Kind::Node { .. }) => *key == card.key(),
            (Focus::Group { name, .. }, Kind::Group { name: group, .. }) => name == group,
            _ => false,
        };
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
        match self.marks.wire(&source.output(from.output), &sink.input(to.input)) {
            Some(Mark::New) => drafted(self.dark),
            Some(_) => sender.gamma_multiply(0.3),
            None if self.dimmed(source) => grey(self.dark),
            None => sender,
        }
    }

    fn inputs(&mut self, card: &Card) -> usize {
        card.left.len()
    }

    fn outputs(&mut self, card: &Card) -> usize {
        card.right.len()
    }

    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        self.row(ui, card, &card.left[pin.id.input])
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<Card>) -> impl SnarlPin + 'static {
        let card = &snarl[pin.id.node];
        self.row(ui, card, &card.right[pin.id.output])
    }

    /// A wire being dragged only snaps to an input it could be connected to.
    fn can_snap(&mut self, from: &OutPin, to: &InPin, snarl: &Snarl<Card>) -> bool {
        let output = snarl[from.id.node].right[from.id.output].spec();
        let input = snarl[to.id.node].left[to.id.input].spec();
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

    /// A click on a card selects what it stands for. A double click on a group shows its inside.
    fn node_clicked(&mut self, node: NodeId, snarl: &Snarl<Card>) {
        let card = &snarl[node];
        match &card.kind {
            Kind::Node { node, .. } => *self.focus = Focus::node(card.key(), &node.params),
            Kind::Group { name, .. } => {
                if self.double {
                    *self.wish = Some(Wish::Enter(name.clone()));
                }
                self.focus.group(name);
            }
            Kind::Entry { group } | Kind::Exit { group } => self.focus.group(group),
        }
    }

    /// A click beside the cards lets go of what was selected.
    fn background_clicked(&mut self, _: &Snarl<Card>) {
        if matches!(self.focus, Focus::Node { .. } | Focus::Group { .. }) {
            *self.focus = Focus::Nothing;
        }
    }

    fn has_node_menu(&mut self, card: &Card) -> bool {
        matches!(card.kind, Kind::Node { .. } | Kind::Group { .. })
    }

    fn show_node_menu(&mut self, node: NodeId, _: &[InPin], _: &[OutPin], ui: &mut Ui, snarl: &mut Snarl<Card>) {
        let card = &snarl[node];
        let wish = match &card.kind {
            Kind::Node { runtime, node } => {
                if actions(ui, &mut self.asking, runtime, node) {
                    ui.close();
                }
                ui.separator();
                if self.inside {
                    let out = ui
                        .button("Move out of the group")
                        .on_hover_text("P does the same for the cards that are selected");
                    out.clicked().then(|| Wish::Separate(self.chosen(card.key())))
                } else {
                    let make =
                        ui.button("Make Group").on_hover_text("Ctrl+G does the same for the cards that are selected");
                    make.clicked().then(|| Wish::Group(self.chosen(card.key())))
                }
            }
            Kind::Group { name, .. } => {
                let edit =
                    ui.button("Edit Group").on_hover_text("Show the nodes inside it. Tab does the same").clicked();
                let apart = ui.button("Ungroup").on_hover_text("Ctrl+Alt+G does the same").clicked();
                if edit { Some(Wish::Enter(name.clone())) } else { apart.then(|| Wish::Ungroup(name.clone())) }
            }
            Kind::Entry { .. } | Kind::Exit { .. } => None,
        };
        if wish.is_some() {
            *self.wish = wish;
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
