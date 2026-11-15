//! The second tab: numbers of connections dragged onto charts and drawn over time.
//!
//! The charts are drawn from a history made here, with known numbers at known
//! times, so what a chart must show is known exactly.
#[path = "support/scene.rs"]
mod scene;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use scene::*;
use std::time::Duration;
use triq_workbench::charts::{self, Charts};
use triq_workbench::egui::{self, PointerButton, accesskit::Role};
use triq_workbench::history::History;

struct Shown {
    charts: Charts,
    history: History,
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

/// Press on something, move to somewhere else in a few steps, and let go.
fn drag(harness: &mut Window<'_>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.run_steps(2);
    harness.event(button(from, true));
    harness.run_steps(2);
    for step in 1..=6 {
        harness.hover_at(from + (to - from) * (step as f32 / 6.0));
        harness.run_steps(1);
    }
    harness.event(button(to, false));
    harness.run_steps(3);
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
    let shown = Shown { charts: Charts::default(), history: history(&scene)? };
    let mut harness = Harness::builder().with_size(egui::vec2(1300.0, 800.0)).wgpu().build_ui_state(
        |ui, shown: &mut Shown| {
            egui::Panel::top("controls").show(ui, |ui| ui.horizontal(|ui| shown.charts.controls(ui)));
            shown.charts.show(ui, &shown.history);
        },
        shown,
    );
    harness.run_steps(3);

    // The list holds every connection, with where it comes from and its numbers as they stand.
    for label in [filtered.clone(), format!("from {robot}/imu/pose"), viewed.clone()] {
        assert!(shows(&harness, &label), "{label:?} is listed");
    }
    assert!(number(&harness, &filtered, "rate").parent().unwrap().query_by_label("40.0/s").is_some());
    assert_eq!(empty(&harness).len(), 1, "one chart to begin with, with nothing on it");

    // Drag the rate onto the chart: it is drawn there, and named.
    let (rate, chart) = (number(&harness, &filtered, "rate").rect().center(), empty(&harness)[0]);
    drag(&mut harness, rate, chart);
    let line = format!("{filtered} rate");
    assert!(shows(&harness, &line), "the chart names what it draws");
    assert!(empty(&harness).is_empty());
    // Top and bottom are the largest and smallest value in view: the last thirty seconds hold them all.
    assert!(
        shows(&harness, "80.0") && shows(&harness, "20.0") && shows(&harness, "30 s ago") && shows(&harness, "now")
    );

    // With only the last five seconds in view, the 80 of eight seconds ago is not among them.
    set(&mut harness, 0, 5.0);
    assert!(shows(&harness, "60.0") && shows(&harness, "20.0") && shows(&harness, "5 s ago"));
    assert!(!shows(&harness, "80.0"));

    // A second number on the same chart: the chart reaches down to take it in.
    set(&mut harness, 0, 30.0);
    let lost = number(&harness, &filtered, "lost").rect().center();
    drag(&mut harness, lost, chart);
    assert!(shows(&harness, &line) && shows(&harness, &format!("{filtered} lost")));
    assert!(shows(&harness, "80.0") && shows(&harness, "0.00"), "nothing was lost, so the bottom is zero");
    drag(&mut harness, lost, chart);
    assert_eq!(harness.query_all_by_label(format!("{filtered} lost").as_str()).count(), 1, "once is enough");

    // More charts, one under the other or side by side, each with its own numbers.
    harness.get_by_label("Add chart").click();
    harness.run_steps(3);
    let below = empty(&harness)[0];
    assert!(below.y > chart.y && (below.x - chart.x).abs() < 1.0, "the new chart is under the first");
    set(&mut harness, 1, 2.0);
    let beside = empty(&harness)[0];
    assert!(beside.x > chart.x && (beside.y - chart.y).abs() < 1.0, "side by side now");
    let latency = number(&harness, &viewed, "latency").rect().center();
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
