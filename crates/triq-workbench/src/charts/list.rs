//! The list beside the charts: everything that has numbers, each to be dragged onto a chart.
//!
//! A click on a number chooses it, and with the command key held it is one
//! more of those that are chosen. Whichever of them is dragged onto a chart
//! takes the others along.
use super::lines::written;
use super::{Carried, Recorded, Trace, carried};
use crate::history::Stat;
use crate::signals::Signals;
use eframe::egui::{self, Ui};
use std::collections::BTreeSet;

/// Everything that has numbers, in order of name: what a node hears and what
/// it sends stand together. Each number can be dragged onto a chart, and
/// `chosen` are those that go together. A topic can be opened, to see the
/// numbers inside its messages; the topics that are open are returned.
pub(super) fn numbers(ui: &mut Ui, recorded: Recorded<'_>, chosen: &mut Vec<Trace>) -> BTreeSet<String> {
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
    let mut listed: Vec<_> = heard.chain(sent).chain(others).collect();
    listed.sort_by(|a, b| a.0.cmp(&b.0));

    let (mut opened, mut rows) = (BTreeSet::new(), Rows { chosen, seen: Vec::new() });
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.weak("Drag a number onto a chart to see it over time.");
        ui.weak("Hold Ctrl, or Cmd on a Mac, and click numbers to drag several at once.");
        ui.separator();
        if listed.is_empty() {
            ui.weak("Nothing is running.");
        }
        for (name, about, numbers, topic) in listed {
            ui.vertical(|ui| {
                ui.strong(&name);
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
    // What the list no longer shows is no longer chosen: nothing is dragged along unseen.
    rows.chosen.retain(|number| rows.seen.contains(number));
    opened
}

/// The rows of the list as they are drawn: the numbers that are chosen, in the
/// order they were chosen in, and which of them were among the rows.
struct Rows<'a> {
    chosen: &'a mut Vec<Trace>,
    seen: Vec<Trace>,
}

impl Rows<'_> {
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
            match (ui.input(|input| input.modifiers.command), among) {
                (true, true) => self.chosen.retain(|chosen| *chosen != trace),
                (true, false) => self.chosen.push(trace.clone()),
                // Without the key it is the only one that is chosen, or none is, if it was that already.
                (false, _) => {
                    *self.chosen = if *self.chosen == [trace.clone()] { Vec::new() } else { vec![trace.clone()] }
                }
            }
        }
        if self.chosen.contains(&trace) {
            self.seen.push(trace);
        }
    }
}
