//! Nodes put together in a group: drawn as one card from outside, and from
//! inside as its nodes between two cards that stand for the outside, with the
//! ports out there on them.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use triq::Remote;
use triq::prelude::*;
use triq_workbench::graph::{self, Card, Cut, Groups, Kind, Places};
use triq_workbench::snarl::Snarl;

fn keys(cards: &[Card]) -> Vec<String> {
    let mut keys: Vec<String> = cards.iter().map(Card::key).collect();
    keys.sort();
    keys
}

fn sorted<const N: usize>(mut names: [String; N]) -> Vec<String> {
    names.sort();
    names.to_vec()
}

fn called<'a>(cards: &'a [Card], key: &str) -> &'a Card {
    cards.iter().find(|card| card.key() == key).unwrap_or_else(|| panic!("no card {key}"))
}

/// What is written beside the pins of a card, down its left side and down its right.
fn written(card: &Card) -> [Vec<String>; 2] {
    [&card.left, &card.right].map(|pins| pins.iter().map(|pin| card.written(pin)).collect())
}

/// The wires of a drawing, as the card and the port at each end.
fn wires(snarl: &Snarl<Card>) -> Vec<String> {
    let end = |card: &Card, port: String| format!("{} {}", card.name(), port.rsplit('/').next().unwrap_or_default());
    let wires = snarl.wires().map(|(from, to)| {
        let (source, sink) = (&snarl[from.node], &snarl[to.node]);
        format!("{} -> {}", end(source, source.output(from.output)), end(sink, sink.input(to.input)))
    });
    let mut wires: Vec<String> = wires.collect();
    wires.sort();
    wires
}

fn groups<const N: usize>(members: [(String, &str); N]) -> Groups {
    members.into_iter().map(|(node, group)| (node, group.to_owned())).collect()
}

/// The camera that the viewer waits for.
#[derive(Ports)]
#[allow(dead_code)]
struct CameraPorts {
    image: Output<[u8]>,
}

#[test]
fn which_group_a_node_is_in_is_what_its_runtime_reports() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    assert_eq!(graph::groups(&scene.inventories()), Groups::new());

    // Each program says which group its nodes are in. A node that is in none is not listed.
    scene.robot.set_group("imu", "sensing")?;
    scene.desk.set_group("viewer", "sensing/looking")?;
    let reported = graph::groups(&scene.inventories());
    assert_eq!(reported, groups([(format!("{robot}/imu"), "sensing"), (format!("{desk}/viewer"), "sensing/looking")]));
    // That is what is drawn: the group as one card, beside the filter.
    let cards = graph::cards(&scene.inventories(), &reported, None);
    assert_eq!(keys(&cards), sorted([format!("{robot}/smoother"), "/sensing".to_owned()]));
    Ok(())
}

#[test]
fn a_group_is_drawn_as_one_card_with_the_ports_that_lead_outside_it() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // The sensor and the filter it feeds are one group. The viewer hears the filter from outside.
    let sensing = groups([(format!("{robot}/imu"), "sensing"), (format!("{robot}/smoother"), "sensing")]);
    let cards = graph::cards(&scene.inventories(), &sensing, None);
    assert_eq!(keys(&cards), sorted([format!("{desk}/viewer"), "/sensing".to_owned()]));
    let group = called(&cards, "/sensing");
    let Kind::Group { path, members } = &group.kind else { panic!("the card of a group") };
    assert_eq!((path.as_str(), members.len(), group.runtime()), ("sensing", 2, Some(robot.as_str())));
    // The wire from the sensor to the filter is the group's own affair: only what leads outside is a pin.
    assert_eq!(written(group), [vec![], vec!["smoother.smooth: Pose".to_owned()]]);
    assert_eq!(group.output(0), format!("{robot}/smoother/smooth"));
    let mut snarl = Snarl::new();
    graph::show(&mut snarl, cards, &mut Places::new(), 0);
    assert_eq!(wires(&snarl), ["sensing smooth -> viewer pose"]);

    // A port that nothing is connected to is a pin as well, to be wired from outside.
    scene.robot.spawn_kind("filter", "spare", Placement::Dedicated, serde_json::json!(null))?;
    let mut with_spare = sensing.clone();
    with_spare.insert(format!("{robot}/spare"), "sensing".to_owned());
    let cards = graph::cards(&scene.inventories(), &with_spare, None);
    let pins = written(called(&cards, "/sensing"));
    assert_eq!(pins[0], ["spare.raw: Pose"]);
    assert_eq!(pins[1], ["smoother.smooth: Pose", "spare.smooth: Pose"]);

    // So is a port that is connected inside the group and outside it.
    scene.desk.connect(&format!("{robot}/imu/pose"), "viewer/pose")?;
    let cards = graph::cards(&scene.inventories(), &sensing, None);
    assert_eq!(written(called(&cards, "/sensing"))[1], ["imu.pose: Pose", "smoother.smooth: Pose"]);
    graph::show(&mut snarl, cards, &mut Places::new(), 0);
    assert_eq!(wires(&snarl), ["sensing pose -> viewer pose", "sensing smooth -> viewer pose"]);

    // A group of nodes from two runtimes is no one runtime's.
    let mixed = groups([(format!("{robot}/smoother"), "both"), (format!("{desk}/viewer"), "both")]);
    let cards = graph::cards(&scene.inventories(), &mixed, None);
    let both = called(&cards, "/both");
    assert_eq!(both.runtime(), None);
    // The viewer's input for images waits for an output that does not exist: it leads outside the group.
    let mut heard = written(both)[0].clone();
    heard.sort();
    assert_eq!(heard, ["smoother.raw: Pose", "viewer.image: u8[]", "viewer.pose: Pose"]);
    // A group none of whose nodes is running is not drawn.
    let gone = groups([("nobody/at-all".to_owned(), "empty")]);
    assert!(graph::cards(&scene.inventories(), &gone, None).iter().all(|card| card.node().is_some()));
    Ok(())
}

#[test]
fn inside_a_group_two_more_cards_stand_for_the_outside() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (mut snarl, mut places) = (Snarl::new(), Places::new());
    graph::show(&mut snarl, graph::cards(&scene.inventories(), &Groups::new(), None), &mut places, 1);
    let place =
        |snarl: &Snarl<Card>, key: &str| snarl.nodes_pos().find(|(_, card)| card.key() == key).map(|found| found.0);
    let outside = place(&snarl, &format!("{robot}/smoother")).expect("the filter's card");

    // The filter is a group by itself. Inside it, the filter's card is between two others:
    // what it hears from outside comes from the one, and what it sends there goes to the other.
    let filtering = groups([(format!("{robot}/smoother"), "filtering")]);
    let cards = graph::cards(&scene.inventories(), &filtering, Some("filtering"));
    assert_eq!(
        keys(&cards),
        sorted(["//in/filtering".to_owned(), "//out/filtering".to_owned(), format!("{robot}/smoother")])
    );
    let (entry, exit) = (called(&cards, "//in/filtering"), called(&cards, "//out/filtering"));
    assert_eq!((entry.name(), exit.name()), ("Group Input", "Group Output"));
    // On them are the ports out there, as a group's ports are on its card: the output that the
    // filter hears, and the input that hears the filter. The viewer's other input hears nothing.
    assert_eq!(written(entry), [vec![], vec!["imu.pose: Pose".to_owned()]]);
    assert_eq!(written(exit), [vec!["viewer.pose: Pose".to_owned(), "viewer.image: u8[]".to_owned()], vec![]]);
    assert_eq!((entry.output(0), exit.input(0)), (format!("{robot}/imu/pose"), format!("{desk}/viewer/pose")));
    graph::show(&mut snarl, cards, &mut places, 2);
    // So every wire in here is a connection, as it is anywhere.
    assert_eq!(wires(&snarl), ["Group Input pose -> smoother raw", "smoother smooth -> Group Output pose"]);
    // The filter's card is where it was outside, with the two others to its left and to its right.
    let inside = |key: &str| place(&snarl, key).expect("a card inside the group");
    assert_eq!(inside(&format!("{robot}/smoother")), outside);
    assert!(inside("//in/filtering").x < outside.x && outside.x < inside("//out/filtering").x);

    // A port of the group that nothing is connected to is a pin of its node, in here. From outside,
    // the card of the group offers it, to be wired from there.
    scene.robot.spawn_kind("filter", "spare", Placement::Dedicated, serde_json::json!(null))?;
    let mut with_spare = filtering.clone();
    with_spare.insert(format!("{robot}/spare"), "filtering".to_owned());
    let cards = graph::cards(&scene.inventories(), &with_spare, Some("filtering"));
    assert_eq!(written(called(&cards, "//in/filtering"))[1], ["imu.pose: Pose"]);
    assert_eq!(written(called(&cards, "//out/filtering"))[0], ["viewer.pose: Pose", "viewer.image: u8[]"]);
    let cards = graph::cards(&scene.inventories(), &with_spare, None);
    let offered = written(called(&cards, "/filtering"));
    assert_eq!(offered[0], ["smoother.raw: Pose", "spare.raw: Pose"]);
    assert_eq!(offered[1], ["smoother.smooth: Pose", "spare.smooth: Pose"]);

    // It is the same the other way round. With that node outside the group, its ports that nothing
    // is connected to are on the two cards for the outside, to be wired from in here.
    let cards = graph::cards(&scene.inventories(), &filtering, Some("filtering"));
    let on = |card: &str, side: usize| {
        let mut pins = written(called(&cards, card))[side].clone();
        pins.sort();
        pins
    };
    assert_eq!(on("//in/filtering", 1), ["imu.pose: Pose", "spare.smooth: Pose"]);
    assert_eq!(on("//out/filtering", 0), ["spare.raw: Pose", "viewer.image: u8[]", "viewer.pose: Pose"]);
    // Nothing is wired to them yet.
    graph::show(&mut snarl, cards, &mut places, 2);
    assert_eq!(wires(&snarl), ["Group Input pose -> smoother raw", "smoother smooth -> Group Output pose"]);
    Remote::open(&robot)?.remove("spare")?;

    // Outside again, the group is one card where its node was wired: the sensor feeds it, and it the viewer.
    let cards = graph::cards(&scene.inventories(), &filtering, None);
    assert_eq!(keys(&cards), sorted(["/filtering".to_owned(), format!("{desk}/viewer"), format!("{robot}/imu")]));
    graph::show(&mut snarl, cards, &mut places, 3);
    assert_eq!(wires(&snarl), ["filtering smooth -> viewer pose", "imu pose -> filtering raw"]);
    // Data passes through the card of a group as through any other: the sensor feeds it, and it the viewer.
    let lit: Vec<String> = graph::flow(&snarl, "/filtering").into_iter().collect();
    assert_eq!(lit, sorted(["/filtering".to_owned(), format!("{desk}/viewer"), format!("{robot}/imu")]));

    // A group that is all there is to its own wiring has no outside to stand for.
    let all = groups([
        (format!("{robot}/imu"), "all"),
        (format!("{robot}/smoother"), "all"),
        (format!("{desk}/viewer"), "all"),
    ]);
    let cards = graph::cards(&scene.inventories(), &all, Some("all"));
    // The viewer's input for images waits for an output that does not exist: there is no pin for
    // that out there. The viewer's own pin says what it waits for.
    assert!(cards.iter().all(|card| card.node().is_some()));
    assert_eq!(called(&cards, &format!("{desk}/viewer")).waiting(1), [format!("{robot}/camera/image")]);
    Ok(())
}

#[test]
fn the_ports_around_a_group_are_offered_one_group_out_and_no_further() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    scene.robot.spawn_kind("filter", "spare", Placement::Dedicated, serde_json::json!(null))?;
    scene.robot.spawn_kind("filter", "near", Placement::Dedicated, serde_json::json!(null))?;
    // The filter is a group inside the group that holds the sensor and one of two spare filters.
    let nested = groups([
        (format!("{robot}/imu"), "sensing"),
        (format!("{robot}/near"), "sensing"),
        (format!("{robot}/smoother"), "sensing/filters"),
    ]);
    let free = |inside: &str| {
        let cards = graph::cards(&scene.inventories(), &nested, Some(inside));
        let mut pins = written(called(&cards, &format!("//out/{inside}")))[0].clone();
        pins.sort();
        pins
    };
    // In the inner group, the spare filter beside it is offered. The one outside both is two groups
    // away, and so is the viewer, of which only the input that hears the group is there.
    assert_eq!(free("sensing/filters"), ["near.raw: Pose", "viewer.pose: Pose"]);
    // In the outer group, those are offered: the other filter, and the viewer's input that hears nothing.
    assert_eq!(free("sensing"), ["spare.raw: Pose", "viewer.image: u8[]", "viewer.pose: Pose"]);
    Ok(())
}

#[test]
fn an_input_that_only_waits_is_offered_as_one_that_is_connected_to_nothing() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    // The viewer's input for images waits for the output of a camera that does not exist: it
    // hears nothing. Seen from inside a group beside the viewer it is on Group Output, to be wired.
    let filtering = groups([(format!("{robot}/smoother"), "filtering")]);
    let cards = graph::cards(&scene.inventories(), &filtering, Some("filtering"));
    let exit = called(&cards, "//out/filtering");
    assert_eq!(written(exit)[0], ["viewer.pose: Pose", "viewer.image: u8[]"]);
    // Its pin says what it waits for, there as on the viewer's own card.
    assert_eq!(exit.waiting(1), [format!("{robot}/camera/image")]);

    // The camera starts. Now the input hears an output that is outside the group, which is no
    // affair of the group's: neither end of that wire is on the two cards.
    let _camera: CameraPorts = scene.robot.attach("camera")?;
    let cards = graph::cards(&scene.inventories(), &filtering, Some("filtering"));
    assert_eq!(written(called(&cards, "//out/filtering"))[0], ["viewer.pose: Pose"]);
    assert_eq!(written(called(&cards, "//in/filtering"))[1], ["imu.pose: Pose"]);
    Ok(())
}

#[test]
fn a_port_whose_last_connection_a_draft_cuts_is_offered_as_one_that_is_connected_to_nothing() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let sensing = groups([(format!("{robot}/imu"), "sensing"), (format!("{robot}/smoother"), "sensing")]);
    let pins = |cut: &Cut, inside: Option<&str>, card: &str| {
        let cards = graph::cards_with(&scene.inventories(), &sensing, inside, cut);
        written(called(&cards, card))
    };
    // The wire from the sensor to the filter is inside the group: neither end is on the group's card.
    assert_eq!(pins(&Cut::new(), None, "/sensing"), [vec![], vec!["smoother.smooth: Pose".to_owned()]]);

    // A draft cuts it. It is still drawn, inside, but both its ends will be connected to nothing:
    // the card of the group offers them already, to be wired from outside in the same draft.
    let inner: Cut = [(format!("{robot}/imu/pose"), format!("{robot}/smoother/raw"))].into();
    let offered = pins(&inner, None, "/sensing");
    assert_eq!(offered[0], ["smoother.raw: Pose"]);
    assert_eq!(offered[1], ["imu.pose: Pose", "smoother.smooth: Pose"]);
    let cards = graph::cards_with(&scene.inventories(), &sensing, Some("sensing"), &inner);
    let mut snarl = Snarl::new();
    graph::show(&mut snarl, cards, &mut Places::new(), 0);
    assert!(wires(&snarl).contains(&"imu pose -> smoother raw".to_owned()));

    // The same goes for what is outside a group. With the wire to the viewer cut, the viewer's
    // input is still on Group Output, where its wire is, and stays there as one that is free.
    let outer: Cut = [(format!("{robot}/smoother/smooth"), format!("{desk}/viewer/pose"))].into();
    assert_eq!(pins(&outer, Some("sensing"), "//out/sensing")[0], ["viewer.pose: Pose", "viewer.image: u8[]"]);
    Ok(())
}

#[test]
fn a_group_can_be_inside_another() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let inventories = scene.inventories();
    // The filter is a group of its own, inside the group that also holds the sensor. A node says
    // which group it is in, and if that is inside another, the outer one first.
    let nested = groups([(format!("{robot}/imu"), "sensing"), (format!("{robot}/smoother"), "sensing/filters")]);

    // From outside every group it is all one card. The filter's output is the port that leads out.
    let cards = graph::cards(&inventories, &nested, None);
    assert_eq!(keys(&cards), sorted([format!("{desk}/viewer"), "/sensing".to_owned()]));
    let outer = called(&cards, "/sensing");
    let Kind::Group { path, members } = &outer.kind else { panic!("the card of a group") };
    assert_eq!((path.as_str(), outer.name(), members.len()), ("sensing", "sensing", 2));
    assert_eq!(written(outer), [vec![], vec!["smoother.smooth: Pose".to_owned()]]);

    // Inside the outer group: the sensor, the inner group as one card, and the card for the outside.
    let cards = graph::cards(&inventories, &nested, Some("sensing"));
    assert_eq!(
        keys(&cards),
        sorted(["//out/sensing".to_owned(), "/sensing/filters".to_owned(), format!("{robot}/imu")])
    );
    let inner = called(&cards, "/sensing/filters");
    assert_eq!(inner.name(), "filters");
    assert_eq!(written(inner), [vec!["smoother.raw: Pose".to_owned()], vec!["smoother.smooth: Pose".to_owned()]]);
    let mut snarl = Snarl::new();
    graph::show(&mut snarl, cards, &mut Places::new(), 0);
    assert_eq!(wires(&snarl), ["filters smooth -> Group Output pose", "imu pose -> filters raw"]);

    // Inside the inner group: the filter, between the two cards for what is outside it.
    let cards = graph::cards(&inventories, &nested, Some("sensing/filters"));
    let expected = ["//in/sensing/filters".to_owned(), "//out/sensing/filters".to_owned(), format!("{robot}/smoother")];
    assert_eq!(keys(&cards), sorted(expected));

    // What the workbench works out of such paths.
    assert_eq!([graph::parent("sensing/filters"), graph::parent("sensing")], [Some("sensing"), None]);
    assert_eq!([graph::called("sensing/filters"), graph::called("sensing")], ["filters", "sensing"]);
    assert!(graph::within("sensing/filters", "sensing") && graph::within("sensing", "sensing"));
    assert!(!graph::within("sensing", "sensing/filters") && !graph::within("sensings", "sensing"));
    // Of the groups directly inside one, the one that a group is, or is inside of.
    assert_eq!(graph::child("sensing/filters/fine", Some("sensing")), Some("sensing/filters"));
    assert_eq!(graph::child("sensing/filters", None), Some("sensing"));
    assert_eq!(graph::child("sensing", Some("sensing")), None);
    // Where a node is when the group it is in, or a group around that, is moved: one group out, or out of every group.
    let moved = |slot, to| graph::replant(slot, "sensing/filters", to);
    assert_eq!(moved("sensing/filters/fine", Some("filters")), Some("filters/fine".to_owned()));
    assert_eq!(moved("sensing/filters", Some("filters")), Some("filters".to_owned()));
    assert_eq!([moved("sensing/filters/fine", None), moved("sensing/filters", None)], [Some("fine".to_owned()), None]);
    Ok(())
}
