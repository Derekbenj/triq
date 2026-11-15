//! What every connection did over time: a point for each time the machine was read.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use std::time::Duration;
use triq_workbench::history::{Connection, History, Stat};

const HALF: Duration = Duration::from_millis(500);

/// The filter's input hearing the sensor, and the viewer's hearing the filter.
fn connections(scene: &Scene) -> (Connection, Connection) {
    let (robot, desk) = (scene.robot.name(), scene.desk.name());
    let filtered = Connection { to: format!("{robot}/smoother/raw"), from: format!("{robot}/imu/pose") };
    let viewed = Connection { to: format!("{desk}/viewer/pose"), from: format!("{robot}/smoother/smooth") };
    (filtered, viewed)
}

/// Send `count` poses and wait until the filter has passed them all on.
fn send(scene: &Scene, count: u64) -> triq::Result {
    let passed = |scene: &Scene| {
        let robot = scene.robot.inventory();
        robot.nodes.iter().find(|node| node.name == "smoother").expect("the filter").outputs[0].sent
    };
    let before = passed(scene);
    for _ in 0..count {
        scene.imu.pose.send(Pose::default())?;
    }
    assert!(wait_for(|| passed(scene) == before + count));
    Ok(())
}

#[test]
fn a_connection_gets_a_point_for_what_happened_between_two_readings() -> triq::Result {
    let scene = Scene::new()?;
    let (filtered, viewed) = connections(&scene);
    let mut history = History::default();
    assert_eq!(history.newest(), None);

    let first = scene.inventories();
    history.record(&first);
    // The viewer's other input waits for an output that does not exist: that is not a connection yet.
    assert_eq!(history.connections().collect::<Vec<_>>(), [&filtered, &viewed], "sorted by the input");
    assert_eq!(history.points(&filtered).count(), 0, "one reading says nothing about the time between two");

    // Thirty samples in half a second, by the robot's clock.
    send(&scene, 30)?;
    while scene.viewer.pose.recv()?.is_some() {}
    let second = later(&first, scene.inventories(), [HALF; 2]);
    history.record(&second);
    let source = &second[1].nodes.iter().find(|node| node.name == "smoother").unwrap().inputs[0].sources[0];
    let point = *history.points(&filtered).last().expect("a point");
    assert_eq!(point.at, second[1].taken_ns as f64 / 1e9, "placed when the receiving runtime read its counters");
    assert_eq!((point.rate, point.lost), (60.0, 0.0));
    assert_eq!(point.latency, source.latency_us as f64 / 1000.0, "in milliseconds, as reported");
    assert_eq!(point.gap, source.gap_us as f64 / 1000.0);
    assert_eq!(history.newest(), Some(point.at));
    for (stat, value) in [(Stat::Rate, 60.0), (Stat::Lost, 0.0), (Stat::Gap, point.gap), (Stat::Latency, point.latency)]
    {
        assert_eq!(stat.of(&point), value, "{}", stat.name());
    }

    // Forty more while the viewer does not read. Its queue holds the newest 32,
    // and it finds the other eight missing once it reads.
    send(&scene, 40)?;
    while scene.viewer.pose.recv()?.is_some() {}
    history.record(&later(&second, scene.inventories(), [HALF; 2]));
    let points: Vec<_> = history.points(&viewed).collect();
    assert_eq!(points.len(), 2);
    assert_eq!((points[0].rate, points[0].lost), (60.0, 0.0));
    assert_eq!((points[1].rate, points[1].lost), (64.0, 16.0), "per second");
    let filter = history.points(&filtered).last().unwrap();
    assert_eq!((filter.rate, filter.lost), (80.0, 0.0), "the filter kept up");
    Ok(())
}

#[test]
fn a_connection_that_starts_again_is_measured_afresh() -> triq::Result {
    let scene = Scene::new()?;
    let (filtered, viewed) = connections(&scene);
    let mut history = History::default();
    let first = scene.inventories();
    history.record(&first);
    send(&scene, 10)?;
    let second = later(&first, scene.inventories(), [HALF; 2]);
    history.record(&second);
    assert_eq!(history.points(&viewed).count(), 1);

    // Disconnected: no longer listed, and what it did is still there to look at.
    scene.desk.disconnect(&viewed.from, "viewer/pose")?;
    let third = later(&second, scene.inventories(), [HALF; 2]);
    history.record(&third);
    assert_eq!(history.connections().collect::<Vec<_>>(), [&filtered]);
    assert_eq!(history.points(&viewed).count(), 1);

    // Connected again, its counters start from nothing: the first reading is only where it starts from.
    scene.desk.connect(&viewed.from, "viewer/pose")?;
    send(&scene, 10)?;
    while scene.viewer.pose.recv()?.is_some() {}
    let fourth = later(&third, scene.inventories(), [HALF; 2]);
    history.record(&fourth);
    assert_eq!(history.connections().count(), 2);
    assert_eq!(history.points(&viewed).count(), 1, "nothing is measured across the break");
    send(&scene, 10)?;
    while scene.viewer.pose.recv()?.is_some() {}
    let fifth = later(&fourth, scene.inventories(), [HALF; 2]);
    history.record(&fifth);
    assert_eq!(history.points(&viewed).count(), 2);

    // A program that started again counts from nothing too.
    let mut restarted = later(&fifth, scene.inventories(), [HALF; 2]);
    restarted.iter_mut().for_each(|inventory| inventory.pid += 1);
    history.record(&restarted);
    assert_eq!(history.points(&viewed).count(), 2);
    assert_eq!(history.points(&filtered).count(), 4, "one for each reading until then, after the first");
    Ok(())
}

#[test]
fn the_past_is_kept_for_an_hour() -> triq::Result {
    let scene = Scene::new()?;
    let (filtered, viewed) = connections(&scene);
    let mut history = History::default();
    let first = scene.inventories();
    history.record(&first);
    let second = later(&first, scene.inventories(), [HALF; 2]);
    history.record(&second);
    scene.desk.disconnect(&viewed.from, "viewer/pose")?;

    // Most of an hour later everything is still there.
    let almost = later(&second, scene.inventories(), [Duration::from_secs(3590); 2]);
    history.record(&almost);
    assert_eq!((history.points(&filtered).count(), history.points(&viewed).count()), (2, 1));

    // A little later the first points are too old to keep, and with them the connection that is gone.
    let past = later(&almost, scene.inventories(), [Duration::from_secs(20); 2]);
    history.record(&past);
    assert_eq!((history.points(&filtered).count(), history.points(&viewed).count()), (2, 0));
    let oldest = history.points(&filtered).next().unwrap().at;
    assert_eq!(oldest, almost[1].taken_ns as f64 / 1e9);
    Ok(())
}
