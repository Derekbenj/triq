//! The details window: what is known of the node or the group that is
//! selected, the form that creates a node, and, while drafting, the changes
//! that are held back.
use super::{Asking, Rates, Wish, awaits};
use crate::draft::{Draft, Mark, Marks};
use crate::feed::Request;
use crate::graph;
use eframe::egui::{self, Ui};
use serde_json::Value;
use triq::{FactoryInfo, Inventory, NodeInfo, NodeState, Placement, PoolInfo, PortSpec};

/// A node about to be created, as the user is filling it in; or a group, of
/// a kind that its runtime offers as one.
pub(super) struct Form {
    runtime: String,
    kind: String,
    name: String,
    /// `None` for a thread of its own.
    pool: Option<String>,
    settings: String,
    /// The names of the nodes that were asked for with this form.
    made: Vec<String>,
}

/// What became of a form in a frame.
pub(super) enum Filled {
    /// It is still being filled in.
    Open,
    /// Something was asked for: these nodes, each `runtime/node` with the
    /// group it is made in, which is none unless the kind is a group. The
    /// form stays, for another of the kind.
    Created { nodes: Vec<(String, String)> },
    /// It is done with: closed, or its runtime or factory went away.
    Closed,
}

impl Form {
    pub(super) fn new(inventory: &Inventory, factory: &FactoryInfo) -> Self {
        let mut form = Self {
            runtime: inventory.runtime.clone(),
            kind: factory.kind.clone(),
            name: String::new(),
            pool: None,
            settings: pretty(&factory.params),
            made: Vec::new(),
        };
        form.name = form.free(inventory);
        form
    }

    /// The first name of the form `kind1`, `kind2`, ... that is free in that
    /// runtime, and that this form has not asked for a node by: a node that
    /// was asked for a moment ago may not be reported yet. The nodes of a
    /// group that was created by a name are called `name.node`.
    fn free(&self, inventory: &Inventory) -> String {
        let named = |node: &NodeInfo, name: &str| {
            node.name.strip_prefix(name).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        };
        let taken = |name: &String| inventory.nodes.iter().any(|node| named(node, name)) || self.made.contains(name);
        (1..).map(|number| format!("{}{number}", self.kind)).find(|name| !taken(name)).unwrap_or_default()
    }

    pub(super) fn show(&mut self, ui: &mut Ui, asking: &mut Asking<'_>, inventories: &[Inventory]) -> Filled {
        let offered = inventories.iter().find(|inventory| inventory.runtime == self.runtime).and_then(|inventory| {
            let factory = inventory.factories.iter().find(|factory| factory.kind == self.kind)?;
            Some((inventory, factory))
        });
        let Some((inventory, factory)) = offered else { return Filled::Closed };
        let what = if factory.group.is_some() { "group" } else { "node" };
        ui.heading(format!("Create a {} {what}", self.kind));
        ui.weak(format!("in {}", self.runtime));
        ui.add_space(8.0);

        let name = ui.label("Name");
        ui.add(wide(egui::TextEdit::singleline(&mut self.name))).labelled_by(name.id);
        if let Some(group) = &factory.group {
            // Where its nodes run and how they are set is the kind's to say.
            ui.weak("Its nodes are created together, wired to each other, in a group of this name:");
            for node in &group.nodes {
                ui.label(format!("{}, a {}", node.name, node.kind));
            }
        } else if factory.local {
            ui.weak("The application that offers this kind runs its nodes itself.");
        } else {
            ui.label("Runs on");
            let chosen = self.pool.as_ref().map_or("a thread of its own".to_owned(), |pool| format!("pool {pool}"));
            egui::ComboBox::from_id_salt("runs-on").selected_text(chosen).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.pool, None, "a thread of its own");
                for pool in &inventory.pools {
                    ui.selectable_value(&mut self.pool, Some(pool.name.clone()), format!("pool {}", pool.name));
                }
            });
        }
        if factory.group.is_none() {
            ui.label("Settings");
            ui.add(wide(egui::TextEdit::multiline(&mut self.settings)).code_editor().desired_rows(4));
        }
        let params = serde_json::from_str::<Value>(&self.settings);
        if let Err(error) = &params {
            ui.colored_label(ui.visuals().error_fg_color, format!("Not valid JSON: {error}"));
        }
        if !factory.info.is_null() {
            ui.label("From the factory");
            ui.monospace(pretty(&factory.info));
        }

        ui.add_space(8.0);
        let mut filled = Filled::Open;
        ui.horizontal_wrapped(|ui| {
            let ready = params.is_ok() && !self.name.is_empty();
            if ui.add_enabled(ready, egui::Button::new("Create")).clicked()
                && let Ok(params) = params
            {
                let placement = self.pool.as_deref().map_or(Placement::Dedicated, Placement::pool);
                let (runtime, kind, name) = (self.runtime.clone(), self.kind.clone(), self.name.clone());
                // One node by that name, in no group; or the nodes of a group, which are named
                // after it, each in it or in a group inside it.
                let member = |node: &triq::Member| (format!("{runtime}/{}", node.named(&name)), node.group_in(&name));
                let nodes = match &factory.group {
                    Some(group) => group.nodes.iter().map(member).collect(),
                    None => vec![(format!("{runtime}/{name}"), String::new())],
                };
                filled = Filled::Created { nodes };
                asking.ask(Request::Spawn { runtime, kind, name: name.clone(), placement, params });
                // The form stays as it is, for another node of the kind, under the next name that is free.
                self.made.push(name);
                self.name = self.free(inventory);
            }
            if ui.button("Close").clicked() {
                filled = Filled::Closed;
            }
        });
        filled
    }
}

/// What is known of a node. `home` is what its runtime last reported, or
/// while drafting that with the draft's changes made.
pub(super) fn node_details(
    ui: &mut Ui,
    asking: &mut Asking<'_>,
    marks: &Marks,
    home: &Inventory,
    node: &NodeInfo,
    settings: &mut String,
    rates: &Rates,
) {
    let key = format!("{}/{}", home.runtime, node.name);
    ui.heading(&node.name);
    if let Some(mark) = marks.card(&key) {
        ui.weak(format!("{}, when the draft is applied", awaits(mark)));
    }
    ui.add_space(8.0);
    // What is said of it has the room that the window has, beside the words for what is said.
    let room = (ui.available_width() - 90.0).max(90.0);
    egui::Grid::new("facts").num_columns(2).max_col_width(room).show(ui, |ui| {
        let mut fact = |name: &str, value: String| {
            ui.weak(name);
            ui.label(value);
            ui.end_row();
        };
        fact("Runs in", format!("{}, process {}", home.runtime, home.pid));
        fact("Runs on", runs_on(&node.placement, &home.pools));
        if !node.kind.is_empty() {
            fact("Kind", node.kind.clone());
        }
        if !node.group.is_empty() {
            fact("Group", node.group.clone());
        }
        fact("State", state(&node.state));
        if node.placement.is_some() {
            fact("Times run", node.runs.to_string());
        }
        if let Some(error) = &node.last_error {
            fact("Errors", format!("{}, last: {error}", node.errors));
        }
    });
    ui.horizontal_wrapped(|ui| actions(ui, asking, &home.runtime, node));

    if !node.params.is_null() {
        ui.separator();
        ui.strong("Settings");
        ui.add(wide(egui::TextEdit::multiline(settings)).code_editor().desired_rows(3));
        match serde_json::from_str::<Value>(settings) {
            Ok(params) => {
                if ui.button("Apply").clicked() {
                    asking.ask(Request::SetParams { runtime: home.runtime.clone(), node: node.name.clone(), params });
                }
            }
            Err(error) => _ = ui.colored_label(ui.visuals().error_fg_color, format!("Not valid JSON: {error}")),
        }
    }

    for input in &node.inputs {
        let name = format!("{key}/{}", input.port.name);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(port(&input.port));
            ui.weak(rate(rates, &name));
        });
        if input.sources.is_empty() {
            ui.weak("not connected");
        }
        for source in &input.sources {
            let drafted = marks.wire(&source.output, &name);
            let link = match drafted {
                Some(Mark::New) => "to be connected, when the draft is applied".to_owned(),
                _ => format!(
                    "{} received, {} lost, {} µs latency, {} µs longest gap",
                    source.received, source.lost, source.latency_us, source.gap_us
                ),
            };
            ui.weak(format!("from {}", source.output));
            ui.horizontal_wrapped(|ui| {
                ui.weak(link);
                if drafted == Some(Mark::Going) {
                    ui.weak("to be disconnected");
                } else if ui.small_button("Disconnect").clicked() {
                    asking.ask(Request::Disconnect { from: source.output.clone(), to: name.clone() });
                }
            });
        }
    }
    for output in &node.outputs {
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(port(&output.port));
            ui.weak(rate(rates, &output.service));
        });
        ui.weak(format!("{} sent to {} inputs", output.sent, output.subscribers));
        if output.undelivered > 0 {
            ui.weak(format!("{} could not be delivered", output.undelivered));
        }
    }
}

/// What is known of a group: what it is called, which can be changed, and
/// every node in it, each `runtime/node` with the group it is in. `shown` is
/// whether the graph shows the inside of this group. Returns the node that
/// was clicked, to see its details.
pub(super) fn group_details(
    ui: &mut Ui,
    group: &str,
    renamed: &mut String,
    nodes: &[(&String, &String)],
    mark: Option<Mark>,
    shown: bool,
    wish: &mut Option<Wish>,
) -> Option<String> {
    ui.heading(graph::called(group));
    let size =
        if nodes.len() == 1 { "a group of 1 node".to_owned() } else { format!("a group of {} nodes", nodes.len()) };
    ui.weak(graph::parent(group).map_or(size.clone(), |outer| format!("{size}, inside {outer}")));
    if let Some(mark) = mark {
        ui.weak(format!("{}, when the draft is applied", awaits(mark)));
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        let about = if shown { "Show what is around the group again" } else { "Show what is inside it" };
        if ui.button(if shown { "Exit Group" } else { "Edit Group" }).on_hover_text(about).clicked() {
            *wish = Some(if shown { Wish::Leave } else { Wish::Enter(group.to_owned()) });
        }
        let about = "Take the group apart. What was in it is where the group was, and runs as before";
        if ui.button("Ungroup").on_hover_text(about).clicked() {
            *wish = Some(Wish::Ungroup(group.to_owned()));
        }
    });
    ui.separator();
    let label = ui.label("Name");
    ui.horizontal(|ui| {
        // The button is put down first, at the far end, and the name has the room that is left.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let changed = !renamed.trim().is_empty() && renamed != graph::called(group);
            if ui.add_enabled(changed, egui::Button::new("Rename")).clicked() {
                *wish = Some(Wish::Rename(group.to_owned(), renamed.clone()));
            }
            ui.add(wide(egui::TextEdit::singleline(renamed))).labelled_by(label.id);
        });
    });
    ui.separator();
    ui.strong("Nodes");
    let mut clicked = None;
    for (node, of) in nodes {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(false, node.as_str()).clicked() {
                clicked = Some((*node).clone());
            }
            // A node of a group inside this one says which.
            if let Some(inner) = of.strip_prefix(group).and_then(|rest| rest.strip_prefix('/')) {
                ui.weak(format!("in {inner}"));
            }
        });
    }
    clicked
}

/// What can be done about groups with the cards that are selected, or with
/// the one the details are about: put them in a new group, in one of the
/// groups that are on the graph, or, inside a group, one group out. `keys`
/// are what the keys for the first and the last of those are called.
pub(super) fn grouping(
    ui: &mut Ui,
    cards: &[String],
    groups: &[String],
    inside: bool,
    keys: [&str; 2],
    wish: &mut Option<Wish>,
) {
    ui.separator();
    ui.strong(if cards.len() == 1 { "Group".to_owned() } else { format!("Group: {} are selected", cards.len()) });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Make Group").on_hover_text(format!("Put it in a new group. {} does the same", keys[0])).clicked()
        {
            *wish = Some(Wish::Group(cards.to_vec()));
        }
        let about = format!("Into the group around this one, or into none. {} does the same", keys[1]);
        if inside && ui.button("Move out").on_hover_text(about).clicked() {
            *wish = Some(Wish::Separate(cards.to_vec()));
        }
        // A group is not moved into itself.
        let others: Vec<&String> = groups.iter().filter(|group| !cards.contains(&graph::group_key(group))).collect();
        if !others.is_empty() {
            egui::ComboBox::from_id_salt("move-into").selected_text("Move into").show_ui(ui, |ui| {
                for group in others {
                    if ui.selectable_label(false, graph::called(group)).clicked() {
                        *wish = Some(Wish::Insert(cards.to_vec(), group.clone()));
                    }
                }
            });
        }
    });
}

/// How a node is paused or started again: the word for it and the request.
/// `None` for a node that can be neither.
pub(super) fn toggle(runtime: &str, node: &NodeInfo) -> Option<(&'static str, Request)> {
    let (runtime, name) = (runtime.to_owned(), node.name.clone());
    // A node that came from a factory can be paused: by its runtime, or by the
    // application that offered the factory. Ports an application merely holds cannot.
    let pausable = node.placement.is_some() || !node.kind.is_empty();
    match &node.state {
        NodeState::Running if pausable => Some(("Pause", Request::Pause { runtime, node: name })),
        NodeState::Paused if pausable => Some(("Resume", Request::Resume { runtime, node: name })),
        _ => None,
    }
}

/// The buttons that act on a node. True if one was pressed.
pub(super) fn actions(ui: &mut Ui, asking: &mut Asking<'_>, runtime: &str, node: &NodeInfo) -> bool {
    let mut pressed = false;
    if let Some((word, request)) = toggle(runtime, node)
        && ui.button(word).clicked()
    {
        asking.ask(request);
        pressed = true;
    }
    if ui.button("Remove").clicked() {
        asking.ask(Request::Remove { runtime: runtime.to_owned(), node: node.name.clone() });
        pressed = true;
    }
    pressed
}

/// The changes a draft holds: what is to be asked of the runtimes, in the
/// order it will be. Each can be taken back.
pub(super) fn held_back(ui: &mut Ui, draft: &mut Draft, real: &[Inventory]) {
    ui.heading("Draft");
    let changes = draft.changes(real);
    if changes.is_empty() {
        ui.weak("What is changed in the graph and here is held back, until the draft is applied.");
    }
    for (number, change) in changes.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("×").on_hover_text("Take this change back").clicked() {
                draft.take_back(change, real);
            }
            ui.label(format!("{}. {change}", number + 1));
        });
    }
}

/// A place to write in that is as wide as the room it is given, and narrower in a narrower window.
fn wide(field: egui::TextEdit<'_>) -> egui::TextEdit<'_> {
    field.desired_width(f32::INFINITY)
}

pub(super) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// `name: Type`, as written beside a pin.
fn port(spec: &PortSpec) -> String {
    format!("{}: {}", spec.name, spec.message)
}

/// How fast data is passing through a port, such as `120/s`, or `0/s` when
/// none is; nothing for a port so new that it is not known yet.
pub(super) fn rate(rates: &Rates, name: &str) -> String {
    match rates.get(name) {
        Some(rate) if *rate >= 10.0 || *rate == 0.0 => format!("{rate:.0}/s"),
        Some(rate) => format!("{rate:.1}/s"),
        None => String::new(),
    }
}

fn state(state: &NodeState) -> String {
    match state {
        NodeState::Running => "running".to_owned(),
        NodeState::Paused => "paused".to_owned(),
        NodeState::Stopped(panic) => format!("stopped by a panic: {panic}"),
    }
}

/// The thread that runs a node, given the pools of its runtime.
fn runs_on(placement: &Option<Placement>, pools: &[PoolInfo]) -> String {
    match placement {
        None => "the application's own loop".to_owned(),
        Some(Placement::Dedicated) => "a thread of its own".to_owned(),
        Some(Placement::Pool(name)) => match pools.iter().find(|pool| pool.name == *name) {
            Some(pool) if pool.shared => format!("pool {name}: any of its {} threads", pool.threads),
            Some(pool) if pool.polling => {
                format!("pool {name}: one of its {} threads, which never sleep", pool.threads)
            }
            Some(pool) => format!("pool {name}: one of its {} threads", pool.threads),
            None => format!("pool {name}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_a_node_runs_says_what_kind_of_pool_it_is_in() {
        let pool = |name: &str, threads, shared, polling| PoolInfo { name: name.into(), threads, shared, polling };
        let pools = [pool("dsp", 2, false, false), pool("heavy", 8, true, false), pool("fast", 1, false, true)];
        let on = |name: &str| runs_on(&Some(Placement::pool(name)), &pools);
        assert_eq!(on("dsp"), "pool dsp: one of its 2 threads");
        assert_eq!(on("heavy"), "pool heavy: any of its 8 threads");
        assert_eq!(on("fast"), "pool fast: one of its 1 threads, which never sleep");
        assert_eq!(on("gone"), "pool gone");
        assert_eq!(runs_on(&Some(Placement::Dedicated), &pools), "a thread of its own");
        assert_eq!(runs_on(&None, &pools), "the application's own loop");
    }
}
