//! The second tab: numbers of connections dragged onto charts and drawn over time.
//!
//! The charts are drawn from a history made here, with known numbers at known
//! times, so what a chart must show is known exactly.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use scene::*;
use std::time::Duration;
use triq_workbench::charts::{self, Charts, Recorded, Trace};
use triq_workbench::egui::{self, accesskit::Role};
use triq_workbench::history::{Connection, History, Stat};
use triq_workbench::signals::Signals;
use window::drag;

struct Shown {
    charts: Charts,
    history: History,
    signals: Signals,
    /// The time at the right edge of every chart.
    now: f64,
}

type Window<'a> = Harness<'a, Shown>;

/// The filter's input heard 160, 120, 40 and then 80 samples in four
/// stretches of two seconds: 80, 60, 20 and 40 a second, the last just now.
fn history(scene: &Scene) -> triq::Result<History> {
    let mut history = History::default();
    let mut reading = scene.inventories();
    history.record(&reading);
    for samples in [160, 120, 40, 80] {
        for _ in 0..samples {
            scene.imu.pose.send(Pose::default())?;
            // Taken one at a time, so that the filter and the viewer both keep up.
            assert!(wait_for(|| scene.viewer.pose.recv().expect("recv").is_some()));
        }
        reading = later(&reading, scene.inventories(), [Duration::from_secs(2); 2]);
        history.record(&reading);
    }
    Ok(history)
}

/// The row of one number of a connection in the list: the thing to drag.
fn number<'a>(harness: &'a Window<'_>, input: &'a str, stat: &'a str) -> Node<'a> {
    let connection = harness.get_by_label(input).parent().expect("the connection's part of the list");
    connection.get_by_label(stat)
}

/// Where to press on one number in the list. Topics of other programs on the
/// machine may be listed above it, so it is brought into view first.
fn grab(harness: &mut Window<'_>, entry: &str, word: &str) -> egui::Pos2 {
    number(harness, entry, word).scroll_to_me();
    harness.run_steps(3);
    number(harness, entry, word).rect().center()
}

fn shows(harness: &Window<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// A place on each empty chart: where it says so.
fn empty(harness: &Window<'_>) -> Vec<egui::Pos2> {
    harness.query_all_by_label("Drag a number here.").map(|hint| hint.rect().center()).collect()
}

/// Type a number into one of the boxes above the charts.
fn set(harness: &mut Window<'_>, which: usize, value: f64) {
    harness.query_all_by_role(Role::SpinButton).nth(which).expect("a number box").set_value_accesskit(value);
    harness.run_steps(3);
}

#[test]
fn numbers_of_connections_are_dragged_onto_charts() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (filtered, viewed) = (format!("{robot}/smoother/raw"), format!("{desk}/viewer/pose"));
    let history = history(&scene)?;
    let now = history.newest().expect("points");
    let shown = Shown { charts: Charts::default(), history, signals: Signals::start()?, now };
    let mut harness = Harness::builder().with_size(egui::vec2(1300.0, 800.0)).wgpu().build_ui_state(
        |ui, shown: &mut Shown| {
            egui::Panel::top("controls").show(ui, |ui| ui.horizontal(|ui| shown.charts.controls(ui)));
            let recorded = Recorded { history: &shown.history, signals: &shown.signals };
            shown.charts.show(ui, recorded, shown.now);
        },
        shown,
    );
    harness.run_steps(3);

    // The list holds every connection, with where it comes from and its numbers as they stand.
    for label in [filtered.clone(), format!("from {robot}/imu/pose"), viewed.clone()] {
        assert!(shows(&harness, &label), "{label:?} is listed");
    }
    assert!(number(&harness, &filtered, "rate").parent().unwrap().query_by_label("40.0/s").is_some());
    // Every output is listed too, with how fast it sends: a node's inputs and outputs stand together.
    let sensor = format!("{robot}/imu/pose");
    assert!(number(&harness, &sensor, "rate").parent().unwrap().query_by_label("40.0/s").is_some());
    let names = |harness: &Window<'_>| -> Vec<f32> {
        let listed = [&sensor, &filtered, &format!("{robot}/smoother/smooth"), &viewed];
        listed.map(|name| harness.get_by_label(name.as_str()).rect().top()).to_vec()
    };
    assert!(names(&harness).is_sorted(), "in order of their names");
    assert_eq!(empty(&harness).len(), 1, "one chart to begin with, with nothing on it");

    // Drag the rate onto the chart: it is drawn there, and named.
    let (rate, chart) = (grab(&mut harness, &filtered, "rate"), empty(&harness)[0]);
    drag(&mut harness, rate, chart);
    let line = format!("{filtered} rate");
    assert!(shows(&harness, &line), "the chart names what it draws");
    assert!(empty(&harness).is_empty());
    // Top and bottom are the largest and smallest value in view: the last thirty seconds hold them all.
    assert!(
        shows(&harness, "80.0") && shows(&harness, "20.0") && shows(&harness, "30 s ago") && shows(&harness, "now")
    );

    // With only the last five seconds in view, the 80 of six seconds ago is not among them:
    // the line comes in at the left edge at 70, on its way down to 60.
    set(&mut harness, 0, 5.0);
    assert!(shows(&harness, "70.0") && shows(&harness, "20.0") && shows(&harness, "5 s ago"));
    assert!(!shows(&harness, "80.0"));

    // A chart ends at the time it is told, not at its newest point. Three
    // seconds on, the line has slid left: what is left in view runs from 20 to 40.
    harness.state_mut().now += 3.0;
    harness.run_steps(2);
    assert!(shows(&harness, "40.0") && shows(&harness, "20.0") && !shows(&harness, "70.0"));
    harness.state_mut().now -= 3.0;

    // A second number on the same chart: the chart reaches down to take it in.
    set(&mut harness, 0, 30.0);
    let lost = grab(&mut harness, &filtered, "lost");
    drag(&mut harness, lost, chart);
    assert!(shows(&harness, &line) && shows(&harness, &format!("{filtered} lost")));
    assert!(shows(&harness, "80.0") && shows(&harness, "0.00"), "nothing was lost, so the bottom is zero");
    drag(&mut harness, lost, chart);
    assert_eq!(harness.query_all_by_label(format!("{filtered} lost").as_str()).count(), 1, "once is enough");
    // How fast an output sends is drawn like any other number.
    let sent = grab(&mut harness, &sensor, "rate");
    drag(&mut harness, sent, chart);
    assert!(shows(&harness, &format!("{sensor} rate")));

    // What is inside its messages is listed once the output is opened: from then on the topic is read.
    assert!(!shows(&harness, "translation[0]") && scene.imu.pose.subscribers() == 1);
    harness.get_by_label(sensor.as_str()).parent().unwrap().get_by_label("numbers inside").click();
    let listed = |harness: &Window<'_>, value: &str| {
        let entry = harness.get_by_label(sensor.as_str()).parent().unwrap();
        let number = entry.query_by_label("translation[0]").and_then(|number| number.parent());
        number.is_some_and(|number| number.query_by_label(value).is_some())
    };
    let read = wait_for(|| {
        scene.imu.pose.send(Pose { translation: [2.5, 0.0, 0.0], ..Pose::default() }).expect("send");
        harness.step();
        listed(&harness, "2.50")
    });
    assert!(read, "the number is listed as it stands");
    assert_eq!(scene.imu.pose.subscribers(), 2);
    // A number from inside a message is dragged onto a chart like the others, and drawn from then on.
    let inside = grab(&mut harness, &sensor, "translation[0]");
    drag(&mut harness, inside, chart);
    let line = format!("{sensor} translation[0]");
    assert!(shows(&harness, &line));
    let drawn = wait_for(|| {
        scene.imu.pose.send(Pose { translation: [7.25, 0.0, 0.0], ..Pose::default() }).expect("send");
        harness.step();
        harness.get_by_label(line.as_str()).parent().unwrap().query_by_label("7.25").is_some()
    });
    assert!(drawn, "the chart says where the line is now");
    let line = format!("{filtered} rate");

    // More charts, one under the other or side by side, each with its own numbers.
    harness.get_by_label("Add chart").click();
    harness.run_steps(3);
    let below = empty(&harness)[0];
    assert!(below.y > chart.y && (below.x - chart.x).abs() < 1.0, "the new chart is under the first");
    set(&mut harness, 1, 2.0);
    let beside = empty(&harness)[0];
    assert!(beside.x > chart.x && (beside.y - chart.y).abs() < 1.0, "side by side now");
    let latency = grab(&mut harness, &viewed, "latency");
    drag(&mut harness, latency, beside);
    assert!(shows(&harness, &format!("{viewed} latency")) && empty(&harness).is_empty());

    // A picture for a human to look at; and the first line is on it, in its colour.
    harness.remove_cursor();
    harness.run_steps(3);
    match harness.render() {
        Ok(picture) => {
            let saved = picture.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("charts.png"));
            saved.map_err(|error| triq::Error::Other(error.to_string()))?;
            let ink = charts::color(0, harness.ctx.global_style().visuals.dark_mode).to_array();
            let inked = picture.pixels().filter(|pixel| (0..3).all(|part| pixel.0[part].abs_diff(ink[part]) < 12));
            assert!(inked.count() > 300, "a line is drawn in the first colour");
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // A line is taken off with the button beside its name, and a chart with its own.
    let name = harness.get_by_label(line.as_str());
    name.parent().expect("the names on a chart").query_all_by_label("×").next().expect("a button").click();
    harness.run_steps(3);
    assert!(!shows(&harness, &line) && shows(&harness, &format!("{filtered} lost")));
    harness.query_all_by_label("Remove chart").last().expect("a button").click();
    harness.run_steps(3);
    assert!(!shows(&harness, &format!("{viewed} latency")), "the second chart is gone");
    assert!(!shows(&harness, "Remove chart"), "and the last chart stays");
    Ok(())
}

#[test]
fn what_comes_with_every_message_is_drawn_up_to_the_present() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    let (sensor, filtered) = (format!("{robot}/imu/pose"), format!("{robot}/smoother/raw"));
    // What the runtimes report: 20 samples a second, then 80, read at two seconds and at four.
    let mut history = History::default();
    let mut reading = scene.inventories();
    history.record(&reading);
    for samples in [40, 160] {
        for _ in 0..samples {
            scene.imu.pose.send(Pose::default())?;
            assert!(wait_for(|| scene.viewer.pose.recv().expect("recv").is_some()));
        }
        reading = later(&reading, scene.inventories(), [Duration::from_secs(2); 2]);
        history.record(&reading);
    }
    let reported = history.newest().expect("points");
    let mut charts = Charts::default();
    let connection = Connection { to: filtered, from: sensor.clone() };
    charts.add(0, Trace::Heard { connection, stat: Stat::Rate });
    let shown = Shown { charts, history, signals: Signals::start()?, now: reported + 0.05 };
    let mut harness = Harness::builder().with_size(egui::vec2(1300.0, 800.0)).wgpu().build_ui_state(
        |ui, shown: &mut Shown| {
            let recorded = Recorded { history: &shown.history, signals: &shown.signals };
            shown.charts.show(ui, recorded, shown.now);
        },
        shown,
    );
    harness.run_steps(3);

    // A report comes every tenth of a second and takes a moment to arrive. So its line is
    // drawn a little behind the present, where it always has a point to run on to: a
    // twentieth of a second after the newest reading, the line is on its way up to it.
    assert!(shows(&harness, "77.0") && !shows(&harness, "80.0"));
    harness.state_mut().now = reported + 0.2;
    harness.run_steps(2);
    assert!(shows(&harness, "80.0"));

    // A number inside a message comes with the message, and is drawn as soon as it is there.
    let height = "translation[1]";
    harness.state_mut().charts = Charts::default();
    harness.state_mut().charts.add(0, Trace::Inside { topic: sensor.clone(), number: height.to_owned() });
    let pose = |y: f32| Pose { translation: [0.0, y, 0.0], ..Pose::default() };
    let read = wait_for(|| {
        scene.imu.pose.send(pose(1.0)).expect("send");
        harness.step();
        harness.state().signals.latest(&sensor, height) == Some(1.0)
    });
    assert!(read, "a number on a chart is read");
    scene.imu.pose.send(pose(5.0))?;
    assert!(wait_for(|| harness.state().signals.latest(&sensor, height) == Some(5.0)));
    let sent = harness.state().signals.line(&sensor, height, 0.0, f64::MAX).last().expect("its point").0;
    harness.state_mut().now = sent + 0.05;
    harness.run_steps(2);
    assert!(shows(&harness, "5.00"), "a twentieth of a second after it was sent, the chart reaches up to it");
    Ok(())
}

#[test]
fn a_chart_reaches_from_the_smallest_value_to_the_largest() {
    assert_eq!(charts::extent([60.0, 80.0, 20.0, 40.0].into_iter()), Some((20.0, 80.0)));
    assert_eq!(charts::extent([-3.0].into_iter()), Some((-3.0, -2.0)), "a flat line lies along the bottom");
    assert_eq!(charts::extent([7.0, 7.0].into_iter()), Some((7.0, 8.0)));
    assert_eq!(charts::extent(std::iter::empty()), None);
    // Three figures or so.
    assert_eq!([charts::written(121.7), charts::written(59.96), charts::written(8.333)], ["122", "60.0", "8.33"]);
}

#[test]
fn a_line_with_more_points_than_pixels_is_thinned_and_looks_the_same() {
    // Ten thousand points across a hundred pixels, going up and down.
    let line: Vec<egui::Pos2> = (0..10_000).map(|i| egui::pos2(i as f32 / 100.0, ((i * 37) % 211) as f32)).collect();
    let thin = charts::thinned(&line);
    assert!(thin.len() <= 400, "at most four points for each pixel across, not {}", thin.len());
    assert_eq!((thin[0], thin[thin.len() - 1]), (line[0], line[line.len() - 1]), "it starts and ends where it did");
    assert!(thin.windows(2).all(|pair| pair[0].x <= pair[1].x), "and is still in order of time");
    // In every pixel column it reaches as high and as low as before.
    for column in 0..100 {
        let reach = |points: &[egui::Pos2]| {
            let heights = points.iter().filter(|point| point.x.floor() == column as f32).map(|point| point.y);
            heights.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), y| (low.min(y), high.max(y)))
        };
        assert_eq!(reach(&thin), reach(&line), "column {column}");
    }

    // A line with room between its points is left as it is.
    let sparse: Vec<egui::Pos2> = (0..50).map(|i| egui::pos2(i as f32 * 3.0, (i % 7) as f32)).collect();
    assert_eq!(charts::thinned(&sparse), sparse);
}
