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

/// The cards on the graph, by what they are called: those of groups, and those of the nodes
/// of this test. Other programs may be running, and their nodes have cards too.
fn cards(harness: &Bench<'_>) -> Vec<String> {
    let own = format!("w{}-", std::process::id());
    let cards = harness.state().graph().nodes().map(Card::key);
    let mut cards: Vec<String> = cards.filter(|key| key.starts_with('/') || key.starts_with(&own)).collect();
    cards.sort();
    cards
}

/// Ask for a filter in the robot: pick the kind among the runtimes, and create it under the
/// name the form proposes. The list of runtimes may be long, with the kind out of sight.
fn create(harness: &mut Bench<'_>) {
    harness.get_all_by_label("New filter").next().expect("the factory").scroll_to_me();
    harness.run_steps(3);
    harness.get_all_by_label("New filter").next().expect("the factory").click();
    assert!(until(harness, |harness| shows(harness, "Create")));
    harness.get_by_label("Create").click();
}

/// How many headers fade from one colour to another: that of a group whose nodes run in more
/// than one runtime does. Such a header is drawn as a mesh of several colours.
fn fading(harness: &Bench<'_>) -> usize {
    fn meshes(shape: &egui::Shape) -> usize {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(meshes).sum(),
            egui::Shape::Mesh(mesh) => {
                // The shade at the end of a list that goes on fades too, to nothing.
                let first = mesh.vertices.first().map(|vertex| vertex.color);
                let solid = mesh.vertices.iter().all(|vertex| vertex.color.is_opaque());
                usize::from(solid && mesh.vertices.iter().any(|vertex| Some(vertex.color) != first))
            }
            _ => 0,
        }
    }
    harness.output().shapes.iter().map(|clipped| meshes(&clipped.shape)).sum()
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

    // Select the sensor and the filter, with Shift held. That was a mistake: a click beside the
    // cards lets go of them all at once, and then Ctrl+G has nothing to group.
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);
    let select = |harness: &mut Bench<'_>| {
        for card in ["imu", "smoother"] {
            let name = header(harness, card);
            shift_click(harness, name);
        }
    };
    select(&mut harness);
    click(&mut harness, empty, PointerButton::Primary);
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &Groups::new());
    // So does a click there with the command key held.
    select(&mut harness);
    click_holding(&mut harness, empty, Modifiers::COMMAND);
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &Groups::new());

    // With that key held, a click on one of the cards lets go of that one alone.
    select(&mut harness);
    let sensor = header(&harness, "imu");
    click_holding(&mut harness, sensor, Modifiers::COMMAND);
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&smoother, "group1")]));
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    assert_eq!(harness.state().groups(), &Groups::new());

    // Selected again, Ctrl+G makes a group of them, drawn as one card.
    select(&mut harness);
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
    let inside = sorted(["//out/sensing".to_owned(), imu.clone(), smoother.clone()]);
    assert_eq!(cards(&harness), inside);
    assert!(shows(&harness, "Group Output") && shows(&harness, "inside sensing") && shows(&harness, "Exit Group"));
    picture(&mut harness, "group-inside.png");

    // A group can be inside another. In here, click the filter and press Ctrl+G: it is a group
    // of its own, inside the first, and is drawn as one card beside the sensor.
    let filter = header(&harness, "smoother");
    click(&mut harness, filter, PointerButton::Primary);
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing"), (&smoother, "sensing/group1")]));
    assert_eq!(cards(&harness), sorted(["//out/sensing".to_owned(), "/sensing/group1".to_owned(), imu.clone()]));
    // Tab steps into the group that is selected, which is the new one, and shows where the graph is.
    harness.key_press(Key::Tab);
    harness.run_steps(4);
    assert_eq!(harness.state().inside(), Some("sensing/group1"));
    assert!(shows(&harness, "inside sensing/group1") && shows(&harness, "Group Input"));
    // In there, P moves the filter one group out: into the group around this one, not out of every
    // group. With nothing left in it the inner group is no more, and the graph shows the one around it.
    let filter = header(&harness, "smoother");
    click(&mut harness, filter, PointerButton::Primary);
    key(&mut harness, Modifiers::NONE, Key::P);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing"), (&smoother, "sensing")]));
    assert_eq!(harness.state().inside(), Some("sensing"));
    assert_eq!(cards(&harness), inside);

    // P once more moves the filter out of this group too. The sensor is what is left in it.
    let filter = header(&harness, "smoother");
    click(&mut harness, filter, PointerButton::Primary);
    key(&mut harness, Modifiers::NONE, Key::P);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing")]));
    assert_eq!(cards(&harness), sorted(["//out/sensing".to_owned(), imu.clone()]));

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

    // A group goes into a group as a node does: select the group and the filter, and press Ctrl+G.
    for card in ["sensing", "smoother"] {
        let name = header(&harness, card);
        shift_click(&mut harness, name);
    }
    key(&mut harness, Modifiers::CTRL, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&imu, "group1/sensing"), (&smoother, "group1")]));
    assert_eq!(cards(&harness), sorted(["/group1".to_owned(), viewer.clone()]));
    assert!(shows(&harness, "a group of 2 nodes"));
    // Ctrl+Alt+G takes the new group apart. What was in it is where the group was: the filter
    // in no group, and the group that was inside it inside no other.
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&imu, "sensing")]));
    assert_eq!(cards(&harness), sorted(["/sensing".to_owned(), smoother.clone(), viewer.clone()]));

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

#[test]
fn a_node_is_put_into_a_group_that_is_there_and_a_node_made_inside_a_group_is_in_it() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (imu, smoother, viewer) = (format!("{robot}/imu"), format!("{robot}/smoother"), format!("{desk}/viewer"));
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);

    // The details are closed. A click on a card opens them, about that card.
    switch(&harness, "Details").click();
    harness.run_steps(3);
    assert!(bar(&harness, "Details").is_none());
    let sensor = header(&harness, "imu");
    click(&mut harness, sensor, PointerButton::Primary);
    harness.run_steps(3);
    assert!(bar(&harness, "Details").is_some() && shows(&harness, &format!("{robot}, process {}", std::process::id())));

    // The sensor is a group by itself: the details of a card have a button for that, as well as the key.
    harness.get_by_label("Make Group").click();
    harness.run_steps(4);
    assert_eq!(harness.state().groups(), &groups([(&imu, "group1")]));
    // The filter is put into the group that is there: select it, select the group last, and press Ctrl+Shift+G.
    for card in ["smoother", "group1"] {
        let name = header(&harness, card);
        shift_click(&mut harness, name);
    }
    key(&mut harness, Modifiers::CTRL | Modifiers::SHIFT, Key::G);
    assert_eq!(harness.state().groups(), &groups([(&imu, "group1"), (&smoother, "group1")]));
    assert_eq!(cards(&harness), sorted(["/group1".to_owned(), viewer.clone()]));
    // Its nodes run in one runtime, and its header is tinted with that one's colour.
    assert_eq!(fading(&harness), 0);
    // The viewer is put into it from its details, which list the groups it can be moved into.
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);
    click(&mut harness, empty, PointerButton::Primary);
    let card = header(&harness, "viewer");
    click(&mut harness, card, PointerButton::Primary);
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(3);
    harness.get_all_by_label("group1").last().expect("the group, among those it can be moved into").click();
    harness.run_steps(4);
    assert_eq!(harness.state().groups().get(&viewer), Some(&"group1".to_owned()));
    assert_eq!(cards(&harness), ["/group1"]);
    // Its nodes run in two runtimes now: its header fades from the colour of one to that of the other.
    assert_eq!(fading(&harness), 1);
    picture(&mut harness, "group-two-runtimes.png");

    // Inside the group, a node is created. It is in the group, and the form stays for another.
    let card = header(&harness, "group1");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("group1"));
    create(&mut harness);
    harness.run_steps(3);
    let created = format!("{robot}/filter1");
    assert!(wait_for(|| scene.robot.inventory().nodes.iter().any(|node| node.name == "filter1")));
    assert!(until(&mut harness, |harness| cards(harness).contains(&created)), "its card is inside the group");
    assert_eq!(harness.state().groups().get(&created), Some(&"group1".to_owned()));
    // Nothing is connected to its ports: they lead nowhere yet, and are not on the cards for the outside.
    assert!(shows(&harness, "viewer.image: u8[]") && !shows(&harness, "filter1.raw: Pose"));
    // From outside, the card of the group offers them, to be wired there.
    // The keys are the graph's while the pointer is over it.
    harness.hover_at(empty);
    harness.run_steps(2);
    key(&mut harness, Modifiers::CTRL, Key::Tab);
    assert_eq!(harness.state().inside(), None);
    assert!(shows(&harness, "filter1.raw: Pose") && shows(&harness, "filter1.smooth: Pose"));

    // The node is removed, and out here one of the same name is created. It is where it was
    // created: beside the group, not in the group that the node before it was in.
    triq::Remote::open(&robot)?.remove("filter1")?;
    assert!(until(&mut harness, |harness| !shows(harness, "filter1.raw: Pose")));
    create(&mut harness);
    harness.run_steps(3);
    assert!(until(&mut harness, |harness| cards(harness).contains(&created)), "{:?}", cards(&harness));
    assert_eq!(harness.state().groups().get(&created), None);
    Ok(())
}
