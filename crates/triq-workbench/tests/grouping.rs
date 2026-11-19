//! Groups in the workbench: made, looked into and taken apart with the keys
//! that Blender has for it, kept from one run to the next, and held back
//! with the rest while drafting.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, Key, Modifiers, PointerButton};
use triq_workbench::graph::{Card, Groups};
use window::*;

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// The cards on the graph, by what they are called.
fn cards(harness: &Bench<'_>) -> Vec<String> {
    let mut cards: Vec<String> = harness.state().graph().nodes().map(Card::key).collect();
    cards.sort();
    cards
}

fn sorted<const N: usize>(mut names: [String; N]) -> Vec<String> {
    names.sort();
    names.to_vec()
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

fn groups<const N: usize>(members: [(&String, &str); N]) -> Groups {
    members.into_iter().map(|(node, group)| (node.clone(), group.to_owned())).collect()
}

#[test]
fn nodes_are_grouped_and_a_group_is_looked_into_and_taken_apart_with_blenders_keys() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (imu, smoother, viewer) = (format!("{robot}/imu"), format!("{robot}/smoother"), format!("{desk}/viewer"));
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    assert_eq!(cards(&harness), sorted([imu.clone(), smoother.clone(), viewer.clone()]));

    // Select the sensor and the filter, with Shift held, and press Ctrl+G: they are a group, drawn as one card.
    for card in ["imu", "smoother"] {
        let name = header(&harness, card);
        shift_click(&mut harness, name);
    }
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&imu, "group1"), (&smoother, "group1")]));
    assert_eq!(cards(&harness), sorted(["/group1".to_owned(), viewer.clone()]));
    // The card shows the one port that leads out of the group, and the details are about the group.
    assert!(shows(&harness, "smoother.smooth: Pose") && !shows(&harness, "raw: Pose"));
    assert!(shows(&harness, "a group of 2 nodes") && shows(&harness, "2 nodes"));

    // Give it a name, in the details.
    harness.get_by_role_and_label(Role::TextInput, "Name").click();
    harness.run_steps(2);
    key(&mut harness, Modifiers::COMMAND, Key::A);
    harness.event(egui::Event::Text("sensing".to_owned()));
    harness.run_steps(2);
    harness.get_by_label("Rename").click();
    harness.run_steps(4);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing"), (&smoother, "sensing")]));
    assert_eq!(cards(&harness), sorted(["/sensing".to_owned(), viewer.clone()]));
    picture(&mut harness, "group.png");

    // Click its card. With the pointer over the graph, Tab shows the inside of the group that is
    // selected: its nodes, and a card that stands for the outside, which the filter's output goes to.
    let card = header(&harness, "sensing");
    click(&mut harness, card, PointerButton::Primary);
    harness.key_press(Key::Tab);
    harness.run_steps(4);
    assert_eq!(harness.state().inside(), Some("sensing"));
    assert_eq!(cards(&harness), sorted(["/sensing/out".to_owned(), imu.clone(), smoother.clone()]));
    assert!(shows(&harness, "Group Output") && shows(&harness, "inside sensing") && shows(&harness, "Exit Group"));
    picture(&mut harness, "group-inside.png");

    // Inside, click the filter and press P: it is moved out of the group. The sensor is what is left in it.
    let filter = header(&harness, "smoother");
    click(&mut harness, filter, PointerButton::Primary);
    key(&mut harness, Modifiers::NONE, Key::P);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing")]));
    assert_eq!(cards(&harness), sorted(["/sensing/out".to_owned(), imu.clone()]));

    // Ctrl+Tab leaves the group. From outside it is one card again, beside the filter and the viewer.
    key(&mut harness, Modifiers::CTRL, Key::Tab);
    assert_eq!(harness.state().inside(), None);
    assert_eq!(cards(&harness), sorted(["/sensing".to_owned(), smoother.clone(), viewer.clone()]));
    // A double click on the card of a group shows its inside too, and so does the button on it;
    // the button above the graph leads out.
    let card = header(&harness, "sensing");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("sensing"));
    harness.get_all_by_label("Exit Group").next().expect("the way out, above the graph").click();
    harness.run_steps(4);
    assert_eq!(harness.state().inside(), None);

    // In the program itself, Tab is taken out of what was typed before a frame makes anything
    // of it, while the pointer is over the graph: it does not move on to the next button.
    let card = header(&harness, "sensing");
    click(&mut harness, card, PointerButton::Primary);
    let (button, modifiers) = (None, Modifiers::NONE);
    let tab = egui::Event::Key { key: Key::Tab, physical_key: button, pressed: true, repeat: false, modifiers };
    let mut typed = egui::RawInput { events: vec![tab], ..Default::default() };
    let window = harness.ctx.clone();
    harness.state_mut().keys(&window, &mut typed);
    assert!(typed.events.is_empty(), "the graph has kept the Tab");
    harness.run_steps(3);
    assert_eq!(harness.state().inside(), Some("sensing"));
    harness.key_press(Key::Tab);
    harness.run_steps(4);
    assert_eq!(harness.state().inside(), None, "and Tab leaves the group again");

    // What is grouped is kept from one run of the workbench to the next.
    let layout = harness.state().layout();
    let mut next = open()?;
    next.state_mut().arrange(&layout);
    assert!(until(&mut next, |next| cards(next) == sorted(["/sensing".to_owned(), smoother.clone(), viewer.clone()])));
    drop(next);

    // The group that was left is the one that is selected: Ctrl+Alt+G takes it apart, and its nodes are cards again.
    let card = header(&harness, "sensing");
    harness.hover_at(card);
    harness.run_steps(2);
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    assert_eq!(harness.state().groups(), &Groups::new());
    assert_eq!(cards(&harness), sorted([imu, smoother, viewer]));
    Ok(())
}

#[test]
fn which_nodes_are_grouped_is_held_back_while_drafting() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (imu, smoother, viewer) = (format!("{robot}/imu"), format!("{robot}/smoother"), format!("{desk}/viewer"));
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    let group = |harness: &mut Bench<'_>| {
        for card in ["imu", "smoother"] {
            let name = header(harness, card);
            shift_click(harness, name);
        }
        key(harness, Modifiers::CTRL, Key::G);
    };

    // In a draft, the sensor and the filter are put in a group. The graph shows them so, as drafted.
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    group(&mut harness);
    assert_eq!(cards(&harness), sorted(["/group1".to_owned(), viewer.clone()]));
    assert!(shows(&harness, "to be created") && shows(&harness, "Draft: 2 changes"));
    // But they are not grouped yet. The draft lists what it holds back, and each can be taken back.
    assert_eq!(harness.state().groups(), &Groups::new());
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);
    click(&mut harness, empty, PointerButton::Primary);
    for node in [&imu, &smoother] {
        assert!(shows(&harness, &format!("put {node} in group group1")));
    }
    picture(&mut harness, "draft-group.png");

    // Discarded, the draft leaves the cards as they were.
    harness.get_by_label("Discard draft").click();
    harness.run_steps(4);
    assert_eq!(cards(&harness), sorted([imu.clone(), smoother.clone(), viewer.clone()]));

    // Drafted again and applied, they are a group.
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    group(&mut harness);
    harness.get_by_label("Apply draft").click();
    harness.run_steps(4);
    assert_eq!(harness.state().groups(), &groups([(&imu, "group1"), (&smoother, "group1")]));
    assert_eq!(cards(&harness), sorted(["/group1".to_owned(), viewer]));
    assert!(!shows(&harness, "to be created") && !shows(&harness, "Apply draft"));
    Ok(())
}
