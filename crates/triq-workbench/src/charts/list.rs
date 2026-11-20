//! The list beside the charts: everything that has numbers, each to be dragged onto a chart.
use super::lines::written;
use super::{Recorded, Trace};
use crate::history::Stat;
use crate::signals::Signals;
use eframe::egui::{self, Ui};
use std::collections::BTreeSet;

/// Everything that has numbers, in order of name: what a node hears and what
/// it sends stand together. Each number can be dragged onto a chart. A topic
/// can be opened, to see the numbers inside its messages; the topics that
/// are open are returned.
pub(super) fn numbers(ui: &mut Ui, recorded: Recorded<'_>) -> BTreeSet<String> {
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

    let mut opened = BTreeSet::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.weak("Drag a number onto a chart to see it over time.");
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
                    number(ui, trace, value);
                }
                if topic {
                    egui::CollapsingHeader::new("numbers inside").id_salt(("inside", &name)).show(ui, |ui| {
                        inside(ui, signals, &name);
                        opened.insert(name.clone());
                    });
                }
            });
            ui.separator();
        }
    });
    opened
}

/// The numbers inside the messages of a topic that is open in the list.
fn inside(ui: &mut Ui, signals: &Signals, topic: &str) {
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
        number(ui, Trace::Inside { topic: topic.to_owned(), number: name }, value.map_or(String::new(), written));
    }
}

/// One number in the list, with its value as it stands: the thing to drag.
fn number(ui: &mut Ui, trace: Trace, value: String) {
    let word = trace.word().to_owned();
    ui.dnd_drag_source(egui::Id::unique(("number", &trace)), trace, |ui| {
        ui.horizontal(|ui| {
            // The values line up in a column of their own, unless the name is too long for that.
            let word = ui.label(word);
            ui.add_space((64.0 - word.rect.width()).max(8.0));
            ui.weak(value);
        });
    });
}
