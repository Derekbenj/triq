//! Nodes put together in a group: drawn as one card from outside, and from
//! inside as its nodes between two cards that stand for the outside.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use triq::{Placement, Remote};
use triq_workbench::graph::{self, Card, Groups, Kind, Places};
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
    assert_eq!(written(entry), [vec![], vec!["smoother.raw: Pose".to_owned()]]);
    assert_eq!(written(exit), [vec!["smoother.smooth: Pose".to_owned()], vec![]]);
    graph::show(&mut snarl, cards, &mut places, 2);
    assert_eq!(wires(&snarl), ["Group Input raw -> smoother raw", "smoother smooth -> Group Output smooth"]);
    // The filter's card is where it was outside, with the two others to its left and to its right.
    let inside = |key: &str| place(&snarl, key).expect("a card inside the group");
    assert_eq!(inside(&format!("{robot}/smoother")), outside);
    assert!(inside("//in/filtering").x < outside.x && outside.x < inside("//out/filtering").x);

    // A port that nothing is connected to leads nowhere, and is not on the cards for the outside:
    // inside the group it is a pin of its node like any other, to be wired there. From outside, the
    // card of the group offers it, to be wired from there.
    scene.robot.spawn_kind("filter", "spare", Placement::Dedicated, serde_json::json!(null))?;
    let mut with_spare = filtering.clone();
    with_spare.insert(format!("{robot}/spare"), "filtering".to_owned());
    let cards = graph::cards(&scene.inventories(), &with_spare, Some("filtering"));
    assert_eq!(written(called(&cards, "//in/filtering"))[1], ["smoother.raw: Pose"]);
    assert_eq!(written(called(&cards, "//out/filtering"))[0], ["smoother.smooth: Pose"]);
    let cards = graph::cards(&scene.inventories(), &with_spare, None);
    let offered = written(called(&cards, "/filtering"));
    assert_eq!(offered[0], ["smoother.raw: Pose", "spare.raw: Pose"]);
    assert_eq!(offered[1], ["smoother.smooth: Pose", "spare.smooth: Pose"]);
    Remote::open(&robot)?.remove("spare")?;

    // Outside again, the group is one card where its node was wired: the sensor feeds it, and it the viewer.
    let cards = graph::cards(&scene.inventories(), &filtering, None);
    assert_eq!(keys(&cards), sorted(["/filtering".to_owned(), format!("{desk}/viewer"), format!("{robot}/imu")]));
    graph::show(&mut snarl, cards, &mut places, 3);
    assert_eq!(wires(&snarl), ["filtering smooth -> viewer pose", "imu pose -> filtering raw"]);
    // Wires join the cards that are wired to a group as they join any others.
    let lit: Vec<String> = graph::joined(&snarl, "/filtering").into_iter().collect();
    assert_eq!(lit, sorted(["/filtering".to_owned(), format!("{desk}/viewer"), format!("{robot}/imu")]));

    // A group that is all there is to its own wiring has no outside to stand for.
    let all = groups([
        (format!("{robot}/imu"), "all"),
        (format!("{robot}/smoother"), "all"),
        (format!("{desk}/viewer"), "all"),
    ]);
    let cards = graph::cards(&scene.inventories(), &all, Some("all"));
    // Only the viewer's input for images leads outside: it waits for an output that does not exist.
    assert!(cards.iter().all(|card| card.key() != "//out/all"));
    assert_eq!(written(called(&cards, "//in/all"))[1], ["viewer.image: u8[]"]);
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
    assert_eq!(wires(&snarl), ["filters smooth -> Group Output smooth", "imu pose -> filters raw"]);

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
