//! How the graph is drawn and handled: what is written on a card, the colours
//! of cards, pins and wires, and what a click, a drag or a menu there asks for.
//!
//! A node wears the colour of the runtime it runs in: its card, its pins and
//! the wires that leave it. The card of a group is outlined in white, or in
//! black on a light background. Its header fades through the colours of the
//! runtimes its nodes run in, with a dot for each, and each of its pins is
//! the colour of its node. When a card is clicked, every card that neither
//! feeds it nor is fed by it goes grey. What is only drafted is a light grey,
//! with a glow around it.
use super::details::{rate, toggle};
use super::{Asking, Focus, Rates, Wish, awaits};
use crate::colors::Colors;
use crate::draft::{Mark, Marks};
use crate::feed::Request;
use crate::graph::{self, Card, Kind, Port};
use eframe::egui::{self, Color32, Rect, Role, Sense, Shape, Stroke, Ui, WidgetInfo, emath::TSTransform};
use egui_snarl::ui::{PinInfo, PinWireInfo, SnarlPin, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use std::collections::BTreeSet;
use triq::NodeState;

/// How the graph widget asks what to draw and reports what the user did.
pub(super) struct Viewer<'a> {
    pub asking: Asking<'a>,
    /// What the draft does to each card and wire.
    pub marks: &'a Marks,
    pub colors: &'a Colors,
    /// Whether the window is drawn light on dark.
    pub dark: bool,
    pub focus: &'a mut Focus,
    /// The cards that keep their colour while one is selected: that one, all
    /// that feed it and all that it feeds. `None` while none is: then every card does.
    pub lit: Option<BTreeSet<String>>,
    pub rates: &'a Rates,
    /// A view to switch to this frame.
    pub frame: Option<TSTransform>,
    /// How far the graph has moved in the window since the last frame. The view moves along.
    pub carried: egui::Vec2,
    /// Set when a card is clicked: its details are to be seen.
    pub reveal: &'a mut bool,
    /// Whether the click of this frame, if there is one, is the second of a double click.
    pub double: bool,
    /// What was asked for that changes what the graph shows.
    pub wish: &'a mut Option<Wish>,
}

impl Viewer<'_> {
    /// Whether a card is greyed: another is selected, which this one neither feeds nor is fed by.
    fn dimmed(&self, card: &Card) -> bool {
        self.lit.as_ref().is_some_and(|lit| !lit.contains(&card.key()))
    }

    /// What the draft does to a card.
    fn mark(&self, card: &Card) -> Option<Mark> {
        self.marks.card(&card.key())
    }

    /// The colour of a card: that of its node's runtime. A group, and the
    /// cards that stand for the outside of one, are no one runtime's. A card
    /// that is only drafted is no runtime's yet, and a card that is greyed is grey.
    fn color(&self, card: &Card) -> Color32 {
        match self.mark(card) {
            Some(Mark::New) => drafted(self.dark),
            _ if self.dimmed(card) => grey(self.dark),
            _ => match &card.kind {
                Kind::Node { runtime, .. } => self.colors.of(runtime, self.dark),
                Kind::Group { .. } | Kind::Entry { .. } | Kind::Exit { .. } => plain(self.dark),
            },
        }
    }

    /// The runtimes whose colours the header of a card fades through: for
    /// a group, those its nodes run in. A group that is greyed, or is only
    /// drafted, has none of their colours.
    fn fades<'a>(&self, card: &'a Card) -> BTreeSet<&'a str> {
        match &card.kind {
            Kind::Group { members, .. } if self.mark(card) != Some(Mark::New) && !self.dimmed(card) => {
                members.iter().map(|(runtime, _)| runtime.as_str()).collect()
            }
            _ => BTreeSet::new(),
        }
    }

    /// How much of its colour the header of a card has. A card that is
    /// greyed has hardly any, and one that stands for the outside has little.
    fn strength(&self, card: &Card) -> f32 {
        match &card.kind {
            _ if self.dimmed(card) => 0.08,
            Kind::Node { .. } | Kind::Group { .. } if self.dark => 0.28,
            Kind::Node { .. } | Kind::Group { .. } => 0.20,
            Kind::Entry { .. } | Kind::Exit { .. } => 0.12,
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
        // On a card with the ports of several nodes, a pin says in full whose it is.
        let whose = match &card.kind {
            Kind::Node { .. } => String::new(),
            Kind::Group { .. } | Kind::Entry { .. } | Kind::Exit { .. } => format!("{}\n", pin.name()),
        };
        match &pin.port {
            Port::Input(input) => {
                let received: u64 = input.sources.iter().map(|source| source.received).sum();
                let lost: u64 = input.sources.iter().map(|source| source.lost).sum();
                written.on_hover_text(format!("{whose}{received} received, {lost} lost"));
                aside(ui, rate(self.rates, &pin.name()));
            }
            Port::Output(output) => {
                written.on_hover_text(format!("{whose}{} sent to {} inputs", output.sent, output.subscribers));
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

/// The grey of a card that neither feeds the one that is selected nor is fed by it.
fn grey(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 105 } else { 150 })
}

/// The grey of what is only drafted: brighter than the grey of a card that
/// is greyed, and with a glow around it.
fn drafted(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 215 } else { 60 })
}

/// The colour of a card that is no one runtime's: a group, and the cards
/// that stand for the outside of one. It is nearly white, or on a light
/// background nearly black: no grey, which is what a greyed card is.
fn plain(dark: bool) -> Color32 {
    Color32::from_gray(if dark { 240 } else { 25 })
}

/// The glow around a card that the draft creates, changes or removes, and
/// along a wire that it draws or cuts, with the pins at its ends.
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

/// Whether a wire would run from one of the two cards that stand for the
/// outside of a group to the other, past every node of the group.
fn passes_by(snarl: &Snarl<Card>, from: OutPinId, to: InPinId) -> bool {
    matches!((&snarl[from.node].kind, &snarl[to.node].kind), (Kind::Entry { .. }, Kind::Exit { .. }))
}

/// Ask for the connection that a wire from one pin to another would be.
pub(super) fn wire(asking: &mut Asking<'_>, snarl: &Snarl<Card>, from: OutPinId, to: InPinId) {
    let (source, sink) = (&snarl[from.node].right[from.output], &snarl[to.node].left[to.input]);
    let (output, input) = (source.spec(), sink.spec());
    if passes_by(snarl, from, to) {
        asking.feed.report("A wire inside a group begins or ends at one of its nodes".to_owned());
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
    asking.ask(Request::Disconnect { from: source.name(), to: sink.name() });
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
                Kind::Group { path, members } => {
                    // A dot for each runtime that its nodes run in.
                    let runtimes: BTreeSet<&str> = members.iter().map(|(runtime, _)| runtime.as_str()).collect();
                    for runtime in runtimes {
                        let color =
                            if self.dimmed(card) { grey(self.dark) } else { self.colors.of(runtime, self.dark) };
                        dot(ui, color, runtime);
                    }
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
                        *self.wish = Some(Wish::Enter(path.clone()));
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
            (Focus::Group { name, .. }, Kind::Group { path, .. }) => name == path,
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
        // Under the colours that the header of a group fades through is the mix of them.
        let tint = self.colors.blend(self.fades(card), self.dark).unwrap_or_else(|| self.color(card));
        default.fill(default.fill.lerp_to_gamma(tint, self.strength(card))).corner_radius(9)
    }

    /// The header of a group has the colours of the runtimes its nodes run
    /// in, from left to right, with as much room for each as for any other.
    ///
    /// The graph fades from each colour it is given to the next, over the
    /// room of one. Halfway between two colours is neither, and a wide
    /// stretch of that looks washed out. So each colour is given several
    /// times over: most of its room is the colour itself, and the fade to
    /// the next is short.
    fn header_gradient(&mut self, default: Color32, node: NodeId, snarl: &Snarl<Card>) -> Vec<Color32> {
        const TIMES: usize = 4;
        let card = &snarl[node];
        let tinted = |runtime| default.lerp_to_gamma(self.colors.of(runtime, self.dark), self.strength(card));
        self.fades(card).into_iter().flat_map(|runtime| [tinted(runtime); TIMES]).collect()
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
            // A wire keeps its colour between two cards that keep theirs.
            None if self.dimmed(source) || self.dimmed(sink) => grey(self.dark),
            None => sender,
        }
    }

    /// A wire that the draft draws or cuts has a glow along it.
    fn connected_wire_glow(&mut self, from: OutPinId, to: InPinId, snarl: &Snarl<Card>) -> Option<Color32> {
        let (source, sink) = (&snarl[from.node], &snarl[to.node]);
        let mark = self.marks.wire(&source.output(from.output), &sink.input(to.input));
        mark.map(|_| glow(self.dark).color)
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
        !passes_by(snarl, from.id, to.id) && graph::compatible(output, input)
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

    /// A click on a card selects what it stands for, and its details are
    /// shown. A double click on a group shows its inside.
    fn node_clicked(&mut self, node: NodeId, snarl: &Snarl<Card>) {
        let card = &snarl[node];
        *self.reveal = true;
        match &card.kind {
            Kind::Node { node, .. } => *self.focus = Focus::node(card.key(), &node.params),
            Kind::Group { path, .. } => {
                if self.double {
                    *self.wish = Some(Wish::Enter(path.clone()));
                }
                self.focus.group(path);
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
}
