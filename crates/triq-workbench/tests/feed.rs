//! The workbench's link to the machine: inventories come in by themselves, requests go out in the background.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use triq::{NodeInfo, NodeState, Placement};
use triq_workbench::feed::{Feed, Request};
use triq_workbench::history::Connection;

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
    let Scene { robot, desk, imu, viewer } = Scene::new()?;
    let (robot_name, desk_name) = (robot.name().to_owned(), desk.name().to_owned());
    let on = |node: &str| (robot_name.clone(), node.to_owned());
    assert!(wait_for(|| node(&feed, &robot_name, "smoother").is_some()), "runtimes show up by themselves");
    assert!(redraws.load(Ordering::Relaxed) > 0, "and the screen is asked to redraw");

    // Rates come by themselves too, from one reading of the counters to the next.
    let sending = format!("{robot_name}/imu/pose");
    assert!(feed.rates().get(&sending).is_none_or(|rate| *rate == 0.0), "nothing is flowing yet");
    assert!(wait_for(|| {
        imu.pose.send(Pose::default()).expect("send");
        feed.rates().get(&sending).is_some_and(|rate| *rate > 10.0)
    }));

    // And every reading adds to what is kept of each connection's past.
    let filtered = Connection { to: format!("{robot_name}/smoother/raw"), from: sending.clone() };
    let points = |feed: &Feed| feed.history().points(&filtered).copied().collect::<Vec<_>>();
    assert!(wait_for(|| {
        imu.pose.send(Pose::default()).expect("send");
        points(&feed).len() >= 3
    }));
    let points = points(&feed);
    assert!(points.iter().any(|point| point.rate > 10.0), "{points:?}");
    assert!(
        points.windows(2).all(|pair| pair[1].at - pair[0].at > 0.2),
        "readings are not taken in a rush: {points:?}"
    );

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
