//! A node's settings edited in its details, as a user would: "Apply changes" asks for what was
//! typed that the node does not have, and "Replace all" for all that is written.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use triq::prelude::*;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{Key, Modifiers};
use triq_workbench::feed::Request;
use window::{Bench, open, until};

#[derive(Ports)]
struct TunerPorts {
    #[allow(dead_code, reason = "a node needs a port; this one's is never sent on")]
    tuned: Output<u64>,
}

/// Two settings, so that one can be changed and the other left.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct TunerParams {
    gain: f64,
    offset: f64,
}

impl Default for TunerParams {
    fn default() -> Self {
        Self { gain: 1.0, offset: 0.0 }
    }
}

struct Tuner;

impl Node for Tuner {
    type Ports = TunerPorts;
    type Params = TunerParams;
    fn new(_: TunerParams) -> triq::Result<Self> {
        Ok(Tuner)
    }
    fn set_params(&mut self, _: TunerParams) -> triq::Result {
        Ok(())
    }
    fn run(&mut self, _: &mut TunerPorts) -> triq::Result {
        Ok(())
    }
}

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Click a button in the details. A card on the graph may have one of the same name; the details come after it.
fn press(harness: &mut Bench<'_>, label: &str) {
    harness.get_all_by_label(label).last().unwrap_or_else(|| panic!("nothing labelled {label:?}")).click();
    harness.run_steps(3);
}

/// The settings as the details have them written.
fn written(harness: &Bench<'_>) -> Value {
    let text = harness.get_by_role(Role::MultilineTextInput).value().expect("the settings' text");
    serde_json::from_str(&text).expect("settings that are JSON")
}

/// Write `settings` in place of what the details have written.
fn write(harness: &mut Bench<'_>, settings: &Value) {
    harness.get_by_role(Role::MultilineTextInput).focus();
    harness.run_steps(2);
    harness.key_combination_modifiers(Modifiers::COMMAND, &[Key::A]);
    harness.run_steps(2);
    harness.get_by_role(Role::MultilineTextInput).type_text(&settings.to_string());
    harness.run_steps(2);
}

#[test]
fn the_details_ask_for_the_settings_that_were_changed_or_for_all_of_them() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    scene.robot.register::<Tuner>("tuner")?;
    scene.robot.spawn_kind("tuner", "tuner", Placement::Dedicated, json!({ "gain": 2.0, "offset": 3.0 }))?;
    let has = || scene.robot.inventory().nodes.into_iter().find(|node| node.name == "tuner").unwrap().params;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| shows(harness, "tuner")), "the node appears");

    // Its details: its settings, with nothing to ask for yet.
    harness.get_all_by_label("tuner").next().expect("the node, among the runtimes").click();
    assert!(until(&mut harness, |harness| shows(harness, "Apply changes")), "its details are shown");
    assert!(shows(&harness, "Replace all") && shows(&harness, "No changes"));
    assert_eq!(written(&harness), json!({ "gain": 2.0, "offset": 3.0 }));

    // Something else changes one of its settings: what is written keeps up.
    scene.robot.change_params("tuner", json!({ "offset": 9.0 }))?;
    assert!(until(&mut harness, |harness| written(harness)["offset"] == 9.0), "{}", written(&harness));

    // The gain is typed. Drafting, so that what would be asked can be read: the gain, and no other setting.
    press(&mut harness, "Draft");
    write(&mut harness, &json!({ "gain": 5.0, "offset": 9.0 }));
    assert!(shows(&harness, r#"Changes: {"gain":5.0}"#), "it says what it would ask for");
    press(&mut harness, "Apply changes");
    let change =
        Request::ChangeParams { runtime: robot.clone(), node: "tuner".into(), changes: json!({ "gain": 5.0 }) };
    assert_eq!(harness.state().drafted(), [change]);
    assert_eq!(has(), json!({ "gain": 2.0, "offset": 9.0 }), "nothing is asked while it is drafted");
    press(&mut harness, "Apply draft");
    assert!(wait_for(|| has() == json!({ "gain": 5.0, "offset": 9.0 })), "{}", has());
    assert!(until(&mut harness, |harness| shows(harness, "No changes")), "the node has it: nothing is left to ask");

    // "Replace all" asks for what is written and nothing else: a setting that is not written is its default.
    write(&mut harness, &json!({ "gain": 7.0 }));
    press(&mut harness, "Replace all");
    assert!(wait_for(|| has() == json!({ "gain": 7.0, "offset": 0.0 })), "{}", has());
    assert!(until(&mut harness, |harness| written(harness) == json!({ "gain": 7.0, "offset": 0.0 })));
    assert!(shows(&harness, "No changes"));
    Ok(())
}
