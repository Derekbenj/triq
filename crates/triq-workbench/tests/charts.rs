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
use window::{alone, drag, drag_by_way_of};

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

/// Type into the box that says how much of the past the charts show.
fn show_the_last(harness: &mut Window<'_>, seconds: f64) {
    harness.get_by_role(Role::SpinButton).set_value_accesskit(seconds);
    harness.run_steps(3);
}

/// The charts in a window.
fn open(shown: Shown) -> Window<'static> {
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
    harness.get_by_label(chart).rect()
}

/// A button on the bar above a chart: the first one to the right of the chart's name.
fn on_bar<'a>(harness: &'a Window<'_>, chart: &str, button: &'a str) -> Node<'a> {
    let name = name(harness, chart);
    let after = |button: &Node<'_>| {
        let place = button.rect();
        (place.center().y - name.center().y).abs() < name.height() && place.left() >= name.right()
    };
    let buttons = harness.query_all_by_label(button).filter(after);
    buttons.min_by(|a, b| a.rect().left().total_cmp(&b.rect().left())).unwrap_or_else(|| panic!("no {button:?}"))
}

/// Press a button on the bar above a chart.
fn split(harness: &mut Window<'_>, chart: &str, button: &str) {
    on_bar(harness, chart, button).click();
    harness.run_steps(3);
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
    show_the_last(&mut harness, 5.0);
    assert!(shows(&harness, "70.0") && shows(&harness, "20.0") && shows(&harness, "5 s ago"));
    assert!(!shows(&harness, "80.0"));

    // A chart ends at the time it is told, not at its newest point. Three
    // seconds on, the line has slid left: what is left in view runs from 20 to 40.
    harness.state_mut().now += 3.0;
    harness.run_steps(2);
    assert!(shows(&harness, "40.0") && shows(&harness, "20.0") && !shows(&harness, "70.0"));
    harness.state_mut().now -= 3.0;

    // A second number on the same chart: the chart reaches down to take it in.
    show_the_last(&mut harness, 30.0);
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

    // More charts, each with its own numbers: a chart is split, and the new one takes half its room.
    split(&mut harness, "Chart 1", "Split right");
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

/// A place on a chart, by where its name is: well inside it, under the bar.
fn on(name: egui::Rect) -> egui::Pos2 {
    name.left_bottom() + egui::vec2(150.0, 150.0)
}

#[test]
fn a_chart_is_split_and_its_parts_are_resized_and_closed() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let (rate, lost, gap) = (format!("{filtered} rate"), format!("{filtered} lost"), format!("{filtered} gap"));
    let mut harness = arranging(&scene, Charts::default())?;

    // One chart to begin with, which cannot be closed. It is split: a new chart takes its lower half.
    assert!(shows(&harness, "Chart 1") && !shows(&harness, "Close"));
    split(&mut harness, "Chart 1", "Split down");
    let (first, second) = (name(&harness, "Chart 1"), name(&harness, "Chart 2"));
    assert!(second.top() > first.top() + 300.0 && (second.left() - first.left()).abs() < 1.0, "under the first");
    // The new chart is split the other way: panes within panes.
    split(&mut harness, "Chart 2", "Split right");
    let (second, third) = (name(&harness, "Chart 2"), name(&harness, "Chart 3"));
    assert!(third.left() > second.left() + 300.0 && (third.top() - second.top()).abs() < 1.0, "beside the second");
    assert_eq!(name(&harness, "Chart 1"), first, "the first is where it was");

    // A number lands on the chart it is put down on, and no chart moves for it.
    for (number, chart) in [("gap", first), ("rate", third), ("lost", second)] {
        let from = grab(&mut harness, &filtered, number);
        drag(&mut harness, from, on(chart));
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
    split(&mut harness, "Chart 1", "Split down");
    split(&mut harness, "Chart 2", "Split right");
    for (number, chart) in [("lost", "Chart 2"), ("rate", "Chart 3")] {
        let (from, chart) = (grab(&mut harness, &filtered, number), name(&harness, chart));
        drag(&mut harness, from, on(chart));
    }
    assert_eq!(lines(&harness, &scene), [inside.clone(), lost.clone(), rate.clone()]);
    let names = |harness: &Window<'_>| ["Chart 1", "Chart 2", "Chart 3"].map(|chart| name(harness, chart));
    let [first, second, third] = names(&harness);

    // A chart is taken by its name and put down elsewhere. On the left half of another, it becomes
    // that chart's left neighbour. Its lines go with it, and what it passes over gains none.
    let left_half = egui::pos2(first.left() + 200.0, (first.bottom() + second.top()) / 2.0);
    drag(&mut harness, third.center(), left_half);
    harness.run_steps(10);
    let [first, second, third] = names(&harness);
    assert!(third.left() < first.left() && (third.top() - first.top()).abs() < 1.0, "to the left of the first");
    assert!(second.top() > first.bottom() + 100.0, "the second is still below, with the row to itself");
    assert_eq!(lines(&harness, &scene), [rate.clone(), inside.clone(), lost.clone()]);
    // A number is put down on it where it is now.
    let from = grab(&mut harness, &filtered, "gap");
    drag(&mut harness, from, on(third));
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
    harness.get_by_label("Chart 1").click();
    harness.run_steps(3);
    assert_eq!(lines(&harness, &scene), [rate.clone(), inside.clone(), gap.clone()]);
    // So does a number that is held over its name, to be put down on it.
    let (from, second) = (grab(&mut harness, &filtered, "latency"), name(&harness, "Chart 2"));
    drag_by_way_of(&mut harness, &[from, second.center(), on(second)]);
    let latency = format!("{filtered} latency");
    assert_eq!(lines(&harness, &scene), [rate.clone(), lost.clone(), gap.clone(), latency.clone()]);

    // A place is taken by the bar its names are on, with every chart in it. Put down on the
    // middle of another chart, all of them share that chart's place.
    let [first, second, third] = names(&harness);
    let (bar, middle) =
        (second.right_center() + egui::vec2(60.0, 0.0), egui::pos2((third.left() + first.left()) / 2.0, 430.0));
    drag(&mut harness, bar, middle);
    harness.run_steps(10);
    let [first, second, third] = names(&harness);
    assert!(third.right() <= first.left() && first.right() <= second.left(), "three names in a row");
    assert!(first.top() == third.top() && second.top() == third.top() && !shows(&harness, "Tabs"), "on one bar");
    assert_eq!(lines(&harness, &scene), [lost.clone(), latency.clone()], "the one that was in front still is");

    // A chart that is closed is no longer read for.
    close(&mut harness, "Chart 1");
    assert!(!shows(&harness, "Chart 1") && lines(&harness, &scene) == [lost, latency]);
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
    // however many there are. The largest value in view is at the very top, level with the first name.
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
fn charts_that_share_a_place_are_each_split_within_it() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let (rate, lost) = (format!("{filtered} rate"), format!("{filtered} lost"));
    let mut harness = arranging(&scene, Charts::default())?;

    // Two charts in one place: the second was made beside the first and put down on its middle.
    split(&mut harness, "Chart 1", "Split right");
    let (first, second) = (name(&harness, "Chart 1"), name(&harness, "Chart 2"));
    drag(&mut harness, second.center(), egui::pos2((first.left() + second.left()) / 2.0, 430.0));
    harness.run_steps(10);
    let (first, second) = (name(&harness, "Chart 1"), name(&harness, "Chart 2"));
    assert!(second.left() >= first.right() && second.top() == first.top(), "their names are on one bar");

    // The buttons on that bar split the chart that is in front, within the place it has. The
    // bar names what is behind each of its names: the first chart, and the second with a third.
    split(&mut harness, "Chart 1", "Split down");
    let (alone, pair) = (name(&harness, "Chart 1"), name(&harness, "Charts 2, 3"));
    assert!(pair.left() >= alone.right() && pair.top() == alone.top(), "one bar still");
    let (second, third) = (name(&harness, "Chart 2"), name(&harness, "Chart 3"));
    assert!(second.top() > pair.bottom() - 1.0 && third.top() > second.top() + 300.0, "the third under the second");
    assert!((second.left() - alone.left()).abs() < 1.0 && (third.left() - alone.left()).abs() < 1.0);
    // A number lands on the chart it is put down on, also here.
    let from = grab(&mut harness, &filtered, "rate");
    drag(&mut harness, from, on(third));
    assert_eq!(lines(&harness, &scene), [rate.as_str()]);

    // The first chart is brought forward, and has the whole place. It is split the other way.
    harness.get_by_label("Chart 1").click();
    harness.run_steps(3);
    assert!(!shows(&harness, "Chart 2") && !shows(&harness, "Chart 3") && lines(&harness, &scene).is_empty());
    split(&mut harness, "Chart 1", "Split right");
    let (pair, other) = (name(&harness, "Charts 1, 4"), name(&harness, "Charts 2, 3"));
    assert!(other.left() >= pair.right() && other.top() == pair.top(), "two names on the bar, each for two charts");
    let (first, fourth) = (name(&harness, "Chart 1"), name(&harness, "Chart 4"));
    assert!(fourth.left() > first.left() + 300.0 && fourth.top() == first.top(), "the fourth beside the first");
    let from = grab(&mut harness, &filtered, "lost");
    drag(&mut harness, from, on(fourth));
    assert_eq!(lines(&harness, &scene), [lost.as_str()]);

    // Each name on the bar brings its arrangement forward.
    harness.get_by_label("Charts 2, 3").click();
    harness.run_steps(3);
    assert!(shows(&harness, "Chart 3") && !shows(&harness, "Chart 4") && lines(&harness, &scene) == [rate.clone()]);
    // A chart within an arrangement is split again like any other.
    split(&mut harness, "Chart 3", "Split right");
    let (third, fifth) = (name(&harness, "Chart 3"), name(&harness, "Chart 5"));
    assert!(fifth.left() > third.left() + 300.0 && fifth.top() == third.top() && shows(&harness, "Charts 2, 3, 5"));

    // The cross beside a name on the bar closes all that is behind the name.
    close(&mut harness, "Charts 1, 4");
    assert!(!shows(&harness, "Chart 1") && !shows(&harness, "Chart 4"));
    assert!(!shows(&harness, "Charts 2, 3, 5"), "what is left has the place to itself, and needs no name");
    assert_eq!(lines(&harness, &scene), [rate]);
    close(&mut harness, "Chart 2");
    close(&mut harness, "Chart 3");
    assert!(shows(&harness, "Chart 5") && !shows(&harness, "Close"), "the last chart stays");
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
