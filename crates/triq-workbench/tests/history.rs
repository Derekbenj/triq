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

/// Send `count` poses, one at a time so that the filter keeps up, and wait until it has passed each on.
fn send(scene: &Scene, count: u64) -> triq::Result {
    let passed = |scene: &Scene| {
        let robot = scene.robot.inventory();
        robot.nodes.iter().find(|node| node.name == "smoother").expect("the filter").outputs[0].sent
    };
    let before = passed(scene);
    for sent in 1..=count {
        scene.imu.pose.send(Pose::default())?;
        assert!(wait_for(|| passed(scene) == before + sent));
    }
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
    // The viewer's other input is connected to nothing.
    // Which of the two inputs comes first depends on the names the two runtimes were given.
    let mut both = [&filtered, &viewed];
    both.sort_by(|one, other| one.to.cmp(&other.to));
    assert_eq!(history.connections().collect::<Vec<_>>(), both, "sorted by the input");
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
fn a_rate_is_measured_over_half_a_second_however_often_the_machine_is_read() -> triq::Result {
    let scene = Scene::new()?;
    let (filtered, _) = connections(&scene);
    let mut history = History::default();
    let mut reading = scene.inventories();
    history.record(&reading);

    // Read every tenth of a second: ten samples each time for half a second, then none.
    let mut rates = Vec::new();
    for samples in [10, 10, 10, 10, 10, 0, 0, 0, 0, 0, 0] {
        send(&scene, samples)?;
        reading = later(&reading, scene.inventories(), [Duration::from_millis(100); 2]);
        history.record(&reading);
        rates.push(history.points(&filtered).last().expect("a point for every reading").rate.round());
    }
    // Counted over a tenth of a second, one sample more or less would be ten
    // a second. So every point is about the half second before it, and a
    // stream that stops is seen to run down over that long.
    assert_eq!(rates, [100.0, 100.0, 100.0, 100.0, 100.0, 80.0, 60.0, 40.0, 20.0, 0.0, 0.0]);
    let times: Vec<f64> = history.points(&filtered).map(|point| point.at).collect();
    assert!(times.windows(2).all(|pair| (pair[1] - pair[0] - 0.1).abs() < 1e-6), "ten points a second");
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

#[test]
fn a_line_is_cut_where_it_crosses_the_edges_of_what_is_in_view() -> triq::Result {
    let scene = Scene::new()?;
    let (filtered, _) = connections(&scene);
    let mut history = History::default();
    let mut reading = scene.inventories();
    history.record(&reading);
    // 80, 60, 20 and 40 samples a second, at two, four, six and eight seconds.
    for samples in [160, 120, 40, 80] {
        send(&scene, samples)?;
        reading = later(&reading, scene.inventories(), [Duration::from_secs(2); 2]);
        history.record(&reading);
    }
    let start = history.points(&filtered).next().unwrap().at - 2.0;
    // Times as seconds from the start, values rounded: what a chart of the rate would draw.
    let line = |since: f64, until: f64| -> Vec<(f64, f64)> {
        let line = history.line(&filtered, Stat::Rate, start + since, start + until);
        line.into_iter().map(|(at, value)| (((at - start) * 10.0).round() / 10.0, value.round())).collect()
    };

    assert_eq!(line(0.0, 10.0), [(2.0, 80.0), (4.0, 60.0), (6.0, 20.0), (8.0, 40.0)], "all of it");
    // It comes in at the left edge on its way from one point to the next, and goes out at the right the same way.
    assert_eq!(line(3.0, 8.0), [(3.0, 70.0), (4.0, 60.0), (6.0, 20.0), (8.0, 40.0)]);
    assert_eq!(line(3.0, 7.0), [(3.0, 70.0), (4.0, 60.0), (6.0, 20.0), (7.0, 30.0)]);
    assert_eq!(line(4.5, 5.5), [(4.5, 50.0), (5.5, 30.0)], "even with no point of its own in view");
    assert_eq!(line(6.0, 6.0), [(6.0, 20.0)]);
    // Before its first point and after its last there is nothing to draw.
    assert_eq!(line(0.0, 1.5), []);
    assert_eq!(line(7.0, 12.0), [(7.0, 30.0), (8.0, 40.0)]);
    assert_eq!(line(9.0, 12.0), []);

    // The newest point of a connection is at hand without looking through the others.
    assert_eq!(history.latest(&filtered).map(|point| point.rate), Some(40.0));
    assert_eq!(history.latest(&Connection { to: "nobody/x/y".into(), from: "nobody/a/b".into() }), None);
    Ok(())
}

#[test]
fn an_output_gets_a_point_for_how_fast_it_sent() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name();
    let (sensor, passed_on, spare) =
        (format!("{robot}/imu/pose"), format!("{robot}/smoother/smooth"), format!("{robot}/spare/pose"));
    // An output nobody hears is measured like any other.
    let unheard: ImuPorts = scene.robot.attach("spare")?;
    let mut history = History::default();
    let first = scene.inventories();
    history.record(&first);
    assert_eq!(history.outputs().collect::<Vec<_>>(), [&sensor, &passed_on, &spare], "sorted by name");
    assert_eq!(history.sending(&sensor), None, "one reading says nothing about the time between two");

    // Thirty samples in half a second, by the robot's clock.
    send(&scene, 30)?;
    let second = later(&first, scene.inventories(), [HALF; 2]);
    history.record(&second);
    let at = second[1].taken_ns as f64 / 1e9;
    assert_eq!((history.sending(&sensor), history.sending(&passed_on)), (Some(60.0), Some(60.0)));
    assert_eq!(history.sending(&spare), Some(0.0));
    assert_eq!(history.sent(&sensor, at - 1.0, at + 1.0), [(at, 60.0)], "a line for a chart, as a connection has");

    // An output that is gone is no longer listed, and what it did is still there to look at.
    drop(unheard);
    history.record(&later(&second, scene.inventories(), [HALF; 2]));
    assert_eq!(history.outputs().collect::<Vec<_>>(), [&sensor, &passed_on]);
    assert_eq!(history.sent(&spare, at - 1.0, at + 1.0), [(at, 0.0)]);
    Ok(())
}
