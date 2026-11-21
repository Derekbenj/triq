//! Groups: a node says which group it is in, and a runtime offers several
//! nodes, wired to each other, as one kind.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use triq::prelude::*;
use triq::{Group, LocalNodes, NodeInfo, Remote};

#[derive(Ports)]
struct Bench {
    feed: Output<u64>,
    probe: Input<u64>,
}

#[derive(Ports)]
struct Through {
    input: Input<u64>,
    output: Output<u64>,
}

#[derive(Ports)]
#[allow(dead_code)]
struct Poses {
    out: Output<Pose>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GainParams {
    factor: u64,
}

impl Default for GainParams {
    fn default() -> Self {
        Self { factor: 1 }
    }
}

struct Gain {
    factor: u64,
}

impl Node for Gain {
    type Ports = Through;
    type Params = GainParams;
    fn new(params: GainParams) -> triq::Result<Self> {
        Ok(Self { factor: params.factor })
    }
    fn run(&mut self, ports: &mut Through) -> triq::Result {
        while let Some(value) = ports.input.recv()? {
            ports.output.send(*value * self.factor)?;
        }
        Ok(())
    }
}

fn node(rt: &Runtime, name: &str) -> Option<NodeInfo> {
    rt.inventory().nodes.into_iter().find(|node| node.name == name)
}

/// The group a node says it is in. `None` if there is no such node.
fn group(rt: &Runtime, name: &str) -> Option<String> {
    node(rt, name).map(|node| node.group)
}

#[test]
fn a_node_says_which_group_it_is_in() -> triq::Result {
    let rt = Runtime::new(&unique("groups"))?;
    rt.register::<Gain>("gain")?;
    rt.spawn_kind("gain", "left", Placement::Dedicated, json!(null))?;
    let _bench: Bench = rt.attach("bench")?;
    assert_eq!(group(&rt, "left").as_deref(), Some(""), "a node is in no group to begin with");

    // The program that owns the node puts it in a group. A tool that is watching is told.
    let before = rt.inventory().revision;
    rt.set_group("left", "filters")?;
    assert_eq!(group(&rt, "left").as_deref(), Some("filters"));
    assert!(rt.inventory().revision > before);

    // Another program moves it into a group inside that one, and puts a node that the
    // application holds in the outer one.
    let remote = Remote::open(rt.name())?;
    remote.set_group("left", "filters/fine")?;
    remote.set_group("bench", "filters")?;
    let groups = |inventory: triq::Inventory| -> Vec<(String, String)> {
        inventory.nodes.into_iter().map(|node| (node.name, node.group)).collect()
    };
    let expected = [("bench", "filters"), ("left", "filters/fine")].map(|(node, group)| (node.into(), group.into()));
    assert_eq!(groups(remote.inventory()?), expected);

    // Out of every group again.
    remote.set_group("left", "")?;
    assert_eq!(group(&rt, "left").as_deref(), Some(""));

    // What is refused: a node that is not there, and a name that is no name.
    assert!(matches!(remote.set_group("ghost", "filters"), Err(Error::Unknown(_))));
    for bad in ["/filters", "filters/", "filters//fine", "two words", "_hidden"] {
        assert!(matches!(rt.set_group("left", bad), Err(Error::Name(_))), "{bad:?} is refused");
    }
    assert_eq!(group(&rt, "left").as_deref(), Some(""));

    // A node that is removed takes its group with it: a new node of its name is in none.
    rt.set_group("left", "filters")?;
    rt.remove("left")?;
    rt.spawn_kind("gain", "left", Placement::Dedicated, json!(null))?;
    assert_eq!(group(&rt, "left").as_deref(), Some(""));
    Ok(())
}

#[test]
fn a_runtime_offers_several_nodes_wired_together_as_one_kind() -> triq::Result {
    let rt = Runtime::builder(&unique("kinds")).pool("work", 2).start()?;
    rt.register::<Gain>("gain")?;
    let chain = Group::new()
        .spawn("gain", "first", Placement::Dedicated, json!({ "factor": 2 }))
        .spawn("gain", "second", Placement::pool("work"), json!({ "factor": 5 }))
        .connect("first/output", "second/input");
    rt.register_group("chain", chain.clone())?;

    // The inventory lists the kind with what it creates. A kind of node creates no group.
    let offered = rt.inventory().factories;
    let kinds: Vec<(&str, Option<&Group>)> =
        offered.iter().map(|factory| (factory.kind.as_str(), factory.group.as_ref())).collect();
    assert_eq!(kinds, [("chain", Some(&chain)), ("gain", None)]);

    // Another program asks for one, as it asks for a node. Its nodes are named after it, are in
    // a group of its name, have the settings and the threads the kind gives them, and are wired.
    let bench: Bench = rt.attach("bench")?;
    Remote::open(rt.name())?.spawn("chain", "x", Placement::Dedicated, json!(null))?;
    let (first, second) = (node(&rt, "x.first").expect("the first"), node(&rt, "x.second").expect("the second"));
    assert_eq!((first.kind.as_str(), first.group.as_str(), &first.params), ("gain", "x", &json!({ "factor": 2 })));
    assert_eq!((second.group.as_str(), second.placement), ("x", Some(Placement::pool("work"))));
    assert_eq!(second.inputs[0].sources[0].output, format!("{}/x.first/output", rt.name()));
    rt.connect("bench/feed", "x.first/input")?;
    rt.connect("x.second/output", "bench/probe")?;
    assert!(wait_for(|| node(&rt, "x.first").is_some_and(|node| node.outputs[0].subscribers == 1)));
    bench.feed.send(3)?;
    assert_eq!(collect(&bench.probe, 1), [30]);

    // Another of the kind is beside it, under its own name.
    rt.spawn_kind("chain", "y", Placement::Dedicated, json!(null))?;
    assert_eq!(group(&rt, "y.second").as_deref(), Some("y"));

    // A group is made whole or not at all. Here the name of its second node is taken.
    let _taken: Bench = rt.attach("z.second")?;
    assert!(matches!(rt.spawn_kind("chain", "z", Placement::Dedicated, json!(null)), Err(Error::Name(_))));
    assert!(node(&rt, "z.first").is_none(), "the node that was made first is gone again");
    Ok(())
}

#[test]
fn a_group_takes_nodes_the_application_runs_and_outputs_from_outside_it() -> triq::Result {
    let rt = Runtime::new(&unique("mixed"))?;
    rt.register::<Gain>("gain")?;
    let mut held: LocalNodes<Through, GainParams> = rt.local_nodes("held")?;
    let bench: Bench = rt.attach("bench")?;
    // The first node of the group hears an output that is not of the group: `runtime/node/port`.
    let feed = format!("{}/bench/feed", rt.name());
    let pair = Group::new()
        .spawn("held", "mine", Placement::Dedicated, json!({ "factor": 7 }))
        .spawn("gain", "after", Placement::Dedicated, json!({ "factor": 3 }))
        .connect(&feed, "mine/input")
        .connect("mine/output", "after/input");
    rt.register_group("pair", pair)?;
    rt.spawn_kind("pair", "p", Placement::Dedicated, json!(null))?;
    rt.connect("p.after/output", "bench/probe")?;

    // The application is handed its node, as it is handed any other it was asked for.
    held.serve();
    let mine = held.nodes.get("p.mine").expect("the node the application runs");
    assert_eq!((mine.params.factor, group(&rt, "p.mine").as_deref()), (7, Some("p")));
    assert!(wait_for(|| node(&rt, "p.after").is_some_and(|node| node.inputs[0].sources[0].connected)));
    bench.feed.send(2)?;
    let heard = collect(&mine.ports.input, 1);
    mine.ports.output.send(heard[0] * mine.params.factor)?;
    assert_eq!(collect(&bench.probe, 1), [42]);
    Ok(())
}

#[test]
fn a_group_that_could_not_be_made_is_not_offered() -> triq::Result {
    let rt = Runtime::new(&unique("refused"))?;
    rt.register::<Gain>("gain")?;
    let _posers: LocalNodes<Poses, ()> = rt.local_nodes("poser")?;
    let _poses: Poses = rt.attach("poses")?;
    let one = |kind: &str, name: &str| Group::new().spawn(kind, name, Placement::Dedicated, json!(null));
    let two = || one("gain", "a").spawn("gain", "b", Placement::Dedicated, json!(null));
    rt.register_group("two", two().connect("a/output", "b/input"))?;

    let refused = |group: Group| rt.register_group("bad", group).expect_err("it is refused");
    // No nodes at all; a kind that is not registered, or is itself a group; two nodes of one name.
    assert!(matches!(refused(Group::new()), Error::Mismatch(_)));
    assert!(matches!(refused(one("nope", "a")), Error::Unknown(_)));
    assert!(matches!(refused(one("two", "a")), Error::Mismatch(_)));
    assert!(matches!(refused(two().spawn("gain", "a", Placement::Dedicated, json!(null))), Error::Name(_)));
    // A wire to or from a port that is not there, the wrong way round, or to a node that is not of the group.
    assert!(matches!(refused(two().connect("a/nope", "b/input")), Error::Unknown(_)));
    assert!(matches!(refused(two().connect("a/output", "c/input")), Error::Unknown(_)));
    assert!(matches!(refused(two().connect("a/input", "b/input")), Error::Unknown(_)));
    assert!(matches!(refused(two().connect("a/output", "b/output")), Error::Unknown(_)));
    assert!(matches!(refused(two().connect("a/output", &format!("{}/b/input", rt.name()))), Error::Unknown(_)));
    // A wire between two of its nodes whose ports carry different messages.
    let posed = two().spawn("poser", "p", Placement::Dedicated, json!(null)).connect("p/out", "a/input");
    assert!(matches!(refused(posed), Error::Mismatch(_)));
    assert_eq!(rt.inventory().factories.len(), 3, "none of them is offered");

    // A wire between ports that carry different messages is found when the group is asked
    // for, if its output is outside the group. Nothing is left of the group then.
    let from_outside = two().connect(&format!("{}/poses/out", rt.name()), "a/input");
    rt.register_group("mismatched", from_outside)?;
    assert!(matches!(rt.spawn_kind("mismatched", "m", Placement::Dedicated, json!(null)), Err(Error::Mismatch(_))));
    assert_eq!(rt.inventory().nodes.len(), 1, "only the node that was there before");
    // So is an output outside the group that is not there then: a wire needs both its ends.
    let early = two().connect(&format!("{}/nobody/out", rt.name()), "a/input");
    rt.register_group("early", early)?;
    assert!(matches!(rt.spawn_kind("early", "e", Placement::Dedicated, json!(null)), Err(Error::Unknown(_))));
    assert_eq!(rt.inventory().nodes.len(), 1, "only the node that was there before");
    Ok(())
}

/// A group with one more node in it: a `gain` of that name, which multiplies by `factor`.
fn gain(group: Group, name: &str, factor: u64) -> Group {
    group.spawn("gain", name, Placement::Dedicated, json!({ "factor": factor }))
}

#[test]
fn a_kind_of_group_puts_some_of_its_nodes_in_a_group_inside_it() -> triq::Result {
    let rt = Runtime::new(&unique("inner"))?;
    rt.register::<Gain>("gain")?;
    // A group inside the group is written as a group is: its nodes, and its wires by the names
    // those nodes have in it. Outside it they are called after it: `fine.second`.
    let fine = gain(gain(Group::new(), "second", 5), "third", 7).connect("second/output", "third/input");
    let stack = gain(Group::new(), "first", 2).group("fine", fine).connect("first/output", "fine.second/input");
    rt.register_group("stack", stack)?;

    // The inventory lists its nodes one after the other, each with the group inside the group
    // that it is in, and its wires by what the nodes are called in the whole group.
    let offered = rt.inventory().factories.into_iter().find(|factory| factory.kind == "stack");
    let listed = offered.and_then(|factory| factory.group).expect("what the kind creates");
    let members: Vec<(&str, &str)> =
        listed.nodes.iter().map(|node| (node.name.as_str(), node.group.as_str())).collect();
    assert_eq!(members, [("first", ""), ("fine.second", "fine"), ("fine.third", "fine")]);
    let wire = |from: &str, to: &str| (from.to_owned(), to.to_owned());
    assert_eq!(
        listed.wires,
        [wire("fine.second/output", "fine.third/input"), wire("first/output", "fine.second/input")]
    );

    // Asked for under the name `x`, the group inside it is `x/fine`, and everything is wired.
    let bench: Bench = rt.attach("bench")?;
    rt.spawn_kind("stack", "x", Placement::Dedicated, json!(null))?;
    let nodes = ["x.first", "x.fine.second", "x.fine.third"];
    assert_eq!(nodes.map(|node| group(&rt, node)), ["x", "x/fine", "x/fine"].map(|group| Some(group.to_owned())));
    rt.connect("bench/feed", "x.first/input")?;
    rt.connect("x.fine.third/output", "bench/probe")?;
    let heard = |name: &str| node(&rt, name).is_some_and(|node| node.outputs[0].subscribers == 1);
    assert!(wait_for(|| nodes.into_iter().chain(["bench"]).all(heard)));
    bench.feed.send(3)?;
    assert_eq!(collect(&bench.probe, 1), [3 * 2 * 5 * 7]);

    // A group inside a group inside the group.
    let deep = Group::new().group("a", Group::new().group("b", gain(Group::new(), "n", 1)));
    rt.register_group("deep", deep)?;
    rt.spawn_kind("deep", "y", Placement::Dedicated, json!(null))?;
    assert_eq!(group(&rt, "y.a.b.n").as_deref(), Some("y/a/b"));

    // Made whole or not at all. Here the name of a node of the group inside is taken.
    let _taken: Bench = rt.attach("z.fine.third")?;
    assert!(matches!(rt.spawn_kind("stack", "z", Placement::Dedicated, json!(null)), Err(Error::Name(_))));
    assert!(node(&rt, "z.first").is_none() && node(&rt, "z.fine.second").is_none(), "nothing of it is left");

    // A group as a runtime listed it before a group could be inside another: every node is directly in it.
    let node = json!({ "kind": "gain", "name": "a", "placement": "Dedicated", "params": null });
    let old: Group = serde_json::from_value(json!({ "nodes": [node], "wires": [] }))?;
    assert_eq!(old.nodes[0].group, "");
    Ok(())
}

#[test]
fn a_group_inside_a_group_that_could_not_be_made_is_not_offered() -> triq::Result {
    let rt = Runtime::new(&unique("badinner"))?;
    rt.register::<Gain>("gain")?;
    let refused = |group: Group| rt.register_group("bad", group).expect_err("it is refused");
    let inner = || gain(Group::new(), "second", 1);
    // What the group inside is called has to be a name.
    for bad in ["", "two words", "a/b", "_hidden"] {
        assert!(matches!(refused(Group::new().group(bad, inner())), Error::Name(_)), "{bad:?} is refused");
    }
    // A wire of the group inside is between its own nodes: `first` is not one of them.
    let reaching = gain(Group::new(), "first", 1).group("fine", inner().connect("first/output", "second/input"));
    assert!(matches!(refused(reaching), Error::Unknown(_)));
    // Two nodes that come to the same name.
    assert!(matches!(refused(gain(Group::new(), "fine.second", 1).group("fine", inner())), Error::Name(_)));
    assert!(rt.inventory().factories.iter().all(|factory| factory.kind != "bad"), "none of them is offered");
    Ok(())
}
