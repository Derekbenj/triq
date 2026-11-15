//! The workbench's link to the machine: inventories come in by themselves, requests go out in the background.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use triq::{NodeInfo, NodeState, Placement};
use triq_workbench::feed::{Feed, Request};

fn node(feed: &Feed, runtime: &str, name: &str) -> Option<NodeInfo> {
    let inventories = feed.inventories();
    let inventory = inventories.iter().find(|inventory| inventory.runtime == runtime)?;
    inventory.nodes.iter().find(|node| node.name == name).cloned()
}

#[test]
fn the_feed_follows_the_machine_and_carries_out_requests() -> triq::Result {
    let redraws = Arc::new(AtomicUsize::new(0));
    let feed = Feed::start({
        let redraws = redraws.clone();
        move || _ = redraws.fetch_add(1, Ordering::Relaxed)
    })?;
    let Scene { robot, desk, imu: _imu, viewer } = Scene::new()?;
    let (robot_name, desk_name) = (robot.name().to_owned(), desk.name().to_owned());
    let on = |node: &str| (robot_name.clone(), node.to_owned());
    assert!(wait_for(|| node(&feed, &robot_name, "smoother").is_some()), "runtimes show up by themselves");
    assert!(redraws.load(Ordering::Relaxed) > 0, "and the screen is asked to redraw");

    // Each request is carried out in the background and shows in the inventory that follows.
    let (runtime, name) = on("extra");
    feed.ask(Request::Spawn {
        runtime,
        kind: "filter".into(),
        name,
        placement: Placement::Dedicated,
        params: json!({ "alpha": 1.0 }),
    });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").is_some()));

    let (from, to) = (format!("{robot_name}/imu/pose"), format!("{robot_name}/extra/raw"));
    feed.ask(Request::Connect { from: from.clone(), to: to.clone() });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().inputs[0].sources.len() == 1));

    let (runtime, name) = on("extra");
    feed.ask(Request::SetParams { runtime, node: name, params: json!({ "alpha": 0.75 }) });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().params == json!({ "alpha": 0.75 })));

    let (runtime, name) = on("extra");
    feed.ask(Request::Pause { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().state == NodeState::Paused));
    let (runtime, name) = on("extra");
    feed.ask(Request::Resume { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().state == NodeState::Running));

    feed.ask(Request::Disconnect { from, to });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().inputs[0].sources.is_empty()));
    let (runtime, name) = on("extra");
    feed.ask(Request::Remove { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").is_none()));
    assert_eq!(feed.problem(), None);

    // A request the owner refuses is reported until it is dismissed.
    let (runtime, name) = on("x");
    feed.ask(Request::Spawn {
        runtime,
        kind: "nope".into(),
        name,
        placement: Placement::Dedicated,
        params: json!(null),
    });
    assert!(wait_for(|| feed.problem().is_some()));
    let problem = feed.problem().unwrap();
    assert!(problem.contains("nope") && problem.contains("create"), "{problem}");
    feed.dismiss();
    assert_eq!(feed.problem(), None);

    // A program that stops disappears.
    drop(viewer);
    drop(desk);
    assert!(wait_for(|| feed.inventories().iter().all(|inventory| inventory.runtime != desk_name)));
    Ok(())
}
