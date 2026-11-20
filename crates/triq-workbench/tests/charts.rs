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
use window::{alone, click, click_holding, drag, drag_by_way_of, strokes};

struct Shown {
    charts: Charts,
    history: History,
    signals: Signals,
    /// The time at the right edge of every chart.
    now: f64,
}

type Window<'a> = Harness<'a, Shown>;

/// As the workbench kept its charts when they all showed the same stretch of the past, and drew
/// nothing but lines over time: one chart, with two lines. The second was given a colour by hand.
const KEPT: &str = r#"{"panes":{"id":14635840284022990772,"root":2,"tiles":{"next_tile_id":3,"tiles":{
    "1":{"Pane":{"number":1,"lines":[
        {"trace":{"Heard":{"connection":{"to":"robot/smoother/raw","from":"robot/imu/pose"},"stat":"Rate"}},
         "shade":0,"chosen":null},
        {"trace":{"Inside":{"topic":"robot/imu/pose","number":"translation[0]"}},"shade":1,"chosen":[255,0,255,255]}]}},
    "2":{"Container":{"Tabs":{"children":[1],"active":1}}}},"invisible":[]},"height":null,"width":null},
    "made":1,"seconds":45.0,"listed":true}"#;

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

/// Type into the box on a chart that says how much of the past it shows.
fn show_the_last(harness: &mut Window<'_>, chart: &str, seconds: f64) {
    let room = room(harness, chart);
    let past = harness.query_all_by_role(Role::SpinButton).find(|number| room.contains(number.rect().center()));
    past.expect("a box on the chart").set_value_accesskit(seconds);
    harness.run_steps(3);
}

/// Whether something is written on a chart.
fn written_on(harness: &Window<'_>, chart: &str, label: &str) -> bool {
    let room = room(harness, chart);
    harness.query_all_by_label(label).any(|written| room.contains(written.rect().center()))
}

/// The charts in a window. The list holds every topic on the machine, and they are first
/// read a moment after the start: the window opens after that, so that the topics of other
/// programs do not come into the list, and move its rows, under a pointer that took aim.
fn open(shown: Shown) -> Window<'static> {
    assert!(wait_for(|| !shown.signals.topics().is_empty()), "the topics of the machine are listed");
    let mut harness = Harness::builder().with_size(egui::vec2(1300.0, 800.0)).wgpu().build_ui_state(
        |ui, shown: &mut Shown| {
            let recorded = Recorded { history: &shown.history, signals: &shown.signals };
            shown.charts.show(ui, recorded, shown.now);
        },
        shown,
    );
    harness.run_steps(3);
    harness
}

/// Where the name of a chart is, on the bar above it: the thing to take a chart by.
fn name(harness: &Window<'_>, chart: &str) -> egui::Rect {
    harness.get_by_role_and_label(Role::Button, chart).rect()
}

/// The name of a chart on the bar above it: a click brings the chart forward.
fn name_of<'a>(harness: &'a Window<'_>, chart: &'a str) -> Node<'a> {
    harness.get_by_role_and_label(Role::Button, chart)
}

/// Where a chart is drawn, under its bar.
fn room(harness: &Window<'_>, chart: &str) -> egui::Rect {
    harness.get_by_role_and_label(Role::Canvas, chart).rect()
}

/// Press the button that adds a chart.
fn add(harness: &mut Window<'_>) {
    harness.get_by_label("New chart").click();
    harness.run_steps(3);
}

/// Take a chart by its name and put it down at a place.
fn put(harness: &mut Window<'_>, chart: &str, at: egui::Pos2) {
    let name = name(harness, chart);
    drag(harness, name.center(), at);
    harness.run_steps(10);
}

/// The middle of one half of where a chart is drawn: `side` points from the chart's middle to that half.
fn half(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    let room = room(harness, chart);
    room.center() + side * room.size() / 4.0
}

/// Where to put a chart down to make it the neighbour of another on that side: high on the
/// half, where the highlight takes in the bar above the chart as well.
fn beside(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    half(harness, chart, side) - egui::vec2(0.0, 30.0)
}

/// Where to put a chart down to have it go in with another, as a group of the two: low on the
/// half, where the highlight is all under the bar.
fn within(harness: &Window<'_>, chart: &str, side: egui::Vec2) -> egui::Pos2 {
    half(harness, chart, side) + egui::vec2(0.0, 30.0)
}

/// Press the cross beside the name of a chart.
fn close(harness: &mut Window<'_>, chart: &str) {
    let name = name(harness, chart);
    let cross = harness.query_all_by_label("Close").find(|cross| name.contains(cross.rect().center()));
    cross.expect("a cross beside the name").click();
    harness.run_steps(3);
}

#[test]
fn numbers_of_connections_are_dragged_onto_charts() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let (filtered, viewed) = (format!("{robot}/smoother/raw"), format!("{desk}/viewer/pose"));
    let history = history(&scene)?;
    let now = history.newest().expect("points");
    let mut harness = open(Shown { charts: Charts::default(), history, signals: Signals::start()?, now });

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
    show_the_last(&mut harness, "Chart 1", 5.0);
    assert!(shows(&harness, "70.0") && shows(&harness, "20.0") && shows(&harness, "5 s ago"));
    assert!(!shows(&harness, "80.0"));

    // A chart ends at the time it is told, not at its newest point. Three
    // seconds on, the line has slid left: what is left in view runs from 20 to 40.
    harness.state_mut().now += 3.0;
    harness.run_steps(2);
    assert!(shows(&harness, "40.0") && shows(&harness, "20.0") && !shows(&harness, "70.0"));
    harness.state_mut().now -= 3.0;

    // A second number on the same chart: the chart reaches down to take it in.
    show_the_last(&mut harness, "Chart 1", 30.0);
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

    // More charts, each with its own numbers: a new chart appears beside the first.
    add(&mut harness);
    let beside = empty(&harness)[0];
    assert!(beside.x > chart.x && (beside.y - chart.y).abs() < 1.0, "the new chart is beside the first");
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

    // A line is taken off with the button beside its name, and a chart with the cross beside its own.
    let named = harness.get_by_label(line.as_str());
    named.parent().expect("the names on a chart").query_all_by_label("×").next().expect("a button").click();
    harness.run_steps(3);
    assert!(!shows(&harness, &line) && shows(&harness, &format!("{filtered} lost")));
    close(&mut harness, "Chart 2");
    assert!(!shows(&harness, &format!("{viewed} latency")), "the second chart is gone");
    assert!(!shows(&harness, "Close"), "and the last chart stays");
    Ok(())
}

/// A window for arranging charts in. The scene was read twice while nothing
/// happened in it: enough to put numbers on charts, though every line is flat.
fn arranging(scene: &Scene, charts: Charts) -> triq::Result<Window<'static>> {
    let (mut history, reading) = (History::default(), scene.inventories());
    history.record(&reading);
    history.record(&later(&reading, scene.inventories(), [Duration::from_secs(2); 2]));
    let now = history.newest().expect("a point");
    Ok(open(Shown { charts, history, signals: Signals::start()?, now }))
}

/// The lines on the charts by name, row by row from the top, and from left to right in a row.
fn lines(harness: &Window<'_>, scene: &Scene) -> Vec<String> {
    // The list on the left names the same things. It is 300 points wide.
    let named = harness.query_all_by_label_contains(scene.robot.name());
    let mut drawn: Vec<_> = named.filter(|line| line.rect().left() > 320.0).collect();
    drawn.sort_by(|a, b| (a.rect().top(), a.rect().left()).partial_cmp(&(b.rect().top(), b.rect().left())).unwrap());
    drawn.iter().filter_map(|line| line.value()).collect()
}

/// Three charts: the first above, the second and third side by side below it. Each was added,
/// and put down where it is.
fn three(harness: &mut Window<'_>) {
    add(harness);
    let under = beside(harness, "Chart 1", egui::vec2(0.0, 1.0));
    put(harness, "Chart 2", under);
    add(harness);
    let right = beside(harness, "Chart 2", egui::vec2(1.0, 0.0));
    put(harness, "Chart 3", right);
}

#[test]
fn charts_are_added_put_where_they_should_be_resized_and_closed() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let (rate, lost, gap) = (format!("{filtered} rate"), format!("{filtered} lost"), format!("{filtered} gap"));
    let mut harness = arranging(&scene, Charts::default())?;

    // One chart to begin with, which cannot be closed. A new chart appears beside it.
    assert!(shows(&harness, "Chart 1") && !shows(&harness, "Close"));
    add(&mut harness);
    let (first, second) = (name(&harness, "Chart 1"), name(&harness, "Chart 2"));
    assert!(second.left() > first.left() + 300.0 && second.top() == first.top(), "beside the first");
    // Taken by its name and put down on the lower half of the first, it is under it.
    let under = beside(&harness, "Chart 1", egui::vec2(0.0, 1.0));
    put(&mut harness, "Chart 2", under);
    let (first, second) = (name(&harness, "Chart 1"), name(&harness, "Chart 2"));
    assert!(second.top() > first.top() + 300.0 && (second.left() - first.left()).abs() < 1.0, "under the first");
    // The next is put down on the right half of the second: panes within panes.
    add(&mut harness);
    let right = beside(&harness, "Chart 2", egui::vec2(1.0, 0.0));
    put(&mut harness, "Chart 3", right);
    assert!(!shows(&harness, "Charts 2, 3") && !shows(&harness, "Charts 1, 2, 3"), "neighbours, and no group");
    let [first, second, third] = ["Chart 1", "Chart 2", "Chart 3"].map(|chart| name(&harness, chart));
    assert!(third.left() > second.left() + 300.0 && (third.top() - second.top()).abs() < 1.0, "beside the second");
    assert!(second.top() > first.top() + 300.0 && (second.left() - first.left()).abs() < 1.0, "the first above both");
    assert!(room(&harness, "Chart 1").width() > room(&harness, "Chart 2").width() * 1.9, "with the row to itself");

    // A number lands on the chart it is put down on, and no chart moves for it.
    for (number, chart) in [("gap", "Chart 1"), ("rate", "Chart 3"), ("lost", "Chart 2")] {
        let (from, chart) = (grab(&mut harness, &filtered, number), room(&harness, chart).center());
        drag(&mut harness, from, chart);
    }
    assert_eq!(lines(&harness, &scene), [gap.clone(), lost.clone(), rate.clone()]);
    assert_eq!(["Chart 1", "Chart 2", "Chart 3"].map(|chart| name(&harness, chart)), [first, second, third]);

    // The line between two charts is dragged, to give one more room and the other less.
    let border = egui::pos2(second.center().x, second.top() - 1.0);
    drag(&mut harness, border, border + egui::vec2(0.0, 100.0));
    let lower = name(&harness, "Chart 2").top();
    assert!((lower - border.y - 100.0).abs() < 6.0, "the lower charts begin where the line was let go");
    // However far it is dragged, a chart keeps a little room, and is drawn in it.
    let border = egui::pos2(second.center().x, lower - 1.0);
    drag(&mut harness, border, border + egui::vec2(0.0, 2000.0));
    let least = name(&harness, "Chart 2").top();
    assert!(least > lower + 100.0 && least < 800.0 && shows(&harness, &lost));
    drag(&mut harness, egui::pos2(border.x, least - 1.0), border);

    // A chart is closed by the cross beside its name, and its neighbour has the room.
    close(&mut harness, "Chart 2");
    assert_eq!(lines(&harness, &scene), [gap.clone(), rate.clone()]);
    assert!((name(&harness, "Chart 3").left() - first.left()).abs() < 1.0, "the third begins at the left now");
    close(&mut harness, "Chart 1");
    assert_eq!(lines(&harness, &scene), [rate]);
    assert!(shows(&harness, "Chart 3") && !shows(&harness, "Close"), "the last chart stays");
    Ok(())
}

#[test]
fn a_chart_is_put_down_beside_another_or_in_its_place() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let robot = scene.robot.name();
    let (sensor, filtered) = (format!("{robot}/imu/pose"), format!("{robot}/smoother/raw"));
    let (rate, lost, gap) = (format!("{filtered} rate"), format!("{filtered} lost"), format!("{filtered} gap"));
    // A number from inside a message is on the first chart from the start: the topic is read for it.
    let (mut charts, inside) = (Charts::default(), format!("{sensor} translation[0]"));
    charts.add(1, Trace::Inside { topic: sensor.clone(), number: "translation[0]".to_owned() });
    let mut harness = arranging(&scene, charts)?;
    let read = wait_for(|| {
        harness.step();
        scene.imu.pose.subscribers() == 2
    });
    assert!(read && shows(&harness, &inside));

    // Three charts: the first above, the second and third side by side below it, each with a line.
    three(&mut harness);
    for (number, chart) in [("lost", "Chart 2"), ("rate", "Chart 3")] {
        let (from, chart) = (grab(&mut harness, &filtered, number), room(&harness, chart).center());
        drag(&mut harness, from, chart);
    }
    assert_eq!(lines(&harness, &scene), [inside.clone(), lost.clone(), rate.clone()]);
    let names = |harness: &Window<'_>| ["Chart 1", "Chart 2", "Chart 3"].map(|chart| name(harness, chart));
    let [_, _, third] = names(&harness);

    // A chart is taken by its name and put down elsewhere. On the left half of another, it becomes
    // that chart's left neighbour. Its lines go with it, and what it passes over gains none.
    let left_half = beside(&harness, "Chart 1", egui::vec2(-1.0, 0.0));
    drag(&mut harness, third.center(), left_half);
    harness.run_steps(10);
    let [first, second, third] = names(&harness);
    assert!(third.left() < first.left() && (third.top() - first.top()).abs() < 1.0, "to the left of the first");
    assert!(second.top() > first.bottom() + 100.0, "the second is still below, with the row to itself");
    assert_eq!(lines(&harness, &scene), [rate.clone(), inside.clone(), lost.clone()]);
    // A number is put down on it where it is now.
    let (from, onto) = (grab(&mut harness, &filtered, "gap"), room(&harness, "Chart 3").center());
    drag(&mut harness, from, onto);
    assert_eq!(lines(&harness, &scene), [rate.clone(), inside.clone(), gap.clone(), lost.clone()]);

    // Put down on the middle of another chart, a chart shares its place, and is the one in front.
    let middle = egui::pos2(first.left() + (first.left() - third.left()) / 2.0, (first.bottom() + second.top()) / 2.0);
    drag(&mut harness, second.center(), middle);
    harness.run_steps(10);
    let [first, second, _] = names(&harness);
    assert!(second.left() >= first.right() && (second.top() - first.top()).abs() < 1.0, "their names are side by side");
    assert_eq!(lines(&harness, &scene), [rate.clone(), lost.clone(), gap.clone()]);
    // The chart behind is still read for, so its lines go on from where they were.
    harness.run_steps(5);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(scene.imu.pose.subscribers(), 2);
    // Its name brings it forward.
    name_of(&harness, "Chart 1").click();
    harness.run_steps(3);
    assert_eq!(lines(&harness, &scene), [rate.clone(), inside.clone(), gap.clone()]);
    // So does a number that is held over its name, to be put down on it.
    let (from, second) = (grab(&mut harness, &filtered, "latency"), name(&harness, "Chart 2"));
    drag_by_way_of(&mut harness, &[from, second.center(), second.left_bottom() + egui::vec2(150.0, 150.0)]);
    let latency = format!("{filtered} latency");
    assert_eq!(lines(&harness, &scene), [rate.clone(), lost.clone(), gap.clone(), latency.clone()]);

    // A place is taken by the bar its names are on, with every chart in it. Put down on the middle
    // of another chart, it is one name on that chart's bar, and has its own bar under that still.
    let [first, second, third] = names(&harness);
    let (bar, middle) =
        (second.right_center() + egui::vec2(60.0, 0.0), egui::pos2((third.left() + first.left()) / 2.0, 430.0));
    drag(&mut harness, bar, middle);
    harness.run_steps(10);
    let [first, second, third] = names(&harness);
    let both = name(&harness, "Charts 1, 2");
    assert!(third.right() <= both.left() && both.top() == third.top(), "the third and the two, on one bar");
    assert!(first.top() > both.bottom() - 1.0 && second.top() == first.top() && first.right() <= second.left());
    assert_eq!(lines(&harness, &scene), [lost.clone(), latency.clone()], "the one that was in front still is");

    // A chart that is closed is no longer read for. What is left of the two is a chart beside the third.
    close(&mut harness, "Chart 1");
    assert!(
        !shows(&harness, "Chart 1") && !shows(&harness, "Charts 1, 2") && lines(&harness, &scene) == [lost, latency]
    );
    assert_eq!(name(&harness, "Chart 2").top(), name(&harness, "Chart 3").top(), "one bar again");
    let unread = wait_for(|| {
        harness.step();
        scene.imu.pose.subscribers() == 1
    });
    assert!(unread);
    Ok(())
}

/// The colour a line on a chart is drawn in, as the button beside its name says it: `#rrggbbaa`.
fn colour(harness: &Window<'_>, line: &str) -> String {
    let names = harness.get_by_label(line).parent().expect("the name's row");
    names.get_by_role(Role::ColorWell).value().expect("a colour")
}

#[test]
fn the_lines_have_the_whole_chart_and_each_a_colour_that_can_be_changed() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (sensor, filtered) =
        (format!("{}/imu/pose", scene.robot.name()), format!("{}/smoother/raw", scene.robot.name()));
    // All four numbers of the filter's connection are on the chart from the start.
    let mut charts = Charts::default();
    for stat in Stat::ALL {
        charts.add(1, Trace::Heard { connection: Connection { to: filtered.clone(), from: sensor.clone() }, stat });
    }
    let history = history(&scene)?;
    let now = history.newest().expect("points");
    let mut harness = open(Shown { charts, history, signals: Signals::start()?, now });
    let [rate, lost, gap, latency] = Stat::ALL.map(|stat| format!("{filtered} {}", stat.name()));

    // The names are written over the chart, not above it, so the lines have all of its height
    // however many there are. The largest value in view is at the very top, beside what is written there.
    let place = |harness: &Window<'_>, label: &str| harness.get_by_label(label).rect();
    let (first, last, top) = (place(&harness, &rate), place(&harness, &latency), place(&harness, "80.0"));
    assert!(last.top() > first.bottom(), "the names are one under another");
    assert!(top.top() < first.bottom() && top.left() > first.right(), "the top of the chart is beside the first name");

    // Every line has a colour of its own, shown beside its name.
    let given = [&rate, &lost, &gap, &latency].map(|line| colour(&harness, line));
    assert!((0..4).all(|a| (0..a).all(|b| given[a] != given[b])), "four different colours: {given:?}");

    // The button that shows a colour opens a picker. The rate is made magenta.
    let boxes = |harness: &Window<'_>| -> Vec<egui::Rect> {
        let mut boxes: Vec<_> = harness.query_all_by_role(Role::SpinButton).map(|number| number.rect()).collect();
        boxes.sort_by(|a, b| a.left().total_cmp(&b.left()));
        boxes
    };
    let before = boxes(&harness);
    // The picker has a button with a pictogram on it. A window borrows a font from the system
    // to draw it with; the tests have none that can, and are told not to mind.
    harness.ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    harness.get_by_label(rate.as_str()).parent().unwrap().get_by_role(Role::ColorWell).click();
    harness.run_steps(3);
    let parts: Vec<egui::Rect> = boxes(&harness).into_iter().filter(|part| !before.contains(part)).collect();
    assert_eq!(parts.len(), 3, "red, green and blue");
    for (part, value) in parts.iter().zip([255.0, 0.0, 255.0]) {
        let part = harness.query_all_by_role(Role::SpinButton).find(|number| number.rect() == *part);
        part.expect("the box").set_value_accesskit(value);
        harness.run_steps(2);
    }
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert_eq!(colour(&harness, &rate), "#ff00ffff");
    assert_eq!([&lost, &gap, &latency].map(|line| colour(&harness, line)), given[1..], "the others keep theirs");

    // The line is drawn in it, right up to the top of the chart: behind the names. Looking takes a GPU.
    harness.remove_cursor();
    harness.run_steps(3);
    match harness.render() {
        Ok(picture) => {
            let saved = picture.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("chart-colours.png"));
            saved.map_err(|error| triq::Error::Other(error.to_string()))?;
            // How many pixels above some row are magenta, or nearly.
            let inked = |above: u32| {
                let magenta = |pixel: [u8; 4]| pixel[0] > 243 && pixel[1] < 12 && pixel[2] > 243;
                picture.enumerate_pixels().filter(|(_, row, pixel)| *row < above && magenta(pixel.0)).count()
            };
            let names_end = (place(&harness, &latency).bottom() * harness.ctx.pixels_per_point()) as u32;
            assert!(inked(picture.height()) > 300, "the rate is magenta");
            assert!(inked(names_end) > 20, "also where the names are");
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // A line that is taken off gives its colour back, and the next line to be put on takes it.
    let named = harness.get_by_label(lost.as_str());
    named.parent().unwrap().query_all_by_label("×").next().expect("a button").click();
    harness.run_steps(3);
    assert!(!shows(&harness, &lost));
    let (from, chart) =
        (grab(&mut harness, &filtered, "lost"), place(&harness, "80.0").center() - egui::vec2(300.0, -200.0));
    drag(&mut harness, from, chart);
    assert_eq!(colour(&harness, &lost), given[1]);
    assert_eq!(colour(&harness, &rate), "#ff00ffff", "and a colour that was chosen stays");
    Ok(())
}

#[test]
fn anything_goes_into_anything() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let (rate, lost) = (format!("{filtered} rate"), format!("{filtered} lost"));
    let mut harness = arranging(&scene, Charts::default())?;
    let (left, right, down) = (egui::vec2(-1.0, 0.0), egui::vec2(1.0, 0.0), egui::vec2(0.0, 1.0));
    let low = egui::vec2(0.0, 40.0);

    // A second chart is put down inside the first, on its right half. The two are a group: a bar
    // that names both, and under it the two side by side, each under its own bar still.
    add(&mut harness);
    let inside = within(&harness, "Chart 1", right);
    put(&mut harness, "Chart 2", inside);
    let [group, first, second] = ["Charts 1, 2", "Chart 1", "Chart 2"].map(|chart| name(&harness, chart));
    assert!(first.top() > group.bottom() - 1.0 && second.top() == first.top(), "the group's bar is above theirs");
    assert!((group.left() - first.left()).abs() < 1.0 && second.left() > first.left() + 300.0, "side by side");
    // A number lands on the chart it is put down on, also in a group.
    let (from, onto) = (grab(&mut harness, &filtered, "rate"), room(&harness, "Chart 2").center());
    drag(&mut harness, from, onto);
    assert_eq!(lines(&harness, &scene), [rate.as_str()]);

    // A group is taken by its name, with all that is in it, and is a group where it is put down.
    add(&mut harness);
    let beyond = beside(&harness, "Chart 3", right);
    put(&mut harness, "Charts 1, 2", beyond);
    let [third, group, first] = ["Chart 3", "Charts 1, 2", "Chart 1"].map(|chart| name(&harness, chart));
    assert!(third.right() < group.left() && third.top() == group.top(), "to the right of the third, its bar level");
    assert!(first.top() > group.bottom() - 1.0 && lines(&harness, &scene) == [rate.as_str()]);

    // Groups go into groups: the third is put down inside the second, under it.
    let inside = within(&harness, "Chart 2", down);
    put(&mut harness, "Chart 3", inside);
    let [outer, inner] = ["Charts 1, 2, 3", "Charts 2, 3"].map(|chart| name(&harness, chart));
    let [first, second, third] = ["Chart 1", "Chart 2", "Chart 3"].map(|chart| name(&harness, chart));
    assert!(first.top() > outer.bottom() - 1.0 && inner.top() == first.top(), "the inner group is beside the first");
    assert!(second.top() > inner.bottom() - 1.0 && (second.left() - inner.left()).abs() < 1.0);
    assert!(third.top() > second.top() + 100.0 && (third.left() - second.left()).abs() < 1.0, "the third under it");
    assert_eq!(lines(&harness, &scene), [rate.as_str()], "and the second kept its line");

    // Put down high on a half, a chart is a neighbour there and no group is made for it.
    add(&mut harness);
    let neighbour = beside(&harness, "Chart 1", left);
    put(&mut harness, "Chart 4", neighbour);
    let (fourth, first) = (name(&harness, "Chart 4"), name(&harness, "Chart 1"));
    assert!(fourth.right() < first.left() && fourth.top() == first.top() && shows(&harness, "Charts 4, 1, 2, 3"));
    assert!(!shows(&harness, "Charts 4, 1"), "the fourth is one more in the group the first was in");

    // Put down on the middle of a chart that has its place to itself, a chart shares the place:
    // two names on one bar, however low on the middle it was. A bar above that would say the same.
    add(&mut harness);
    let middle = room(&harness, "Chart 4").center() + low;
    put(&mut harness, "Chart 5", middle);
    let (fourth, fifth) = (name(&harness, "Chart 4"), name(&harness, "Chart 5"));
    assert!(fifth.left() >= fourth.right() && fifth.top() == fourth.top() && !shows(&harness, "Charts 4, 5"));
    assert!(shows(&harness, "Charts 4, 5, 1, 2, 3"));
    let (from, onto) = (grab(&mut harness, &filtered, "lost"), room(&harness, "Chart 5").center());
    drag(&mut harness, from, onto);
    assert_eq!(lines(&harness, &scene), [lost.clone(), rate.clone()]);

    // Put down low on the middle of a chart that shares its place, a chart goes in with that
    // chart alone: the two are one name on the bar, with a bar of their own under it.
    add(&mut harness);
    let inside = room(&harness, "Chart 5").center() + low;
    put(&mut harness, "Chart 6", inside);
    let [fourth, both, fifth, sixth] =
        ["Chart 4", "Charts 5, 6", "Chart 5", "Chart 6"].map(|chart| name(&harness, chart));
    assert!(fourth.right() <= both.left() && both.top() == fourth.top(), "the fourth, and the two, on one bar");
    assert!(fifth.top() > both.bottom() - 1.0 && sixth.top() == fifth.top() && fifth.right() <= sixth.left());
    assert!(shows(&harness, "Charts 4, 5, 6, 1, 2, 3") && lines(&harness, &scene) == [rate.as_str()]);
    // The chart behind keeps its line, and its name brings it forward.
    name_of(&harness, "Chart 5").click();
    harness.run_steps(3);
    assert_eq!(lines(&harness, &scene), [lost.clone(), rate.clone()]);

    // A group that is left with one chart is no group any more, and its bar goes.
    close(&mut harness, "Chart 6");
    assert!(!shows(&harness, "Charts 5, 6") && name(&harness, "Chart 5").top() == name(&harness, "Chart 4").top());
    close(&mut harness, "Chart 3");
    assert!(!shows(&harness, "Charts 2, 3") && shows(&harness, "Charts 4, 5, 1, 2"));
    assert_eq!(name(&harness, "Chart 2").top(), name(&harness, "Chart 1").top());

    // The cross beside the name of a group closes all that is in it, unless that is every chart there is.
    let crosses = |harness: &Window<'_>| harness.query_all_by_label("Close").count();
    assert_eq!(crosses(&harness), 4, "one for each chart, and none for the group they are all in");
    add(&mut harness);
    assert_eq!(crosses(&harness), 6);
    close(&mut harness, "Charts 4, 5, 1, 2");
    assert!(["Chart 4", "Chart 5", "Chart 1", "Chart 2"].iter().all(|chart| !shows(&harness, chart)));
    assert!(shows(&harness, "Chart 7") && crosses(&harness) == 0 && lines(&harness, &scene).is_empty());
    Ok(())
}

#[test]
fn the_list_of_numbers_is_hidden_to_give_the_charts_its_room() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let sensor = format!("{}/imu/pose", scene.robot.name());
    let mut harness = arranging(&scene, Charts::default())?;
    let readers = |harness: &mut Window<'_>, readers: usize| {
        wait_for(|| {
            harness.step();
            scene.imu.pose.subscribers() == readers
        })
    };

    // An output is opened in the list, and one of the numbers inside it is put on the chart.
    grab(&mut harness, &sensor, "numbers inside");
    number(&harness, &sensor, "numbers inside").click();
    assert!(readers(&mut harness, 2), "an output that is open in the list is read");
    let listed = wait_for(|| {
        scene.imu.pose.send(Pose::default()).expect("send");
        harness.step();
        shows(&harness, "translation[0]")
    });
    assert!(listed);
    let (from, chart) = (grab(&mut harness, &sensor, "translation[0]"), room(&harness, "Chart 1"));
    drag(&mut harness, from, chart.center());
    let line = format!("{sensor} translation[0]");
    assert!(shows(&harness, &line));

    // The list is hidden. The chart has its room, and goes on drawing what is on it.
    harness.get_by_label("Numbers").click();
    harness.run_steps(3);
    assert!(!shows(&harness, &sensor) && !shows(&harness, "numbers inside") && shows(&harness, &line));
    let wider = room(&harness, "Chart 1");
    assert!(wider.left() < 30.0 && wider.width() > chart.width() + 250.0, "{wider:?}");
    harness.run_steps(5);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(scene.imu.pose.subscribers(), 2, "a number on a chart is still read");
    // With the line taken off, nothing wants that output: a list nobody sees keeps nothing open.
    harness.get_by_label(line.as_str()).parent().unwrap().get_by_label("×").click();
    assert!(readers(&mut harness, 1));

    // Shown again, the list is as it was: the output is still open in it, and read again.
    harness.get_by_label("Numbers").click();
    harness.run_steps(3);
    assert!(shows(&harness, &sensor) && shows(&harness, "numbers inside") && room(&harness, "Chart 1") == chart);
    assert!(readers(&mut harness, 2));
    let listed = wait_for(|| {
        scene.imu.pose.send(Pose::default()).expect("send");
        harness.step();
        shows(&harness, "translation[0]")
    });
    assert!(listed, "with the numbers inside it");
    Ok(())
}

#[test]
fn what_comes_with_every_message_is_drawn_up_to_the_present() -> triq::Result {
    let _alone = alone();
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
    charts.add(1, Trace::Heard { connection, stat: Stat::Rate });
    let mut harness = open(Shown { charts, history, signals: Signals::start()?, now: reported + 0.05 });

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
    harness.state_mut().charts.add(1, Trace::Inside { topic: sensor.clone(), number: height.to_owned() });
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
fn each_chart_shows_as_much_of_the_past_as_it_is_told() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (sensor, filtered) =
        (format!("{}/imu/pose", scene.robot.name()), format!("{}/smoother/raw", scene.robot.name()));
    // The rate of the filter's connection is on two charts.
    let rate = Trace::Heard { connection: Connection { to: filtered, from: sensor }, stat: Stat::Rate };
    let mut charts = Charts::default();
    charts.add(1, rate.clone());
    let history = history(&scene)?;
    let now = history.newest().expect("points");
    let mut harness = open(Shown { charts, history, signals: Signals::start()?, now });
    add(&mut harness);
    harness.state_mut().charts.add(2, rate);
    harness.run_steps(3);

    // A chart begins with the last thirty seconds, which hold the 80 of six seconds ago.
    for chart in ["Chart 1", "Chart 2"] {
        assert!(written_on(&harness, chart, "30 s ago") && written_on(&harness, chart, "80.0"), "{chart}");
    }
    // The first is told to show five. The second goes on as it was.
    show_the_last(&mut harness, "Chart 1", 5.0);
    assert!(written_on(&harness, "Chart 1", "5 s ago") && written_on(&harness, "Chart 1", "70.0"));
    assert!(!written_on(&harness, "Chart 1", "80.0"));
    assert!(written_on(&harness, "Chart 2", "30 s ago") && written_on(&harness, "Chart 2", "80.0"));
    // And a chart that is added begins with thirty, like the others did.
    add(&mut harness);
    assert!(written_on(&harness, "Chart 1", "5 s ago") && !shows(&harness, "5 s"), "only the first shows five");
    Ok(())
}

#[test]
fn charts_kept_by_an_older_workbench_are_still_drawn() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let charts: Charts = serde_json::from_str(KEPT).expect("charts");
    let mut harness = arranging(&scene, charts)?;
    harness.run_steps(3);
    for line in ["robot/smoother/raw rate", "robot/imu/pose translation[0]"] {
        assert!(written_on(&harness, "Chart 1", line), "{line}");
    }
    assert_eq!(colour(&harness, "robot/imu/pose translation[0]"), "#ff00ffff", "in the colour that was chosen");
    Ok(())
}

#[test]
fn a_line_is_taken_by_its_name_and_put_down_on_another_chart() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    // Two lines on the first chart, and a second chart with nothing on it. Nothing by the lines'
    // names is running, which a line that is moved does not mind.
    let mut harness = arranging(&scene, serde_json::from_str(KEPT).expect("charts"))?;
    add(&mut harness);
    let (rate, inside) = ("robot/smoother/raw rate", "robot/imu/pose translation[0]");
    let name = |harness: &Window<'_>, line: &str| harness.get_by_label(line).rect().center();
    let on = |harness: &Window<'_>, chart: &str| [rate, inside].map(|line| written_on(harness, chart, line));

    // Put down on the second chart, a line is drawn there, and no longer on the first.
    let (from, second) = (name(&harness, inside), room(&harness, "Chart 2").center());
    drag(&mut harness, from, second);
    assert_eq!((on(&harness, "Chart 1"), on(&harness, "Chart 2")), ([true, false], [false, true]));
    assert_eq!(colour(&harness, inside), "#ff00ffff", "in the colour that was chosen for it");
    // Put down on the chart it is on, it stays there.
    let from = name(&harness, inside);
    drag(&mut harness, from, second + egui::vec2(0.0, 100.0));
    assert_eq!((on(&harness, "Chart 1"), on(&harness, "Chart 2")), ([true, false], [false, true]));

    // The other follows it. A line that was given its colour takes one that is free where it lands.
    let from = name(&harness, rate);
    drag(&mut harness, from, second);
    assert_eq!((on(&harness, "Chart 1"), on(&harness, "Chart 2")), ([false, false], [true, true]));
    assert_ne!(colour(&harness, rate), colour(&harness, inside));
    assert!(written_on(&harness, "Chart 1", "Drag a number here."), "the first chart has nothing on it");
    Ok(())
}

#[test]
fn several_numbers_are_chosen_and_dragged_onto_a_chart_together() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let [rate, lost, gap, latency] = Stat::ALL.map(|stat| format!("{filtered} {}", stat.name()));
    let mut harness = arranging(&scene, Charts::default())?;
    three(&mut harness);
    let choose = |harness: &mut Window<'_>, number: &str| {
        let at = grab(harness, &filtered, number);
        click_holding(harness, at, egui::Modifiers::COMMAND);
    };
    let put = |harness: &mut Window<'_>, number: &str, chart: &str| {
        let (from, onto) = (grab(harness, &filtered, number), room(harness, chart).center());
        drag(harness, from, onto);
    };

    // Clicked with the command key held, a number is one more of those that are chosen. Clicked so
    // again, it is not. Whichever of them is dragged takes the others along, in the order they
    // were chosen in.
    for number in ["latency", "gap", "rate", "gap"] {
        choose(&mut harness, number);
    }
    put(&mut harness, "rate", "Chart 1");
    assert_eq!(lines(&harness, &scene), [latency.clone(), rate.clone()]);
    // A number that is not among them goes alone, and they stay chosen.
    put(&mut harness, "lost", "Chart 2");
    put(&mut harness, "latency", "Chart 3");
    assert_eq!(lines(&harness, &scene), [latency.clone(), rate.clone(), lost.clone(), latency.clone(), rate.clone()]);

    // Clicked without the key, a number is the only one that is chosen.
    let at = grab(&mut harness, &filtered, "gap");
    click(&mut harness, at, egui::PointerButton::Primary);
    choose(&mut harness, "lost");
    put(&mut harness, "lost", "Chart 1");
    assert_eq!(lines(&harness, &scene)[..4], [latency, rate, gap, lost]);
    Ok(())
}

/// The pieces that the lines of a chart are drawn in, from left to right: how strong the colour of each is.
fn strengths(harness: &Window<'_>, chart: &str) -> Vec<u8> {
    let room = room(harness, chart).expand(1.0);
    let lines = strokes(harness).into_iter().filter(|(points, _, width)| {
        *width == 2.0 && points.len() > 1 && points.iter().all(|point| room.contains(*point))
    });
    let mut pieces: Vec<(f32, u8)> = lines.map(|(points, color, _)| (points[0].x, color.a())).collect();
    pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
    pieces.into_iter().map(|(_, strength)| strength).collect()
}

#[test]
fn a_line_is_told_to_fade_into_the_past() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (sensor, filtered) =
        (format!("{}/imu/pose", scene.robot.name()), format!("{}/smoother/raw", scene.robot.name()));
    let mut charts = Charts::default();
    charts.add(1, Trace::Heard { connection: Connection { to: filtered.clone(), from: sensor }, stat: Stat::Rate });
    let history = history(&scene)?;
    let now = history.newest().expect("points");
    let mut harness = open(Shown { charts, history, signals: Signals::start()?, now });
    // The chart shows five seconds, and the line crosses all of it.
    show_the_last(&mut harness, "Chart 1", 5.0);
    let line = format!("{filtered} rate");
    assert_eq!(strengths(&harness, "Chart 1"), [255], "a line is drawn in one piece, in its colour");

    // The button beside its name opens what can be said of how it is drawn. Told to fade, it is
    // weaker the further back it goes, in steps too small to see, and as strong as ever at the present.
    harness.get_by_label(line.as_str()).parent().unwrap().get_by_label("…").click();
    harness.run_steps(3);
    harness.get_by_label("Fade into the past").click();
    harness.run_steps(3);
    let faded = strengths(&harness, "Chart 1");
    assert!(faded.len() > 20 && faded.is_sorted() && faded[faded.len() - 1] > 240, "{faded:?}");
    assert!(faded.windows(2).all(|pair| pair[1] - pair[0] < 12) && faded[0] < 64, "{faded:?}");
    // The slider says how see-through its oldest point is: half.
    harness.get_by_role(Role::Slider).set_value_accesskit(0.5);
    harness.run_steps(3);
    let faded = strengths(&harness, "Chart 1");
    assert!(faded[0].abs_diff(128) < 10 && faded[faded.len() - 1] > 240, "{faded:?}");
    // That is kept with the chart, for the next run.
    let kept = serde_json::to_string(&harness.state().charts).expect("charts as text");
    assert!(kept.contains(r#""fade":0.5"#), "{kept}");

    // Told not to fade any more, it is one piece again.
    harness.get_by_label("Fade into the past").click();
    harness.run_steps(3);
    assert_eq!(strengths(&harness, "Chart 1"), [255]);
    Ok(())
}

#[test]
fn a_line_that_fades_is_drawn_in_pieces_each_stronger_than_the_one_before_it() {
    // A line from the oldest moment a chart shows to the present: its places, and how old each is.
    let line: Vec<(egui::Pos2, f32)> =
        (0..=100).map(|step| (egui::pos2(step as f32, 0.0), 1.0 - step as f32 / 100.0)).collect();
    let ends = |pieces: &[(Vec<egui::Pos2>, f32)]| (pieces[0].0[0], *pieces[pieces.len() - 1].0.last().unwrap());

    // One that does not fade at all is one piece, at full strength.
    let whole = charts::pieces(&line, 0.0);
    assert_eq!((whole.len(), whole[0].0.len(), whole[0].1), (1, 101, 1.0));

    // One that fades to nothing begins at nothing and is at full strength at the present.
    let faded = charts::pieces(&line, 1.0);
    assert!(faded.len() > 20 && faded.len() <= 40, "{} pieces", faded.len());
    assert!(faded.windows(2).all(|pair| pair[0].1 < pair[1].1), "each stronger than the one before it");
    assert!(faded.windows(2).all(|pair| pair[0].0.last() == pair[1].0.first()), "and beginning where that ends");
    assert_eq!(ends(&faded), (line[0].0, line[100].0), "from end to end");
    assert!(faded[0].1 < 0.05 && faded[faded.len() - 1].1 == 1.0);
    // Halfway, its oldest point is at half strength.
    assert!((charts::pieces(&line, 0.5)[0].1 - 0.5).abs() < 0.05);

    // A line of two points far apart fades as evenly as one of many.
    let sparse = charts::pieces(&[line[0], line[100]], 1.0);
    assert!(sparse.len() > 20 && sparse.windows(2).all(|pair| pair[0].1 < pair[1].1));
    assert_eq!(ends(&sparse), (line[0].0, line[100].0));
    let middle = &sparse[sparse.len() / 2];
    assert!((middle.0[0].x - 50.0).abs() < 5.0 && (middle.1 - 0.5).abs() < 0.05, "half as strong halfway");
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
