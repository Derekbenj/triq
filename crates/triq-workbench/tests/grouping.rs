//! Groups in the workbench: made, looked into and taken apart with the keys
//! that Blender has for it, and held back with the rest while drafting.
//! Which group a node is in is its runtime's to say: the workbench asks for
//! it, and a moment later the runtime reports it.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use serde_json::json;
use triq::{Group, Placement};
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, Key, Modifiers, PointerButton};
use triq_workbench::feed::Request;
use triq_workbench::graph::{self, Card, Groups, Kind};
use window::*;

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// The cards on the graph that are this test's, by what they are called: those of its nodes,
/// those of groups that hold one of them, and inside a group those for the outside, if a port
/// of one of its nodes is on them. Other programs may be running, with nodes and groups of their own.
fn cards(harness: &Bench<'_>) -> Vec<String> {
    let own = format!("w{}-", std::process::id());
    let ours = |card: &&Card| match &card.kind {
        Kind::Node { runtime, .. } => runtime.starts_with(&own),
        Kind::Group { members, .. } => members.iter().any(|(runtime, _)| runtime.starts_with(&own)),
        Kind::Entry { .. } | Kind::Exit { .. } => {
            card.left.iter().chain(&card.right).any(|pin| pin.runtime.starts_with(&own))
        }
    };
    let mut cards: Vec<String> = harness.state().graph().nodes().filter(ours).map(Card::key).collect();
    cards.sort();
    cards
}

/// What the next group to be made will be called, inside `inside` or outside every group:
/// the first of `group1`, `group2`, ... that is free. Other programs may have groups already.
fn fresh(harness: &Bench<'_>, inside: Option<&str>) -> String {
    let taken = |name: &String| {
        let path = graph::join(inside, name);
        harness.state().groups().values().any(|of| graph::within(of, &path))
    };
    (1..).map(|number| format!("group{number}")).find(|name| !taken(name)).expect("a name that is free")
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

/// Which group each node of this test is in, by what the runtimes report.
/// Other programs may be running, with groups of their own.
fn grouped(harness: &Bench<'_>) -> Groups {
    let own = format!("w{}-", std::process::id());
    let groups = harness.state().groups().iter().filter(|(node, _)| node.starts_with(&own));
    groups.map(|(node, group)| (node.clone(), group.clone())).collect()
}

/// Wait until the nodes of this test are grouped so: a runtime that is asked
/// to put a node in a group reports it a moment later.
#[track_caller]
fn becomes<const N: usize>(harness: &mut Bench<'_>, members: [(&String, &str); N]) {
    let expected = groups(members);
    until(harness, |harness| grouped(harness) == expected);
    assert_eq!(grouped(harness), expected);
}

/// Give the runtimes time to do what they should not have been asked for, and see that they did not.
#[track_caller]
fn stays<const N: usize>(harness: &mut Bench<'_>, members: [(&String, &str); N]) {
    std::thread::sleep(std::time::Duration::from_millis(150));
    harness.run_steps(3);
    assert_eq!(grouped(harness), groups(members));
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
    stays(&mut harness, []);
    // So does a click there with the command key held.
    select(&mut harness);
    click_holding(&mut harness, empty, Modifiers::COMMAND);
    key(&mut harness, Modifiers::CTRL, Key::G);
    stays(&mut harness, []);

    // With that key held, a click on one of the cards lets go of that one alone.
    select(&mut harness);
    let sensor = header(&harness, "imu");
    click_holding(&mut harness, sensor, Modifiers::COMMAND);
    let made = fresh(&harness, None);
    key(&mut harness, Modifiers::CTRL, Key::G);
    becomes(&mut harness, [(&smoother, &made)]);
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    becomes(&mut harness, []);

    // Selected again, Ctrl+G makes a group of them, drawn as one card.
    select(&mut harness);
    key(&mut harness, Modifiers::CTRL, Key::G);
    becomes(&mut harness, [(&imu, &made), (&smoother, &made)]);
    assert_eq!(cards(&harness), sorted([format!("/{made}"), viewer.clone()]));
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
    becomes(&mut harness, [(&imu, "sensing"), (&smoother, "sensing")]);
    assert_eq!(cards(&harness), sorted(["/sensing".to_owned(), viewer.clone()]));
    // The details are still about the group, under its new name.
    harness.run_steps(3);
    assert!(shows(&harness, "a group of 2 nodes"));
    picture(&mut harness, "group.png");

    // Click its card. With the pointer over the graph, Tab shows the inside of the group that is
    // selected: its nodes, and a card that stands for the outside, with the viewer's input that
    // the filter's output goes to.
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
    becomes(&mut harness, [(&imu, "sensing"), (&smoother, "sensing/group1")]);
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
    becomes(&mut harness, [(&imu, "sensing"), (&smoother, "sensing")]);
    assert!(until(&mut harness, |harness| harness.state().inside() == Some("sensing")));
    assert_eq!(cards(&harness), inside);

    // P once more moves the filter out of this group too. The sensor is what is left in it.
    let filter = header(&harness, "smoother");
    click(&mut harness, filter, PointerButton::Primary);
    key(&mut harness, Modifiers::NONE, Key::P);
    becomes(&mut harness, [(&imu, "sensing")]);
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
    becomes(&mut harness, [(&imu, &format!("{made}/sensing")), (&smoother, &made)]);
    assert_eq!(cards(&harness), sorted([format!("/{made}"), viewer.clone()]));
    assert!(shows(&harness, "a group of 2 nodes"));
    // Ctrl+Alt+G takes the new group apart. What was in it is where the group was: the filter
    // in no group, and the group that was inside it inside no other.
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    becomes(&mut harness, [(&imu, "sensing")]);
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

    // Another workbench draws the same groups, with nothing handed to it: the nodes say which group they are in.
    let mut next = open()?;
    assert!(until(&mut next, |next| cards(next) == sorted(["/sensing".to_owned(), smoother.clone(), viewer.clone()])));
    drop(next);

    // The group that was left is the one that is selected: Ctrl+Alt+G takes it apart, and its nodes are cards again.
    let card = header(&harness, "sensing");
    harness.hover_at(card);
    harness.run_steps(2);
    key(&mut harness, Modifiers::CTRL | Modifiers::ALT, Key::G);
    becomes(&mut harness, []);
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
    let made = fresh(&harness, None);
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    group(&mut harness);
    assert_eq!(cards(&harness), sorted([format!("/{made}"), viewer.clone()]));
    assert!(shows(&harness, "to be created") && shows(&harness, "Draft: 2 changes"));
    // But they are not grouped yet: no runtime was asked. The draft lists what it holds back, in order.
    stays(&mut harness, []);
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);
    click(&mut harness, empty, PointerButton::Primary);
    for (number, node) in [(1, &imu), (2, &smoother)] {
        assert!(shows(&harness, &format!("{number}. put {node} in group {made}")));
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
    becomes(&mut harness, [(&imu, &made), (&smoother, &made)]);
    assert_eq!(cards(&harness), sorted([format!("/{made}"), viewer]));
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
    let made = fresh(&harness, None);
    harness.get_by_label("Make Group").click();
    harness.run_steps(4);
    becomes(&mut harness, [(&imu, &made)]);
    // The filter is put into the group that is there: select it, select the group last, and press Ctrl+Shift+G.
    for card in ["smoother", made.as_str()] {
        let name = header(&harness, card);
        shift_click(&mut harness, name);
    }
    key(&mut harness, Modifiers::CTRL | Modifiers::SHIFT, Key::G);
    becomes(&mut harness, [(&imu, &made), (&smoother, &made)]);
    assert_eq!(cards(&harness), sorted([format!("/{made}"), viewer.clone()]));
    // Its nodes run in one runtime, and its header is tinted with that one's colour.
    assert_eq!(fading(&harness), 0);
    // The viewer is put into it from its details, which list the groups it can be moved into.
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);
    click(&mut harness, empty, PointerButton::Primary);
    let card = header(&harness, "viewer");
    click(&mut harness, card, PointerButton::Primary);
    // The list of numbers beside the charts has a box of the same kind: this is the one in the details.
    let groups = harness.query_all_by_role(Role::ComboBox).find(|list| list.value().as_deref() == Some("Move into"));
    groups.expect("the groups it can be moved into").click();
    harness.run_steps(3);
    harness.get_all_by_label(&made).last().expect("the group, among those it can be moved into").click();
    harness.run_steps(4);
    becomes(&mut harness, [(&imu, &made), (&smoother, &made), (&viewer, &made)]);
    assert_eq!(cards(&harness), [format!("/{made}")]);
    // Its nodes run in two runtimes now: its header fades from the colour of one to that of the other.
    assert_eq!(fading(&harness), 1);
    picture(&mut harness, "group-two-runtimes.png");

    // Inside the group, a node is created. It is in the group, and the form stays for another.
    let card = header(&harness, &made);
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some(made.as_str()));
    create(&mut harness, "filter");
    let created = format!("{robot}/filter1");
    assert!(wait_for(|| scene.robot.inventory().nodes.iter().any(|node| node.name == "filter1")));
    assert!(until(&mut harness, |harness| cards(harness).contains(&created)), "its card is inside the group");
    assert_eq!(grouped(&harness).get(&created), Some(&made));
    // Nothing is connected to its ports yet. In here they are pins of its own card, and of no other.
    assert!(!shows(&harness, "filter1.raw: Pose"));
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
    create(&mut harness, "filter");
    assert!(until(&mut harness, |harness| cards(harness).contains(&created)), "{:?}", cards(&harness));
    assert_eq!(grouped(&harness).get(&created), None);
    Ok(())
}

#[test]
fn a_group_that_a_runtime_offers_as_a_kind_is_created_whole() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    // The robot offers two filters, one after the other, as one kind.
    let pair = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .spawn("filter", "fine", Placement::pool("dsp"), json!({ "alpha": 0.1 }))
        .connect("coarse/smooth", "fine/raw");
    scene.robot.register_group("pair", pair)?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    let node = |group: &str, node: &str| format!("{robot}/{group}.{node}");

    // It is among the kinds of the robot, and is asked for as a node is. The form says what it is made of.
    create(&mut harness, "pair");
    assert!(
        shows(&harness, "Create a pair group")
            && shows(&harness, "coarse, a filter")
            && shows(&harness, "fine, a filter")
    );
    // Its nodes are named after it, are in a group of its name, and are drawn as one card.
    let (coarse, fine) = (node("pair1", "coarse"), node("pair1", "fine"));
    becomes(&mut harness, [(&coarse, "pair1"), (&fine, "pair1")]);
    assert!(cards(&harness).contains(&"/pair1".to_owned()) && !cards(&harness).contains(&coarse));
    // The wire between the two is inside the group: its card shows the ends that lead nowhere yet.
    assert!(shows(&harness, "pair1.coarse.raw: Pose") && shows(&harness, "pair1.fine.smooth: Pose"));
    assert!(!shows(&harness, "pair1.fine.raw: Pose"));

    // Created while the graph shows the inside of a group, another is a group inside that one.
    // The graph draws whatever else runs on the machine as well: it is tidied and shown whole
    // first, so that the card is in view and under no other.
    for button in ["Tidy", "Fit"] {
        harness.get_by_label(button).click();
        harness.run_steps(4);
    }
    let card = header(&harness, "pair1");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("pair1"));
    create(&mut harness, "pair");
    let inner = [node("pair2", "coarse"), node("pair2", "fine")];
    becomes(
        &mut harness,
        [(&coarse, "pair1"), (&fine, "pair1"), (&inner[0], "pair1/pair2"), (&inner[1], "pair1/pair2")],
    );
    assert!(cards(&harness).contains(&"/pair1/pair2".to_owned()));

    // In a draft, one more is held back whole, and drawn as it would be: a group that is to be created.
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    create(&mut harness, "pair");
    assert!(cards(&harness).contains(&"/pair1/pair3".to_owned()), "{:?}", cards(&harness));
    assert!(shows(&harness, "Draft: 3 changes") && shows(&harness, "to be created"));
    assert!(scene.robot.inventory().nodes.iter().all(|node| !node.name.starts_with("pair3")), "nothing was created");
    harness.get_by_label("Discard draft").click();
    harness.run_steps(4);
    assert!(!cards(&harness).contains(&"/pair1/pair3".to_owned()));
    Ok(())
}

#[test]
fn a_kind_with_a_group_inside_it_is_created_with_that_group_inside() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    // The robot offers a filter, and after it another in a group inside the group, as one kind.
    let nest = Group::new()
        .spawn("filter", "coarse", Placement::Dedicated, json!({ "alpha": 0.9 }))
        .group("deep", Group::new().spawn("filter", "fine", Placement::Dedicated, json!({ "alpha": 0.1 })))
        .connect("coarse/smooth", "deep.fine/raw");
    scene.robot.register_group("nest", nest)?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);
    let node = |group: &str, node: &str| format!("{robot}/{group}.{node}");

    // The form says what it is made of, and its nodes are in the group and in the group inside it.
    create(&mut harness, "nest");
    assert!(shows(&harness, "coarse, a filter") && shows(&harness, "deep.fine, a filter"));
    let (coarse, fine) = (node("nest1", "coarse"), node("nest1", "deep.fine"));
    becomes(&mut harness, [(&coarse, "nest1"), (&fine, "nest1/deep")]);
    assert!(cards(&harness).contains(&"/nest1".to_owned()) && !cards(&harness).contains(&"/nest1/deep".to_owned()));

    // Created while the graph shows the inside of a group, another is inside that one, and the
    // group inside it is still inside it.
    for button in ["Tidy", "Fit"] {
        harness.get_by_label(button).click();
        harness.run_steps(4);
    }
    let card = header(&harness, "nest1");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("nest1"));
    assert!(cards(&harness).contains(&"/nest1/deep".to_owned()), "{:?}", cards(&harness));
    picture(&mut harness, "group-kind-inside.png");
    create(&mut harness, "nest");
    let inner = [node("nest2", "coarse"), node("nest2", "deep.fine")];
    becomes(
        &mut harness,
        [(&coarse, "nest1"), (&fine, "nest1/deep"), (&inner[0], "nest1/nest2"), (&inner[1], "nest1/nest2/deep")],
    );
    Ok(())
}

#[test]
fn a_wire_is_cut_with_a_right_click_wherever_it_is_drawn() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // The sensor and the filter are one group. The viewer, which hears the filter, is another.
    scene.robot.set_group("imu", "sensing")?;
    scene.robot.set_group("smoother", "sensing")?;
    scene.desk.set_group("viewer", "looking")?;
    let cut = Request::Disconnect { from: format!("{robot}/smoother/smooth"), to: format!("{desk}/viewer/pose") };
    let mut harness = open()?;
    let outside = sorted(["/looking".to_owned(), "/sensing".to_owned()]);
    assert!(until(&mut harness, |harness| cards(harness) == outside), "{:?}", cards(&harness));
    harness.run_steps(4);
    // What is cut is held back in a draft, to be seen at once. Each time, the draft is begun afresh.
    let draft = |harness: &mut Bench<'_>| {
        if shows(harness, "Discard draft") {
            harness.get_by_label("Discard draft").click();
            harness.run_steps(3);
        }
        harness.get_by_label("Draft").click();
        harness.run_steps(3);
    };
    let right_click = |harness: &mut Bench<'_>, at: egui::Pos2, what: &str| {
        draft(harness);
        click(harness, at, PointerButton::Secondary);
        harness.run_steps(2);
        assert_eq!(harness.state().drafted(), std::slice::from_ref(&cut), "a right click on {what}");
    };
    let halfway = |from: egui::Pos2, to: egui::Pos2| from + (to - from) / 2.0;

    // From outside, the wire runs from the card of one group to the card of the other.
    let (from, to) = (pin(&harness, "smoother.smooth: Pose", true), pin(&harness, "viewer.pose: Pose", false));
    right_click(&mut harness, halfway(from, to), "the wire between two groups");
    right_click(&mut harness, to, "the pin it arrives at");
    right_click(&mut harness, from, "the pin it leaves");

    // Inside the group that sends, the viewer's input is on the card that stands for the outside,
    // and the wire runs from the filter to it: the same connection, cut in the same way.
    let card = header(&harness, "sensing");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("sensing"));
    let (from, to) = (pin(&harness, "smooth: Pose", true), pin(&harness, "viewer.pose: Pose", false));
    right_click(&mut harness, halfway(from, to), "the wire to Group Output");
    right_click(&mut harness, to, "the viewer's pin on Group Output");
    right_click(&mut harness, from, "the pin of the filter");
    harness.get_all_by_label("Exit Group").next().expect("the way out").click();
    harness.run_steps(4);

    // Inside the group that hears, the filter's output is on the other such card.
    let card = header(&harness, "looking");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("looking"));
    let (from, to) = (pin(&harness, "smoother.smooth: Pose", true), pin(&harness, "pose: Pose", false));
    right_click(&mut harness, halfway(from, to), "the wire from Group Input");
    right_click(&mut harness, from, "the filter's pin on Group Input");
    right_click(&mut harness, to, "the pin of the viewer");
    Ok(())
}

#[test]
fn what_is_outside_a_group_is_wired_from_inside_it() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // The sensor and the filter are one group. Outside it are the viewer, which hears the filter,
    // and a second filter that nothing is connected to.
    scene.robot.spawn_kind("filter", "spare", Placement::Dedicated, json!(null))?;
    scene.robot.set_group("imu", "sensing")?;
    scene.robot.set_group("smoother", "sensing")?;
    let port = |runtime: &str, port: &str| format!("{runtime}/{port}");
    let connect = |from: &str, to: &str| Request::Connect { from: port(&robot, from), to: port(&robot, to) };
    let mut harness = open()?;
    let outside = sorted(["/sensing".to_owned(), format!("{robot}/spare"), format!("{desk}/viewer")]);
    assert!(until(&mut harness, |harness| cards(harness) == outside), "{:?}", cards(&harness));
    harness.run_steps(4);
    harness.get_by_label("Draft").click();
    harness.run_steps(3);

    // Inside the group, the two cards for the outside have the ports out there: the viewer's input
    // that hears the filter, and the ports of the second filter, which are free to be wired.
    let card = header(&harness, "sensing");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("sensing"));
    let inside =
        ["//in/sensing".to_owned(), "//out/sensing".to_owned(), format!("{robot}/imu"), format!("{robot}/smoother")];
    assert_eq!(cards(&harness), sorted(inside));
    for free in ["spare.raw: Pose", "spare.smooth: Pose", "viewer.pose: Pose"] {
        assert!(shows(&harness, free), "{free:?} is on a card for the outside");
    }
    // A pin there says in full whose it is.
    let written = seen(&harness, &text(&harness, "spare.raw: Pose"));
    harness.hover_at(written);
    harness.run_steps(6);
    assert!(harness.query_by_label_contains(&format!("{robot}/spare/raw\n")).is_some(), "the pin says whose it is");

    // The viewer is to hear the sensor instead of the filter: cut the one wire, and draw the other
    // to the same pin. The pin is the viewer's input, and stays where it is.
    let (filter, viewer) = (pin(&harness, "smooth: Pose", true), pin(&harness, "viewer.pose: Pose", false));
    click(&mut harness, filter + (viewer - filter) / 2.0, PointerButton::Secondary);
    let sensor = pin(&harness, "pose: Pose", true);
    drag(&mut harness, sensor, viewer);
    harness.run_steps(3);
    let cut = Request::Disconnect { from: port(&robot, "smoother/smooth"), to: port(&desk, "viewer/pose") };
    let join = Request::Connect { from: port(&robot, "imu/pose"), to: port(&desk, "viewer/pose") };
    assert_eq!(harness.state().drafted(), [cut.clone(), join.clone()]);
    // The second filter is to hear the sensor too, and the first is to hear the second: a wire to
    // its input on Group Output, and one from its output on Group Input.
    let (sensor, spare) = (pin(&harness, "pose: Pose", true), pin(&harness, "spare.raw: Pose", false));
    drag(&mut harness, sensor, spare);
    let (spare, filter) = (pin(&harness, "spare.smooth: Pose", true), pin(&harness, "raw: Pose", false));
    drag(&mut harness, spare, filter);
    harness.run_steps(3);
    let drafted = [cut, join, connect("imu/pose", "spare/raw"), connect("spare/smooth", "smoother/raw")];
    assert_eq!(harness.state().drafted(), drafted);
    harness.remove_cursor();
    harness.run_steps(3);
    picture(&mut harness, "draft-inside-group.png");

    // A wire from one of the two cards to the other would pass every node of the group by: it is refused.
    harness.get_by_label("Discard draft").click();
    harness.run_steps(3);
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    let (from, to) = (pin(&harness, "spare.smooth: Pose", true), pin(&harness, "spare.raw: Pose", false));
    drag(&mut harness, from, to);
    harness.run_steps(3);
    assert_eq!(harness.state().drafted(), []);
    assert!(harness.query_by_label_contains("begins or ends at one of its nodes").is_some());
    Ok(())
}

#[test]
fn a_port_that_a_draft_frees_inside_a_group_is_offered_on_the_card_of_the_group() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    scene.robot.set_group("imu", "sensing")?;
    scene.robot.set_group("smoother", "sensing")?;
    let mut harness = open()?;
    let outside = sorted(["/sensing".to_owned(), format!("{desk}/viewer")]);
    assert!(until(&mut harness, |harness| cards(harness) == outside), "{:?}", cards(&harness));
    harness.run_steps(4);
    // The wire from the sensor to the filter is inside the group: its ends are not on the group's card.
    assert!(shows(&harness, "smoother.smooth: Pose") && !shows(&harness, "imu.pose: Pose"));

    // In a draft, that wire is cut, inside the group.
    harness.get_by_label("Draft").click();
    harness.run_steps(3);
    let card = header(&harness, "sensing");
    double_click(&mut harness, card);
    assert_eq!(harness.state().inside(), Some("sensing"));
    let (from, to) = (pin(&harness, "pose: Pose", true), pin(&harness, "raw: Pose", false));
    click(&mut harness, from + (to - from) / 2.0, PointerButton::Secondary);
    let cut = Request::Disconnect { from: format!("{robot}/imu/pose"), to: format!("{robot}/smoother/raw") };
    assert_eq!(harness.state().drafted(), std::slice::from_ref(&cut));
    harness.get_all_by_label("Exit Group").next().expect("the way out").click();
    harness.run_steps(4);

    // Outside, the card of the group offers both ends already, as ports that nothing is connected
    // to: they will be so once the draft is applied. They are wired there in the same draft.
    assert!(shows(&harness, "imu.pose: Pose") && shows(&harness, "smoother.raw: Pose"));
    // With two more pins the card is wider than it was, and reaches under the viewer's beside it.
    harness.get_by_label("Tidy").click();
    harness.run_steps(4);
    let (sensor, viewer) = (pin(&harness, "imu.pose: Pose", true), pin(&harness, "pose: Pose", false));
    drag(&mut harness, sensor, viewer);
    harness.run_steps(3);
    let join = Request::Connect { from: format!("{robot}/imu/pose"), to: format!("{desk}/viewer/pose") };
    assert_eq!(harness.state().drafted(), [cut, join]);
    harness.remove_cursor();
    harness.run_steps(3);
    picture(&mut harness, "draft-freed-ports.png");
    Ok(())
}
