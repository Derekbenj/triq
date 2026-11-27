//! The list beside the charts: everything that has numbers, each to be dragged onto a chart.
//!
//! It is listed node by node. What a node hears and what it sends is folded
//! away under its name until that is clicked, and a box searches the list by
//! name: what is found is open.
//!
//! A click on a number chooses it, and with the command key held it is one
//! more of those that are chosen. With Shift held, every number of the list
//! from the one that was chosen last to this one is. Whichever of them is
//! dragged onto a chart takes the others along.
use super::lines::written;
use super::{Carried, Recorded, Trace, carried};
use crate::colors::{Colors, dot};
use crate::history::Stat;
use crate::signals::Signals;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Ui};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// How large the dot before the name of an entry is.
const DOT: f32 = 5.0;
/// What the box at the top of the list says when the numbers of every runtime are listed.
const EVERY: &str = "every runtime";

/// The runtime that an entry of the list belongs to: the first part of its name. A connection
/// is its input's, and a topic that is no runtime's is listed by what its name begins with.
pub fn runtime_of(name: &str) -> &str {
    name.split('/').next().unwrap_or(name)
}

/// The node that an entry of the list belongs to, `runtime/node`: its name without its port.
pub fn node_of(name: &str) -> &str {
    name.rsplit_once('/').map_or(name, |(node, _)| node)
}

/// Whether an entry is among what is searched for: each word that was typed is somewhere in
/// its name or in what is said under it, in capitals or not. With nothing typed, every entry is.
pub fn found(search: &str, name: &str, about: &str) -> bool {
    let (name, about) = (name.to_lowercase(), about.to_lowercase());
    let has = |word: &str| name.contains(word) || about.contains(word);
    search.to_lowercase().split_whitespace().all(has)
}

/// Which of two entries of the list comes first, by their names. The entries of a runtime
/// stand together, the runtimes in order of their names. Within a runtime they are in order
/// of node and port, so what a node hears and what it sends stand together.
pub fn by_runtime(one: &str, other: &str) -> Ordering {
    /// A name as its runtime and the rest.
    fn parts(name: &str) -> (&str, &str) {
        name.split_once('/').unwrap_or((name, ""))
    }
    parts(one).cmp(&parts(other))
}

/// An entry of the list: what it is called, what is said under that, its numbers, and whether
/// it is a topic.
type Entry = (String, String, Vec<Trace>, bool);

/// Everything that has numbers, runtime by runtime and node by node: see [`by_runtime`]. Each
/// node has a dot in the colour of its runtime, and its entries are folded away under its name
/// until that is clicked. The box at the top of the list names the runtimes: the numbers of
/// the one that is chosen there are the only ones listed, which `only` holds. Under it, what
/// is typed in `search` is looked for in the names: only what is found is listed, and open.
/// Each number can be dragged onto a chart, and `chosen` are those that go together. A topic
/// can be opened, to see the numbers inside its messages; the topics that are open are returned.
pub(super) fn numbers(
    ui: &mut Ui,
    (recorded, colors): (Recorded<'_>, &Colors),
    chosen: &mut Vec<Trace>,
    only: &mut Option<String>,
    search: &mut String,
) -> BTreeSet<String> {
    let Recorded { history, signals } = recorded;
    // What each is called, what is said under that, its numbers, and whether it is a topic.
    let heard = history.connections().map(|connection| {
        let numbers = Stat::ALL.map(|stat| Trace::Heard { connection: connection.clone(), stat });
        (connection.to.clone(), format!("from {}", connection.from), numbers.to_vec(), false)
    });
    let outputs: BTreeSet<&String> = history.outputs().collect();
    let sent = outputs
        .iter()
        .map(|output| ((*output).clone(), "output".to_owned(), vec![Trace::Sent { output: (*output).clone() }], true));
    // A topic that no runtime reports has nobody to count for it: another program's,
    // or an output whose program stopped while something still listens to it.
    let topics = signals.topics();
    let others = topics.iter().filter(|topic| !outputs.contains(&topic.name)).map(|topic| {
        let carried = format!("{}{}", topic.message, if topic.slice { "[]" } else { "" });
        (topic.name.clone(), format!("a topic no runtime answers for: {carried}"), Vec::new(), true)
    });
    let mut listed: Vec<Entry> = heard.chain(sent).chain(others).collect();
    listed.sort_by(|one, other| by_runtime(&one.0, &other.0));
    // The runtimes that have anything in the list. One that was chosen and has gone is chosen no longer.
    let runtimes: BTreeSet<String> = listed.iter().map(|entry| runtime_of(&entry.0).to_owned()).collect();
    if only.as_ref().is_some_and(|runtime| !runtimes.contains(runtime)) {
        *only = None;
    }
    let dark = ui.visuals().dark_mode;

    let (mut opened, mut rows) = (BTreeSet::new(), Rows { chosen, listed: Vec::new(), run_to: None });
    // The two boxes stay where they are, over the list as it is scrolled.
    ui.horizontal(|ui| {
        let label = ui.weak("Numbers of");
        if let Some(runtime) = only.as_deref() {
            dot(ui, colors.of(runtime, dark), runtime);
        }
        let shown = only.clone().unwrap_or_else(|| EVERY.to_owned());
        let which = egui::ComboBox::from_id_salt("runtime").selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(only, None, EVERY);
            for runtime in &runtimes {
                ui.horizontal(|ui| {
                    dot(ui, colors.of(runtime, dark), runtime);
                    ui.selectable_value(only, Some(runtime.clone()), runtime);
                });
            }
        });
        which.response.labelled_by(label.id).on_hover_text("List the numbers of one runtime, or of every one");
    });
    ui.horizontal(|ui| {
        let label = ui.weak("Search");
        let about = "List what has every word typed here in its name: a runtime, a node, a port, or part of one";
        let words = egui::TextEdit::singleline(search).hint_text("by name").desired_width(f32::INFINITY);
        ui.add(words).labelled_by(label.id).on_hover_text(about);
    });
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.weak("Click the name of a node to list what it hears and sends, and again to fold that away.");
        ui.weak("Drag a number onto a chart to see it over time.");
        ui.weak("Hold Ctrl, or Cmd on a Mac, and click numbers to drag several at once. With Shift, a click takes all from the last one chosen to this one.");
        ui.separator();
        listed.retain(|entry| only.as_deref().is_none_or(|runtime| runtime_of(&entry.0) == runtime));
        if listed.is_empty() {
            ui.weak("Nothing is running.");
        }
        let searching = !search.trim().is_empty();
        listed.retain(|(name, about, ..)| found(search, name, about));
        if listed.is_empty() && searching {
            ui.weak("Nothing is called that.");
        }
        // Node by node, in the order of the list: what a node hears and sends is under its name.
        let mut nodes: Vec<(String, Vec<Entry>)> = Vec::new();
        for entry in listed {
            match nodes.iter_mut().find(|(node, _)| node == node_of(&entry.0)) {
                Some((_, entries)) => entries.push(entry),
                None => nodes.push((node_of(&entry.0).to_owned(), vec![entry])),
            }
        }
        for (node, entries) in nodes {
            // Folded away until it is clicked; what was searched for is open until it is. The two
            // are kept apart, so that a search leaves the list folded as it was.
            let color = colors.of(runtime_of(&node), dark);
            let folded = egui::CollapsingHeader::new(rows.named(ui, &node))
                .id_salt(("node", &node, searching))
                .default_open(searching);
            let shown = folded.show(ui, |ui| {
                for (name, about, numbers, topic) in entries {
                    ui.vertical(|ui| {
                        rows.entry(ui, &name, color);
                        ui.weak(about);
                        for trace in numbers {
                            let value = trace.latest(recorded).map_or(String::new(), |value| written(value) + trace.unit());
                            rows.number(ui, trace, value);
                        }
                        if topic {
                            egui::CollapsingHeader::new("numbers inside").id_salt(("inside", &name)).show(ui, |ui| {
                                rows.inside(ui, signals, &name);
                                opened.insert(name.clone());
                            });
                        }
                    });
                    ui.separator();
                }
            });
            // The dot is drawn in the room that the name leaves before itself, after the arrow.
            let header = shown.header_response.rect;
            let middle = egui::pos2(header.left() + ui.spacing().indent + DOT, header.center().y);
            ui.painter().circle_filled(middle, DOT, color);
        }
    });
    rows.settle();
    opened
}

/// The rows of the list as they are drawn.
struct Rows<'a> {
    /// The numbers that are chosen, in the order they were chosen in.
    chosen: &'a mut Vec<Trace>,
    /// Every number that has a row, from the top down.
    listed: Vec<Trace>,
    /// A number that was clicked with Shift held: the last of a run of numbers to choose.
    run_to: Option<Trace>,
}

impl Rows<'_> {
    /// A name with room before it for a dot in the colour of its runtime.
    fn named(&self, ui: &Ui, name: &str) -> LayoutJob {
        let mut words = LayoutJob::default();
        let ink = TextFormat {
            font_id: egui::TextStyle::Body.resolve(ui.style()),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        };
        words.append(name, DOT * 2.0 + 6.0, ink);
        words
    }

    /// The name that heads an entry, after a dot in the colour of its runtime. The two are one
    /// piece: the dot is drawn in the room that the name leaves before itself.
    fn entry(&self, ui: &mut Ui, name: &str, color: Color32) {
        let words = self.named(ui, name);
        let named = ui.label(words).on_hover_text(format!("runs in {}", runtime_of(name)));
        ui.painter().circle_filled(egui::pos2(named.rect.left() + DOT, named.rect.center().y), DOT, color);
    }

    /// Once every row is drawn: choose the run of numbers that a click with Shift asked for,
    /// and let go of what the list no longer shows, so that nothing is dragged along unseen.
    fn settle(&mut self) {
        let row = |number: &Trace| self.listed.iter().position(|listed| listed == number);
        if let Some(last) = self.run_to.take().and_then(|number| row(&number)) {
            // From the number that was chosen last, down the list or up it, to the one that was clicked.
            let first = self.chosen.last().and_then(row).unwrap_or(last);
            let run: Vec<usize> = if first <= last { (first..=last).collect() } else { (last..=first).rev().collect() };
            for number in run.into_iter().map(|row| self.listed[row].clone()) {
                if !self.chosen.contains(&number) {
                    self.chosen.push(number);
                }
            }
        }
        self.chosen.retain(|number| self.listed.contains(number));
    }

    /// The numbers inside the messages of a topic that is open in the list.
    fn inside(&mut self, ui: &mut Ui, signals: &Signals, topic: &str) {
        let Some(watched) = signals.watched(topic) else {
            ui.weak("opening");
            return;
        };
        if let Some(problem) = &watched.problem {
            ui.weak(problem);
        } else if watched.numbers.is_empty() {
            ui.weak("Nobody says what is inside.");
        }
        for (name, value) in watched.numbers {
            let number = Trace::Inside { topic: topic.to_owned(), number: name };
            self.number(ui, number, value.map_or(String::new(), written));
        }
    }

    /// One number in the list, with its value as it stands: the thing to drag, and to choose by a click.
    fn number(&mut self, ui: &mut Ui, trace: Trace, value: String) {
        let among = self.chosen.contains(&trace);
        // Whichever of the chosen is dragged takes the others along. Any other number goes alone.
        let taken = if among { self.chosen.clone() } else { vec![trace.clone()] };
        let behind = ui.painter().add(egui::Shape::Noop);
        let row = carried(ui, egui::Id::unique(("number", &trace)), Carried::Numbers(taken), |ui| {
            let row = ui.horizontal(|ui| {
                // The values line up in a column of their own, unless the name is too long for that.
                let word = ui.label(trace.word());
                ui.add_space((64.0 - word.rect.width()).max(8.0));
                ui.weak(value);
            });
            row.response
        });
        if among {
            let marked = egui::Shape::rect_filled(row.rect.expand(1.0), 2.0, ui.visuals().selection.bg_fill);
            ui.painter().set(behind, marked);
        }
        if row.clicked() {
            let held = ui.input(|input| input.modifiers);
            match (held.shift, held.command, among) {
                (true, ..) => self.run_to = Some(trace.clone()),
                (_, true, true) => self.chosen.retain(|chosen| *chosen != trace),
                (_, true, false) => self.chosen.push(trace.clone()),
                // With no key held it is the only one that is chosen, or none is, if it was that already.
                _ => *self.chosen = if *self.chosen == [trace.clone()] { Vec::new() } else { vec![trace.clone()] },
            }
        }
        self.listed.push(trace);
    }
}
