//! What a group is asked as one, in its details: every node in it paused, started again or
//! removed, at once, or held back in a draft with the rest.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use serde_json::json;
use triq::{Group, NodeState, Placement};
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, Key, Modifiers, PointerButton};
use triq_workbench::graph::{self, Card};
use window::*;

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Ask the robot for something of a kind it offers: pick the kind among the runtimes, and create
/// it under the name the form proposes. The list of runtimes may be long, with the kind out of sight.
fn create(harness: &mut Bench<'_>, kind: &str) {
    let button = format!("New {kind}");
    harness.get_all_by_label(&button).next().expect("the factory").scroll_to_me();
    harness.run_steps(3);
    harness.get_all_by_label(&button).next().expect("the factory").click();
    assert!(until(harness, |harness| shows(harness, "Create")));
    harness.get_by_label("Create").click();
    harness.run_steps(3);
}

/// Where on screen the name at the top of a card is. The details may be headed
/// by the same name: they are drawn after the graph.
fn header(harness: &Bench<'_>, card: &str) -> egui::Pos2 {
    let name = harness.query_all_by_role_and_label(Role::Label, card).next();
    seen(harness, &name.unwrap_or_else(|| panic!("no card is called {card:?}")))
}

/// Save a picture of the window, for a human to look at. Drawing it needs a GPU.
fn picture(harness: &mut Bench<'_>, name: &str) {
    match harness.render() {
        Ok(image) => image.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).expect("save a picture"),
        Err(reason) => eprintln!("no picture: {reason}"),
    }
}

/// Whether the graph has a card by this key.
fn has_card(harness: &Bench<'_>, key: &str) -> bool {
    harness.state().graph().nodes().any(|card| Card::key(card) == key)
}

/// Lay the graph out and bring all of it into view, so that a card is in view and under no
/// other: the graph draws whatever else runs on the machine as well.
fn tidy(harness: &mut Bench<'_>) {
    for button in ["Tidy", "Fit"] {
        harness.get_by_label(button).click();
        harness.run_steps(4);
    }
}

/// Click the card of a group, and see that the details are about it.
#[track_caller]
fn turn_to(harness: &mut Bench<'_>, group: &str, size: &str) {
    tidy(harness);
    let card = header(harness, group);
    click(harness, card, PointerButton::Primary);
    harness.run_steps(3);
    assert!(shows(harness, size), "the details are about {group}: {size}");
}

/// The robot of a scene, which offers two filters, one after the other, as one kind, `pair`.
fn scene_with_pairs() -> triq::Result<Scene> {
    let scene = Scene::new()?;
    let pair = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .spawn("filter", "fine", Placement::pool("dsp"), json!({ "alpha": 0.1 }))
        .connect("coarse/smooth", "fine/raw");
    scene.robot.register_group("pair", pair)?;
    Ok(scene)
}

/// What the robot says each of these nodes is doing: none for one that is not there.
fn states<const N: usize>(scene: &Scene, nodes: [&str; N]) -> [Option<NodeState>; N] {
    let reported = scene.robot.inventory().nodes;
    nodes.map(|name| reported.iter().find(|node| node.name == name).map(|node| node.state.clone()))
}

const RUNNING: Option<NodeState> = Some(NodeState::Running);
const PAUSED: Option<NodeState> = Some(NodeState::Paused);

#[test]
fn every_node_of_a_group_is_paused_started_again_and_removed_from_the_details_of_the_group() -> triq::Result {
    let _alone = alone();
    let scene = scene_with_pairs()?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    create(&mut harness, "pair");
    assert!(until(&mut harness, |harness| has_card(harness, "/pair1")), "the group is drawn as one card");
    let pair = ["pair1.coarse", "pair1.fine"];

    // Click its card: the details are about the group, list its nodes, and have what all of them
    // can be asked together. They all run, so there is nothing to start again.
    turn_to(&mut harness, "pair1", "a group of 2 nodes");
    assert!(shows(&harness, "Pause all") && shows(&harness, "Remove all") && !shows(&harness, "Resume all"));

    // Pause all: both are paused, the list says so of each, and now they can be started again.
    harness.get_by_label("Pause all").click();
    assert!(until(&mut harness, |_| states(&scene, pair) == [PAUSED, PAUSED]), "{:?}", states(&scene, pair));
    assert!(until(&mut harness, |harness| harness.query_all_by_label("paused").count() == 2));
    assert!(shows(&harness, "Resume all") && !shows(&harness, "Pause all"));

    // One of them is started again by other means: some run and some do not, and the group can
    // be asked either. Only the node that is paused is said to be.
    scene.robot.resume("pair1.fine")?;
    assert!(until(&mut harness, |harness| shows(harness, "Pause all") && shows(harness, "Resume all")));
    assert_eq!(harness.query_all_by_label("paused").count(), 1);
    picture(&mut harness, "group-details.png");
    harness.get_by_label("Resume all").click();
    assert!(until(&mut harness, |_| states(&scene, pair) == [RUNNING, RUNNING]), "{:?}", states(&scene, pair));
    assert!(until(&mut harness, |harness| !shows(harness, "Resume all") && !shows(harness, "paused")));

    // In a draft, removing the group is held back with the rest: a change for each of its
    // nodes, and nothing removed. The draft is discarded.
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    harness.get_by_label("Remove all").click();
    harness.run_steps(3);
    assert!(shows(&harness, "Draft: 2 changes"));
    assert_eq!(states(&scene, pair), [RUNNING, RUNNING]);
    harness.get_by_label("Discard draft").click();
    harness.run_steps(4);

    // Out of the draft, Remove all removes them: its nodes are gone, its card with them, and
    // the details are about nothing. The rest of the machine is as it was.
    turn_to(&mut harness, "pair1", "a group of 2 nodes");
    harness.get_by_label("Remove all").click();
    assert!(until(&mut harness, |_| states(&scene, pair) == [None, None]), "{:?}", states(&scene, pair));
    assert!(until(&mut harness, |harness| !has_card(harness, "/pair1") && !shows(harness, "a group of 2 nodes")));
    assert!(!shows(&harness, "Remove all"));
    assert_eq!(states(&scene, ["smoother"]), [RUNNING]);
    Ok(())
}

#[test]
fn a_group_asks_the_nodes_of_a_group_inside_it_too_and_passes_over_a_node_that_cannot_be_paused() -> triq::Result {
    let _alone = alone();
    let scene = scene_with_pairs()?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    create(&mut harness, "pair");
    assert!(until(&mut harness, |harness| has_card(harness, "/pair1")), "the group is drawn as one card");

    // Put the sensor, the filter and the pair in a group together. The sensor is the test's
    // own: its application runs it, and it cannot be paused.
    let taken =
        |harness: &Bench<'_>, name: &String| harness.state().groups().values().any(|of| graph::within(of, name));
    let made = (1..).map(|number| format!("group{number}")).find(|name| !taken(&harness, name)).expect("a free name");
    tidy(&mut harness);
    for card in ["imu", "smoother", "pair1"] {
        let name = header(&harness, card);
        shift_click(&mut harness, name);
    }
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert!(until(&mut harness, |harness| has_card(harness, &format!("/{made}"))), "the three are one card");
    let all = ["imu", "smoother", "pair1.coarse", "pair1.fine"];

    // The details are about the new group: four nodes, two of them in the group inside it.
    turn_to(&mut harness, &made, "a group of 4 nodes");
    assert_eq!(harness.query_all_by_label("in pair1").count(), 2);

    // Pause all pauses those inside the inner group as well, and leaves the sensor running.
    harness.get_by_label("Pause all").click();
    assert!(
        until(&mut harness, |_| states(&scene, all) == [RUNNING, PAUSED, PAUSED, PAUSED]),
        "{:?}",
        states(&scene, all)
    );
    assert!(until(&mut harness, |harness| harness.query_all_by_label("paused").count() == 3));
    // Nothing in it is left to pause.
    assert!(shows(&harness, "Resume all") && !shows(&harness, "Pause all"));

    // Remove all removes every node in it, the inner group's as well.
    harness.get_by_label("Remove all").click();
    assert!(until(&mut harness, |_| states(&scene, all) == [None, None, None, None]), "{:?}", states(&scene, all));
    assert!(until(&mut harness, |harness| !has_card(harness, &format!("/{made}"))));
    assert!(!shows(&harness, "a group of 4 nodes"));
    Ok(())
}
