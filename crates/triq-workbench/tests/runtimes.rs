//! The list of runtimes: a box finds a runtime by its name or by its process, and of the
//! kinds a runtime offers, one that makes a group of nodes says so on its button.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use serde_json::json;
use triq::{Group, Placement};
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, Key, Modifiers};
use window::*;

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// The workbench with the list of runtimes alone: the other windows name the runtimes too.
fn listing() -> triq::Result<Bench<'static>> {
    let mut harness = open()?;
    for window in ["Graph", "Details", "Charts"] {
        switch(&harness, window).click();
        harness.run_steps(3);
    }
    Ok(harness)
}

/// Save a picture of the window, for a human to look at. Drawing it needs a GPU.
fn picture(harness: &mut Bench<'_>, name: &str) {
    match harness.render() {
        Ok(image) => image.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).expect("save a picture"),
        Err(reason) => eprintln!("no picture: {reason}"),
    }
}

/// Write in the box that finds runtimes, in place of what is written there.
fn find(harness: &mut Bench<'_>, words: &str) {
    harness.get_by_role_and_label(Role::TextInput, "Find").click();
    harness.run_steps(2);
    key(harness, Modifiers::COMMAND, Key::A);
    key(harness, Modifiers::NONE, Key::Backspace);
    if !words.is_empty() {
        harness.event(egui::Event::Text(words.to_owned()));
    }
    harness.run_steps(6);
}

#[test]
fn a_runtime_is_found_by_its_name_or_by_its_process() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let listed = |harness: &Bench<'_>| (shows(harness, &robot), shows(harness, &desk));
    let mut harness = listing()?;
    assert!(until(&mut harness, |harness| listed(harness) == (true, true)), "both runtimes are listed");

    // What is typed is looked for in the names, in capitals or not: the runtime that has it is
    // listed, with what it offers and its nodes, and no other.
    find(&mut harness, &robot.to_uppercase());
    assert_eq!(listed(&harness), (true, false));
    assert!(shows(&harness, "New filter") && shows(&harness, "smoother") && !shows(&harness, "viewer"));
    // And in the processes: the two runtimes of the scene are one program's.
    let process = std::process::id();
    for words in [format!("process {process}"), process.to_string()] {
        find(&mut harness, &words);
        assert_eq!(listed(&harness), (true, true), "{words}");
    }
    // Every word is to be found in one runtime: this name, of that process.
    find(&mut harness, &format!("{desk} {process}"));
    assert_eq!(listed(&harness), (false, true));
    // Another process, and words that no runtime has: neither is listed, which is said.
    find(&mut harness, &format!("process {}", process + 1));
    assert_eq!(listed(&harness), (false, false));
    find(&mut harness, "no runtime is called this");
    assert!(listed(&harness) == (false, false) && shows(&harness, "No runtime has that in its name or its process."));
    // With nothing in the box, every runtime is listed again.
    find(&mut harness, "");
    assert_eq!(listed(&harness), (true, true));
    Ok(())
}

#[test]
fn a_kind_that_makes_a_group_says_so_on_its_button() -> triq::Result {
    let scene = Scene::new()?;
    // Other tests have robots that offer filters too: this one's kind of group is its own.
    let kind = unique("pair");
    let pair = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .spawn("filter", "fine", Placement::Dedicated, json!({ "alpha": 0.1 }))
        .connect("coarse/smooth", "fine/raw");
    scene.robot.register_group(&kind, pair)?;
    let mut harness = listing()?;
    assert!(until(&mut harness, |harness| shows(harness, scene.robot.name())));
    find(&mut harness, scene.robot.name());

    // A kind that makes one node, and a kind that makes a group of them.
    let (node, group) = ("New filter".to_owned(), format!("New group {kind}"));
    assert!(shows(&harness, &node) && shows(&harness, &group), "each says what it makes");
    assert!(!shows(&harness, "New group filter") && !shows(&harness, &format!("New {kind}")));
    // The button for a group is told from the other at a glance, not by its words alone: it is
    // drawn with a line around it.
    let outlined = |harness: &Bench<'_>, label: &str| {
        let button = harness.get_by_label(label).rect();
        let mut strokes = Vec::new();
        fn rects(shape: &egui::Shape, found: &mut Vec<(egui::Rect, f32)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| rects(shape, found)),
                egui::Shape::Rect(rect) => found.push((rect.rect, rect.stroke.width)),
                _ => {}
            }
        }
        harness.output().shapes.iter().for_each(|clipped| rects(&clipped.shape, &mut strokes));
        strokes.iter().any(|(rect, width)| {
            *width > 0.0 && rect.expand(2.0).contains_rect(button) && button.expand(4.0).contains_rect(*rect)
        })
    };
    assert!(outlined(&harness, &group) && !outlined(&harness, &node));
    picture(&mut harness, "runtimes.png");
    // It makes what its kind makes, as before: the form to create one opens.
    harness.get_by_label(&group).click();
    assert!(until(&mut harness, |harness| shows(harness, "Create")));
    Ok(())
}
