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
    // In the list of numbers, what a node sends is folded away under its name until that is clicked.
    assert!(until(&mut harness, |harness| shows(harness, &format!("{robot}/imu"))), "the sensor is in the list");
    unfold(&mut harness, &sensor);
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
    harness.run_steps(3);
    harness.query_all_by_role(Role::SpinButton).for_each(|past| past.set_value_accesskit(10.0));
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

/// Where on screen the text of a port is, on the card that is at `place` in the graph.
fn port_on(harness: &Bench<'_>, place: egui::Pos2, port: &str) -> egui::Pos2 {
    let on_card = |written: &egui::Rect| written.left() >= place.x && written.top() >= place.y;
    let nearest = |written: &egui::Rect| (written.left_top() - place).length();
    let found = harness.query_all_by_role_and_label(Role::Label, port).map(|text| text.rect()).filter(on_card);
    let written = found.min_by(|a, b| nearest(a).total_cmp(&nearest(b))).expect("the port is written on the card");
    view(harness) * written.center()
}

#[test]
fn a_card_is_where_it_was_put_when_its_program_runs_again() -> triq::Result {
    // A program with a sensor, under a name it will have again when it runs the next time.
    let robot = format!("w{}-cards", std::process::id());
    let start = || -> triq::Result<(Runtime, ImuPorts)> {
        let runtime = Runtime::new(&robot)?;
        let imu = runtime.attach("imu")?;
        Ok((runtime, imu))
    };
    let place = |harness: &Bench<'_>| {
        let mut cards = harness.state().graph().nodes_pos();
        cards.find(|(_, card)| card.runtime() == Some(robot.as_str()) && card.name() == "imu").map(|found| found.0)
    };
    let (runtime, imu) = start()?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| place(harness).is_some()), "its card appears");
    harness.run_steps(4);

    // The user drags the card somewhere else, by the text of its port.
    let was = place(&harness).unwrap();
    let port = port_on(&harness, was, "pose: Pose");
    drag(&mut harness, port, port + egui::vec2(150.0, 110.0));
    harness.run_steps(3);
    let put = place(&harness).unwrap();
    assert!((put - was).length() > 100.0, "the card moved, from {was:?} to {put:?}");

    // The workbench says how it is, to be kept. Then it closes, and so does the program.
    let layout = harness.state().layout();
    drop((harness, imu, runtime));

    // The next run, told how it was. The program is not running: there is no card.
    let mut harness = open()?;
    harness.state_mut().arrange(&layout);
    assert!(until(&mut harness, |harness| place(harness).is_none()));
    // It runs again, and its card is where the user put it.
    let (runtime, imu) = start()?;
    assert!(until(&mut harness, |harness| place(harness).is_some()), "its card appears again");
    assert_eq!(place(&harness), Some(put));

    // `Tidy` lays every card out afresh, and forgets where cards were put.
    harness.get_by_label("Tidy").click();
    harness.run_steps(3);
    let tidied = place(&harness).unwrap();
    assert_ne!(tidied, put);
    drop((imu, runtime));
    assert!(until(&mut harness, |harness| place(harness).is_none()));
    let (_runtime, _imu) = start()?;
    assert!(until(&mut harness, |harness| place(harness).is_some()));
    assert_ne!(place(&harness), Some(put), "it is not put back where it was before the tidying");

    // A layout from before cards were remembered is taken as it is, with no cards.
    let mut older: serde_json::Value = serde_json::from_str(&layout).expect("a layout is JSON");
    assert!(older.as_object_mut().unwrap().remove("cards").is_some(), "cards are part of what is kept");
    harness.state_mut().arrange(&older.to_string());
    harness.run_steps(3);
    assert_eq!(arranged(&harness).iter().flatten().count(), 5, "the four windows and a chart are there");
    Ok(())
}

#[test]
fn the_colour_chosen_for_a_runtime_is_its_colour_in_the_next_run_too() -> triq::Result {
    let robot = format!("w{}-painted", std::process::id());
    let runtime = Runtime::new(&robot)?;
    let _imu: ImuPorts = runtime.attach("imu")?;
    let listed =
        |harness: &Bench<'_>| harness.state().graph().nodes().any(|card| card.runtime() == Some(robot.as_str()));
    let mut harness = open()?;
    assert!(until(&mut harness, listed), "its card appears");

    // The user chooses a colour for it, with the picker beside its name among the runtimes.
    let pink = egui::Color32::from_rgb(250, 20, 160);
    harness.state_mut().paint(&robot, Some(pink));
    harness.run_steps(3);
    assert_eq!(harness.state().colors().of(&robot, true), pink);
    // The card wears it. Looking at colours takes a GPU.
    let dot = |harness: &mut Bench<'_>| {
        let picture = harness.render().map_err(|reason| eprintln!("no picture: {reason}")).ok()?;
        // Other programs have a node called `imu` as well. The dot is called by the runtime, and is on its card.
        let dot = harness.query_all_by_label(&robot).last().expect("a dot");
        let at = seen(harness, &dot) * harness.ctx.pixels_per_point();
        Some(picture.get_pixel(at.x as u32, at.y as u32).0)
    };
    if let Some(drawn) = dot(&mut harness) {
        assert_eq!(drawn, pink.to_array());
    }

    // The next run, told how the last one was: the runtime has the colour that was chosen for it.
    let layout = harness.state().layout();
    drop(harness);
    let mut harness = open()?;
    harness.state_mut().arrange(&layout);
    assert!(until(&mut harness, listed));
    assert_eq!(harness.state().colors().of(&robot, true), pink);

    // Given back, it has a colour of the workbench's choosing again, and nothing is kept of the other.
    harness.state_mut().paint(&robot, None);
    assert_ne!(harness.state().colors().of(&robot, true), pink);
    let mut again = open()?;
    again.state_mut().arrange(&harness.state().layout());
    assert!(until(&mut again, listed));
    assert_ne!(again.state().colors().of(&robot, true), pink);
    Ok(())
}
