//! The four windows of the workbench: closed, opened again, and put where the user wants them.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq_workbench::egui::{self, PointerButton};
use window::*;

const WINDOWS: [&str; 4] = ["Runtimes", "Graph", "Details", "Charts"];

/// Where the name of each window is, on the bar above it. A window that is closed has none.
fn names(harness: &Bench<'_>) -> [Option<egui::Rect>; 4] {
    WINDOWS.map(|window| bar(harness, window).map(|name| name.rect()))
}

fn shows(harness: &Bench<'_>, label: &str) -> bool {
    harness.query_all_by_label(label).next().is_some()
}

#[test]
fn the_windows_are_closed_opened_and_put_elsewhere() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);

    // Four windows, all open: the runtimes, the graph and the details side by side, the charts under them.
    let all = names(&harness);
    let [runtimes, graph, details, charts] = all.map(|name| name.expect("an open window"));
    assert!(runtimes.right() < graph.left() && graph.right() < details.left(), "three in a row");
    assert!(runtimes.top() == graph.top() && graph.top() == details.top());
    assert!(charts.top() > graph.top() + 200.0 && charts.left() == runtimes.left(), "the charts under them");
    // Each shows what it is about.
    let about = [
        "New filter",
        "Tidy",
        "Pause a node with the button on its card. Its details have the rest.",
        "Drag a number here.",
    ];
    assert!(about.iter().all(|label| shows(&harness, label)));
    // A picture of them for a human to look at. Drawing it needs a GPU; without one the rest still stands.
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("windows.png");
    match harness.render() {
        Ok(image) => image.save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // The cross beside its name closes a window, and its neighbours have the room.
    let card = |harness: &Bench<'_>| seen(harness, &text(harness, "raw: Pose"));
    let was = card(&harness);
    close(&mut harness, "Runtimes");
    assert!(names(&harness)[0].is_none() && !shows(&harness, "New filter"));
    let [_, wider, beside, _] = names(&harness);
    let (wider, beside) = (wider.expect("the graph"), beside.expect("the details"));
    assert_eq!(wider.left(), runtimes.left(), "the graph begins at the left now");
    // The cards keep their place in the graph: its middle moved, and they moved with it.
    let moved = ((wider.left() + beside.left()) - (graph.left() + details.left())) / 2.0;
    let carried = card(&harness) - was;
    assert!(moved < -50.0 && (carried.x - moved).abs() < 1.5 && carried.y.abs() < 1.5, "{carried:?}, not {moved}");
    // Its name in the row along the top opens it again, where it was.
    switch(&harness, "Runtimes").click();
    harness.run_steps(3);
    assert!(names(&harness) == all && shows(&harness, "New filter"));
    assert!((card(&harness) - was).length() < 1.5, "and the cards are where they were");
    // The same name closes it.
    for window in ["Charts", "Details"] {
        switch(&harness, window).click();
        harness.run_steps(3);
    }
    assert_eq!(names(&harness).map(|name| name.is_some()), [true, true, false, false]);
    assert!(!shows(&harness, "Drag a number here.") && shows(&harness, "Tidy"));
    for window in ["Charts", "Details"] {
        switch(&harness, window).click();
        harness.run_steps(3);
    }
    assert_eq!(names(&harness), all);

    // A window is taken by its name and put down elsewhere. High on the right half of the graph,
    // where the highlight takes in the graph's bar, the charts become its neighbour there, and the
    // row has the whole height.
    let right_half =
        egui::pos2(graph.left() + (details.left() - graph.left()) * 0.75, (graph.bottom() + charts.top()) / 2.0 - 30.0);
    let from_middle = was.x - (graph.left() + details.left()) / 2.0;
    drag(&mut harness, charts.center(), right_half);
    harness.run_steps(10);
    let [runtimes, graph, details, charts] = names(&harness).map(|name| name.expect("an open window"));
    assert!(graph.right() < charts.left() && charts.right() < details.left(), "between the graph and the details");
    assert!(runtimes.top() == charts.top() && shows(&harness, "Drag a number here."));
    // The graph has half the width it had and all of the height. What was in the middle of it still is.
    let at = card(&harness);
    assert!((at.x - (graph.left() + charts.left()) / 2.0 - from_middle).abs() < 1.5 && at.y > was.y + 100.0, "{at:?}");

    // Put down on the middle of another, a window shares its place and is the one in front.
    let middle = egui::pos2((runtimes.left() + graph.left()) / 2.0, 500.0);
    drag(&mut harness, details.center(), middle);
    harness.run_steps(10);
    let [runtimes, _, details, _] = names(&harness).map(|name| name.expect("an open window"));
    assert!(runtimes.right() <= details.left() && runtimes.top() == details.top(), "their names are on one bar");
    assert!(
        shows(&harness, "Pause a node with the button on its card. Its details have the rest.")
            && !shows(&harness, "New filter")
    );
    // The name of the one behind brings it forward.
    bar(&harness, "Runtimes").expect("its name").click();
    harness.run_steps(3);
    assert!(
        shows(&harness, "New filter")
            && !shows(&harness, "Pause a node with the button on its card. Its details have the rest.")
    );
    // Closed, it leaves the place to the other. Opened again, it is in front there.
    close(&mut harness, "Runtimes");
    assert!(
        shows(&harness, "Pause a node with the button on its card. Its details have the rest.")
            && names(&harness)[0].is_none()
    );
    switch(&harness, "Runtimes").click();
    harness.run_steps(3);
    assert!(shows(&harness, "New filter") && names(&harness)[0].is_some());

    // The windows still work together where they are now: a click on a card in the graph is told by the details.
    bar(&harness, "Details").expect("its name").click();
    harness.run_steps(3);
    let port = seen(&harness, &text(&harness, "raw: Pose"));
    click(&mut harness, port, PointerButton::Primary);
    assert!(shows(&harness, &format!("{robot}, process {}", std::process::id())));

    // Low on a half of another, where the highlight is all under that window's bar, a window goes
    // in with it: the two are a group, under a bar that names both.
    let [_, graph, _, charts] = names(&harness).map(|name| name.expect("an open window"));
    let lower_half =
        egui::pos2((graph.left() + charts.left()) / 2.0, graph.bottom() + (892.0 - graph.bottom()) * 0.75 + 30.0);
    drag(&mut harness, charts.center(), lower_half);
    harness.run_steps(10);
    let group = called(&harness, "Graph, Charts").first().expect("a bar for the two").rect();
    let [_, inner, _, under] = names(&harness).map(|name| name.expect("an open window"));
    assert!(group.top() == graph.top() && inner.top() > group.bottom() - 1.0, "the graph's bar is under the group's");
    assert!(under.top() > inner.top() + 200.0 && under.left() == inner.left(), "and the charts are under the graph");
    // The cross beside the name of a group closes what is in it.
    let cross = harness.query_all_by_label("Close").find(|cross| group.contains(cross.rect().center()));
    cross.expect("a cross beside the name").click();
    harness.run_steps(3);
    assert_eq!(names(&harness).map(|name| name.is_some()), [true, false, true, false]);
    // Opened alone, one of the two is under its own bar and no other: a bar for the group would say the same.
    switch(&harness, "Charts").click();
    harness.run_steps(3);
    assert!(
        called(&harness, "Charts").len() == 2 && shows(&harness, "Drag a number here."),
        "its name above, and on its bar"
    );
    // With the other open as well, they are the group they were.
    switch(&harness, "Graph").click();
    harness.run_steps(3);
    let again = called(&harness, "Graph, Charts").first().expect("the bar for the two").rect();
    assert_eq!((again, names(&harness)[1], names(&harness)[3]), (group, Some(inner), Some(under)));

    // With every window closed, the row along the top is still there to open them with.
    for window in WINDOWS {
        switch(&harness, window).click();
        harness.run_steps(3);
    }
    assert_eq!(names(&harness), [None; 4]);
    assert!(shows(&harness, "Every window is closed. Their names above open them."));
    switch(&harness, "Graph").click();
    harness.run_steps(3);
    assert!(names(&harness)[1].is_some() && shows(&harness, "Tidy"));

    Ok(())
}
