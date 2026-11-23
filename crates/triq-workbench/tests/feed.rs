//! The workbench's link to the machine: inventories come in by themselves, requests go out in the background.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use triq::{NodeInfo, NodeState, Placement};
use triq_workbench::feed::{Feed, Request};
use triq_workbench::history::Connection;

/// Held by each test from before it starts its feed. A feed asks every runtime on the machine,
/// and the first time it asks one takes a moment: many times what asking it again does. Four
/// feeds doing that at once, while the scenes of four tests come and go and whatever else
/// runs on the machine is asked too, take longer over it than a test waits.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static FEED: Mutex<()> = Mutex::new(());
    FEED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn node(feed: &Feed, runtime: &str, name: &str) -> Option<NodeInfo> {
    let inventories = feed.inventories();
    let inventory = inventories.iter().find(|inventory| inventory.runtime == runtime)?;
    inventory.nodes.iter().find(|node| node.name == name).cloned()
}

#[test]
fn the_feed_follows_the_machine_and_carries_out_requests() -> triq::Result {
    let _turn = one_at_a_time();
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

    // And every reading adds to what is kept of each connection's past: ten points a second.
    let filtered = Connection { to: format!("{robot_name}/smoother/raw"), from: sending.clone() };
    let points = |feed: &Feed| feed.history().points(&filtered).copied().collect::<Vec<_>>();
    assert!(wait_for(|| {
        imu.pose.send(Pose::default()).expect("send");
        points(&feed).len() >= 15
    }));
    let points = points(&feed);
    assert!(points.iter().any(|point| point.rate > 10.0), "{points:?}");
    let mut apart: Vec<f64> = points.windows(2).map(|pair| pair[1].at - pair[0].at).collect();
    apart.sort_by(f64::total_cmp);
    assert!((0.08..0.14).contains(&apart[apart.len() / 2]), "a tenth of a second apart, as a rule: {apart:?}");
    assert!(apart[0] > 0.04, "and never taken in a rush: {apart:?}");

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
    feed.ask(Request::ChangeParams { runtime, node: name, changes: json!({ "alpha": 0.5 }) });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().params == json!({ "alpha": 0.5 })));

    // Which group a node is in is its runtime's to say, too.
    let (runtime, name) = on("extra");
    feed.ask(Request::SetGroup { runtime, node: name, group: "filters/fine".to_owned() });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().group == "filters/fine"));

    let (runtime, name) = on("extra");
    feed.ask(Request::Pause { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().state == NodeState::Paused));
    let (runtime, name) = on("extra");
    feed.ask(Request::Resume { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().state == NodeState::Running));

    feed.ask(Request::Disconnect { from, to });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").unwrap().inputs[0].sources.is_empty()));
    // A node that is removed takes its wires with it: what heard it, here from another runtime, lets go of it.
    let heard = format!("{robot_name}/extra/smooth");
    feed.ask(Request::Connect { from: heard.clone(), to: format!("{desk_name}/viewer/pose") });
    let hears = |feed: &Feed| {
        let viewer = node(feed, &desk_name, "viewer").expect("the viewer");
        viewer.inputs[0].sources.iter().any(|source| source.output == heard)
    };
    assert!(wait_for(|| hears(&feed)));
    let (runtime, name) = on("extra");
    feed.ask(Request::Remove { runtime, node: name });
    assert!(wait_for(|| node(&feed, &robot_name, "extra").is_none() && !hears(&feed)));
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

#[test]
fn nothing_of_the_feed_runs_once_it_is_dropped() -> triq::Result {
    let _turn = one_at_a_time();
    // What the feed calls to have the window drawn again belongs to its threads.
    let kept = Arc::new(());
    let feed = Feed::start({
        let kept = kept.clone();
        move || _ = &kept
    })?;
    assert!(Arc::strong_count(&kept) > 1);
    // It has read the machine once. That first reading opens every runtime there is, which on a
    // busy machine can take longer than a thread is waited for.
    let Scene { robot, desk: _desk, imu: _imu, viewer: _viewer } = Scene::new()?;
    assert!(wait_for(|| node(&feed, robot.name(), "smoother").is_some()));

    // When the workbench closes, its program ends at once, and a thread that is still in the
    // middle of something leaves that on the machine. So dropping the feed waits for them.
    drop(feed);
    assert_eq!(Arc::strong_count(&kept), 1, "its threads have ended, and let go of what they held");
    Ok(())
}

#[test]
fn requests_that_are_applied_together_stop_at_the_first_that_is_refused() -> triq::Result {
    let _turn = one_at_a_time();
    let feed = Feed::start(|| {})?;
    let Scene { robot, desk, imu: _imu, viewer: _viewer } = Scene::new()?;
    let (robot, desk) = (robot.name().to_owned(), desk.name().to_owned());
    assert!(wait_for(|| node(&feed, &desk, "viewer").is_some()));
    let heard = |feed: &Feed| {
        let viewer = node(feed, &desk, "viewer").expect("the viewer");
        viewer.inputs[0].sources.iter().map(|source| source.output.clone()).collect::<Vec<_>>()
    };
    let (pose, smooth) = (format!("{robot}/imu/pose"), format!("{robot}/smoother/smooth"));
    let viewer = format!("{desk}/viewer/pose");

    // Three changes, to be made in this order. The runtime refuses the second: it has no such node.
    let changes = vec![
        Request::Connect { from: pose.clone(), to: viewer.clone() },
        Request::Pause { runtime: robot.clone(), node: "nobody".to_owned() },
        Request::Disconnect { from: smooth.clone(), to: viewer.clone() },
    ];
    feed.apply(changes.clone());
    assert!(wait_for(|| feed.problem().is_some()));
    let problem = feed.problem().unwrap();
    assert!(problem.contains("pause") && problem.contains("1 of the 3"), "{problem}");
    // The first was made. The second and the third were not, and are handed back, once.
    assert!(wait_for(|| heard(&feed) == [smooth.clone(), pose.clone()]), "{:?}", heard(&feed));
    assert_eq!(feed.unmade(), changes[1..]);
    assert_eq!(feed.unmade(), []);
    feed.dismiss();

    // When every one of them is made, nothing comes back.
    let cut = |from: &String| Request::Disconnect { from: from.clone(), to: viewer.clone() };
    feed.apply(vec![cut(&pose), cut(&smooth)]);
    assert!(wait_for(|| heard(&feed).is_empty()));
    assert_eq!((feed.problem(), feed.unmade()), (None, Vec::new()));

    // A request that was asked by itself is not handed back when it is refused: it is only reported.
    feed.ask(changes[1].clone());
    assert!(wait_for(|| feed.problem().is_some()));
    assert_eq!(feed.unmade(), []);
    Ok(())
}

#[test]
fn the_feed_says_when_what_was_asked_for_shows_in_what_it_reports() -> triq::Result {
    let _turn = one_at_a_time();
    let feed = Feed::start(|| {})?;
    let Scene { robot, desk: _desk, imu: _imu, viewer: _viewer } = Scene::new()?;
    let robot = robot.name().to_owned();
    assert!(wait_for(|| node(&feed, &robot, "smoother").is_some()));
    assert!(feed.settled(), "nothing was asked for");

    // From the moment something is asked for, the machine is not as the feed reports it:
    // not until the request was carried out, and the machine was read after that.
    let group = |group: &str| Request::SetGroup {
        runtime: robot.clone(),
        node: "smoother".to_owned(),
        group: group.to_owned(),
    };
    for name in ["one", "two", "three"] {
        feed.ask(group(name));
        assert!(!feed.settled());
        assert!(wait_for(|| feed.settled()));
        assert_eq!(node(&feed, &robot, "smoother").expect("the filter").group, name);
    }

    // Something that is refused is settled as well, once it was: there is nothing more to wait for.
    feed.ask(Request::Pause { runtime: robot.clone(), node: "nobody".to_owned() });
    assert!(!feed.settled());
    assert!(wait_for(|| feed.settled()));
    assert!(feed.problem().is_some());
    Ok(())
}
