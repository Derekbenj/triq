//! What the workbench keeps from one run to the next: how its windows and
//! charts were arranged, and which numbers were on which chart. A number is
//! remembered by what it is called, so it is drawn again when a program with
//! those names runs again, and waits until then.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::{NodeT, Queryable};
use scene::*;
use triq::prelude::*;
use triq_workbench::egui::{self, accesskit::Role};
use window::*;

const WINDOWS: [&str; 4] = ["Runtimes", "Graph", "Details", "Charts"];

/// Where the name of each window is, on the bar above it, and where the name of each of the
/// two charts is: all that an arrangement can be told by.
fn arranged(harness: &Bench<'_>) -> Vec<Option<egui::Rect>> {
    let windows = WINDOWS.map(|window| bar(harness, window).map(|name| name.rect()));
    let charts = ["Chart 1", "Chart 2"]
        .map(|chart| harness.query_all_by_role_and_label(Role::Button, chart).next().map(|name| name.rect()));
    windows.into_iter().chain(charts).collect()
}

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

/// Where a chart is drawn, under its bar.
fn room(harness: &Bench<'_>, chart: &str) -> egui::Pos2 {
    harness.get_by_role_and_label(Role::Canvas, chart).rect().center()
}

/// Where to press on one number of an output in the list. It is brought into view first:
/// topics of other programs on the machine may be listed above it.
fn grab(harness: &mut Bench<'_>, output: &str, number: &str) -> egui::Pos2 {
    let row = |harness: &Bench<'_>| {
        harness.get_by_label(output).parent().expect("its part of the list").get_by_label(number).rect()
    };
    harness.get_by_label(output).parent().expect("its part of the list").get_by_label(number).scroll_to_me();
    harness.run_steps(3);
    row(harness).center()
}

/// What is written beside the name of a line on a chart: its value as it stands, or that there is none yet.
fn beside(harness: &Bench<'_>, line: &str) -> Vec<String> {
    let Some(names) = harness.query_by_label(line).and_then(|name| name.parent()) else { return Vec::new() };
    names.query_all_by_role(Role::Label).filter_map(|label| label.value()).filter(|text| text != line).collect()
}

#[test]
fn the_workbench_is_as_it_was_when_it_was_last_closed() -> triq::Result {
    // A program with a sensor, under a name it will have again when it runs the next time.
    let robot = format!("w{}-again", std::process::id());
    let sensor = format!("{robot}/imu/pose");
    let start = || -> triq::Result<(Runtime, ImuPorts)> {
        let runtime = Runtime::new(&robot)?;
        let imu = runtime.attach("imu")?;
        Ok((runtime, imu))
    };
    let pose = Pose { translation: [2.5, 0.0, 0.0], ..Pose::default() };
    let (runtime, imu) = start()?;
    let mut harness = open()?;
    let listed = until(&mut harness, |harness| {
        imu.pose.send(pose).expect("send");
        let output = harness.query_by_label(&sensor).and_then(|name| name.parent());
        output.is_some_and(|output| output.query_by_label("rate").is_some())
    });
    assert!(listed, "the sensor's output is listed, with how fast it sends");

    // The user arranges things: the details are closed, there is a second chart, and the charts
    // show the last ten seconds. How fast the sensor sends is on the second chart, and a number
    // from inside its messages on the first.
    switch(&harness, "Details").click();
    harness.get_by_label("New chart").click();
    harness.get_by_role(Role::SpinButton).set_value_accesskit(10.0);
    harness.run_steps(3);
    let (from, onto) = (grab(&mut harness, &sensor, "rate"), room(&harness, "Chart 2"));
    drag(&mut harness, from, onto);
    harness.get_by_label(sensor.as_str()).parent().unwrap().get_by_label("numbers inside").click();
    let opened = until(&mut harness, |harness| {
        imu.pose.send(pose).expect("send");
        harness.query_by_label("translation[0]").is_some()
    });
    assert!(opened);
    let (from, onto) = (grab(&mut harness, &sensor, "translation[0]"), room(&harness, "Chart 1"));
    drag(&mut harness, from, onto);
    let (rate, inside) = (format!("{sensor} rate"), format!("{sensor} translation[0]"));
    // Both are on their charts, with their values as they stand, and drawn over the last ten seconds.
    let drawn = |harness: &Bench<'_>| {
        let valued =
            beside(harness, &rate).iter().any(|value| value.ends_with("/s")) && beside(harness, &inside) == ["2.50"];
        valued && harness.query_all_by_label("10 s ago").count() == 2
    };
    let both = until(&mut harness, |harness| {
        imu.pose.send(pose).expect("send");
        drawn(harness)
    });
    assert!(both, "{:?} {:?}", beside(&harness, &rate), beside(&harness, &inside));
    let before = arranged(&harness);
    assert!(before[2].is_none() && before.iter().flatten().count() == 5, "{before:?}");

    // The workbench says how it is, to be kept. Then it closes, and so does the program.
    let layout = harness.state().layout();
    drop((harness, imu, runtime));

    // The next run. Told how it was, the workbench is so again: the windows, the charts, and
    // what is on them.
    let mut harness = open()?;
    harness.state_mut().arrange(&layout);
    harness.run_steps(5);
    assert_eq!(arranged(&harness), before);
    // The sensor's program is not running. Its numbers are on their charts by name, and wait.
    assert!(until(&mut harness, |harness| harness.query_by_label(&sensor).is_none()), "it is not in the list");
    assert_eq!(
        (beside(&harness, &rate), beside(&harness, &inside)),
        (vec!["nothing yet".to_owned()], vec!["nothing yet".to_owned()])
    );

    // The program runs again, under the names it had. Nobody touches the workbench: the lines are drawn.
    let (_runtime, imu) = start()?;
    let both = until(&mut harness, |harness| {
        imu.pose.send(pose).expect("send");
        drawn(harness)
    });
    assert!(both, "{:?} {:?}", beside(&harness, &rate), beside(&harness, &inside));
    assert!(!shows(&harness, "nothing yet"), "over the ten seconds that were chosen");
    assert_eq!(arranged(&harness), before);

    // What cannot be read as an arrangement is passed over, and everything stays as it is.
    harness.state_mut().arrange("neither windows nor charts");
    harness.state_mut().arrange(r#"{"windows": 3, "charts": []}"#);
    harness.run_steps(3);
    assert!(arranged(&harness) == before && shows(&harness, &rate));
    Ok(())
}
