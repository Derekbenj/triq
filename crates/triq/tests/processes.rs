//! Three separate programs: two that own nodes, and the `triq` command-line
//! tool as a third party that owns nothing and wires them together.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use std::process::Command;
use triq::{Inventory, NodeInfo};

/// Run the command-line tool and return what it printed.
fn triq(args: &[&str]) -> Result<String, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_triq")).args(args).output().expect("run triq");
    if output.status.success() {
        Ok(String::from_utf8(output.stdout).unwrap())
    } else {
        Err(String::from_utf8(output.stderr).unwrap())
    }
}

fn inventory(runtime: &str) -> Option<Inventory> {
    let all: Vec<Inventory> = serde_json::from_str(&triq(&["json"]).ok()?).ok()?;
    all.into_iter().find(|inventory| inventory.runtime == runtime)
}

fn node(runtime: &str, name: &str) -> Option<NodeInfo> {
    inventory(runtime)?.nodes.into_iter().find(|node| node.name == name)
}

fn received(runtime: &str, name: &str) -> u64 {
    node(runtime, name).map_or(0, |node| node.inputs[0].sources.iter().map(|source| source.received).sum())
}

#[test]
fn a_third_program_wires_two_others_and_a_crash_takes_its_wiring_with_it() {
    let (left, right) = (unique("left"), unique("right"));
    let mut producer = station(&left);
    let _consumer = station(&right);
    assert!(
        wait_for(|| inventory(&left).is_some() && inventory(&right).is_some()),
        "both programs announce themselves"
    );
    assert_eq!(inventory(&left).unwrap().factories.len(), 4);
    assert_ne!(inventory(&left).unwrap().pid, inventory(&right).unwrap().pid);

    // Spawn a source in one program and a chain of two nodes in the other, then wire across.
    triq(&["spawn", &left, "counter", "ticks", "--params", r#"{"period_ms":2}"#]).unwrap();
    triq(&["spawn", &right, "doubler", "twice", "--pool", "work"]).unwrap();
    triq(&["spawn", &right, "logger", "log"]).unwrap();
    triq(&["connect", &format!("{left}/ticks/output"), &format!("{right}/twice/input")]).unwrap();
    triq(&["connect", &format!("{right}/twice/output"), &format!("{right}/log/input")]).unwrap();
    assert!(wait_for(|| received(&right, "log") > 50), "data flows through both programs");
    let link = node(&right, "twice").unwrap().inputs[0].sources[0].clone();
    assert_eq!((link.output.as_str(), link.connected, link.lost), (format!("{left}/ticks/output").as_str(), true, 0));

    let listing = triq(&["list"]).unwrap();
    for expected in [left.as_str(), "counter", "ticks", "twice", "pool work", &format!("<- {left}/ticks/output")] {
        assert!(listing.contains(expected), "`triq list` shows {expected:?}:\n{listing}");
    }

    // What an output carries is there for the asking, for a program that does not have its type.
    let fields = triq(&["fields", &format!("{left}/ticks/output")]).unwrap();
    assert!(fields.contains("value") && fields.contains("U64"), "`triq fields` lists the numbers:\n{fields}");

    // The producing program crashes. What the consumer was connected to is gone, and the
    // consumer lets go of it.
    producer.0.kill().unwrap();
    producer.0.wait().unwrap();
    assert!(wait_for(|| inventory(&left).is_none()));
    assert!(wait_for(|| node(&right, "twice").unwrap().inputs[0].sources.is_empty()), "the connection is gone");

    // It comes back under the same name, with a node of the same name. That is another node:
    // nothing hears it until it is connected.
    let _producer = station(&left);
    assert!(wait_for(|| inventory(&left).is_some()));
    triq(&["spawn", &left, "counter", "ticks", "--params", r#"{"period_ms":2}"#]).unwrap();
    let before = received(&right, "log");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!((received(&right, "log"), node(&left, "ticks").unwrap().outputs[0].subscribers), (before, 0));
    triq(&["connect", &format!("{left}/ticks/output"), &format!("{right}/twice/input")]).unwrap();
    assert!(wait_for(|| received(&right, "log") > before + 50), "connected again, data flows again");

    // Requests the owner refuses come back as errors.
    assert!(triq(&["spawn", &left, "nope", "x"]).unwrap_err().contains("unknown"));
    assert!(triq(&["connect", &format!("{left}/ticks/output"), &format!("{right}/ghost/input")]).is_err());
    // So does a connection to an output that is not there: nothing waits for it to appear.
    let refused = triq(&["connect", &format!("{left}/tiks/output"), &format!("{right}/twice/input")]).unwrap_err();
    assert!(refused.contains("unknown output") && refused.contains("tiks"), "{refused}");

    triq(&["set", &format!("{left}/ticks"), r#"{"period_ms":50}"#]).unwrap();
    assert_eq!(node(&left, "ticks").unwrap().params["period_ms"], 50);

    // A node is put in a group, and taken out of it again.
    triq(&["group", &format!("{left}/ticks"), "sources/slow"]).unwrap();
    assert_eq!(node(&left, "ticks").unwrap().group, "sources/slow");
    assert!(triq(&["list"]).unwrap().contains("in group sources/slow"));
    triq(&["group", &format!("{left}/ticks")]).unwrap();
    assert_eq!(node(&left, "ticks").unwrap().group, "");
    // The program offers its three kinds of node as one more kind: wired together, and in a group.
    assert!(
        triq(&["list"]).unwrap().contains("factory chain: a group of ticks (counter), twice (doubler), log (logger)")
    );
    triq(&["spawn", &right, "chain", "first"]).unwrap();
    assert_eq!(node(&right, "first.twice").unwrap().group, "first");
    assert!(wait_for(|| received(&right, "first.log") > 0), "data flows through the group");
    triq(&["remove", &format!("{right}/log")]).unwrap();
    assert!(node(&right, "log").is_none());
    triq(&["shutdown", &right]).unwrap();
    assert!(wait_for(|| inventory(&right).is_none()), "a shutdown request ends the program");
}
