//! The details window: what is known of the node or the group that is
//! selected, the form that creates a node, and, while drafting, the changes
//! that are held back.
use super::{Asking, Rates, Wish, awaits};
use crate::draft::{Draft, Mark, Marks};
use crate::feed::Request;
use crate::graph::{self, Groups};
use eframe::egui::{self, Ui};
use serde_json::Value;
use triq::{FactoryInfo, Inventory, NodeInfo, NodeState, Placement, PoolInfo, PortSpec};

/// A node about to be created, as the user is filling it in.
pub(super) struct Form {
    runtime: String,
    kind: String,
    name: String,
    /// `None` for a thread of its own.
    pool: Option<String>,
    settings: String,
}

impl Form {
    pub(super) fn new(inventory: &Inventory, factory: &FactoryInfo) -> Self {
        // The first name of the form `kind1`, `kind2`, ... that is free in that runtime.
        let taken = |name: &String| inventory.nodes.iter().any(|node| node.name == *name);
        let name = (1..).map(|number| format!("{}{number}", factory.kind)).find(|name| !taken(name));
        Self {
            runtime: inventory.runtime.clone(),
            kind: factory.kind.clone(),
            name: name.unwrap_or_default(),
            pool: None,
            settings: pretty(&factory.params),
        }
    }

    /// Returns true when the form is done with, created or cancelled.
    pub(super) fn show(&mut self, ui: &mut Ui, asking: &mut Asking<'_>, inventories: &[Inventory]) -> bool {
        let offered = inventories.iter().find(|inventory| inventory.runtime == self.runtime).and_then(|inventory| {
            let factory = inventory.factories.iter().find(|factory| factory.kind == self.kind)?;
            Some((inventory, factory))
        });
        let Some((inventory, factory)) = offered else {
            return true; // the runtime or its factory went away
        };
        ui.heading(format!("Create a {} node", self.kind));
        ui.weak(format!("in {}", self.runtime));
        ui.add_space(8.0);

        let name = ui.label("Name");
        ui.text_edit_singleline(&mut self.name).labelled_by(name.id);
        if factory.local {
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
        ui.label("Settings");
        ui.add(egui::TextEdit::multiline(&mut self.settings).code_editor().desired_rows(4));
        let params = serde_json::from_str::<Value>(&self.settings);
        if let Err(error) = &params {
            ui.colored_label(ui.visuals().error_fg_color, format!("Not valid JSON: {error}"));
        }
        if !factory.info.is_null() {
            ui.label("From the factory");
            ui.monospace(pretty(&factory.info));
        }

        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            let ready = params.is_ok() && !self.name.is_empty();
            if ui.add_enabled(ready, egui::Button::new("Create")).clicked()
                && let Ok(params) = params
            {
                let placement = self.pool.as_deref().map_or(Placement::Dedicated, Placement::pool);
                let (runtime, kind, name) = (self.runtime.clone(), self.kind.clone(), self.name.clone());
                asking.ask(Request::Spawn { runtime, kind, name, placement, params });
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
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
    egui::Grid::new("facts").num_columns(2).show(ui, |ui| {
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
        fact("State", state(&node.state));
        if node.placement.is_some() {
            fact("Times run", node.runs.to_string());
        }
        if let Some(error) = &node.last_error {
            fact("Errors", format!("{}, last: {error}", node.errors));
        }
    });
    ui.horizontal(|ui| actions(ui, asking, &home.runtime, node));

    if !node.params.is_null() {
        ui.separator();
        ui.strong("Settings");
        ui.add(egui::TextEdit::multiline(settings).code_editor().desired_rows(3));
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
        ui.horizontal(|ui| {
            ui.label(port(&input.port));
            ui.weak(rate(rates, &name));
        });
        if input.sources.is_empty() {
            ui.weak("not connected");
        }
        for source in &input.sources {
            let drafted = marks.wire(&source.output, &name);
            let link = match (&source.problem, source.connected) {
                _ if drafted == Some(Mark::New) => "to be connected, when the draft is applied".to_owned(),
                (Some(problem), _) => problem.clone(),
                (None, true) => format!(
                    "{} received, {} lost, {} µs latency, {} µs longest gap",
                    source.received, source.lost, source.latency_us, source.gap_us
                ),
                (None, false) => "waiting for the output".to_owned(),
            };
            ui.weak(format!("from {}", source.output));
            ui.horizontal(|ui| {
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
        ui.horizontal(|ui| {
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
    ui.horizontal(|ui| {
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
        ui.text_edit_singleline(renamed).labelled_by(label.id);
        let changed = !renamed.trim().is_empty() && renamed != graph::called(group);
        if ui.add_enabled(changed, egui::Button::new("Rename")).clicked() {
            *wish = Some(Wish::Rename(group.to_owned(), renamed.clone()));
        }
    });
    ui.separator();
    ui.strong("Nodes");
    let mut clicked = None;
    for (node, of) in nodes {
        ui.horizontal(|ui| {
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
/// order it will be, and which nodes are to change group. Each can be taken back.
pub(super) fn held_back(ui: &mut Ui, draft: &mut Draft, real: &[Inventory], groups: &Groups) {
    ui.heading("Draft");
    let (changes, regrouped) = (draft.changes(real), draft.regrouped(groups));
    if changes.is_empty() && regrouped.is_empty() {
        ui.weak("What is changed in the graph and here is held back, until the draft is applied.");
    }
    let take_back = |ui: &mut Ui| ui.small_button("×").on_hover_text("Take this change back").clicked();
    for (number, change) in changes.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            if take_back(ui) {
                draft.take_back(change);
            }
            ui.label(format!("{}. {change}", number + 1));
        });
    }
    for (node, group) in &regrouped {
        ui.horizontal_wrapped(|ui| {
            if take_back(ui) {
                draft.leave(node);
            }
            ui.label(match group {
                Some(group) => format!("put {node} in group {group}"),
                None => format!("take {node} out of its group"),
            });
        });
    }
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
            Some(pool) => format!("pool {name}: one of its {} threads", pool.threads),
            None => format!("pool {name}"),
        },
    }
}
