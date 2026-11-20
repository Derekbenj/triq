//! Charts of numbers against each other: two of one message on a plane, or three in space.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/charts.rs"]
mod shown;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::{NodeT, Queryable};
use scene::*;
use shown::*;
use triq_workbench::charts::{self, Charts, Frame, Ground, Lens};
use triq_workbench::egui::{self, Rect, accesskit::Role, pos2, vec2};
use window::{alone, click_holding, drag, drag_with, drawn};

/// Open the sensor's output in the list, so that the numbers inside its messages are listed.
fn opened(harness: &mut Window<'_>, scene: &Scene) -> String {
    let sensor = format!("{}/imu/pose", scene.robot.name());
    inside(harness, scene, &sensor);
    sensor
}

/// Open an output of the robot in the list, so that the numbers inside its messages are listed.
fn inside(harness: &mut Window<'_>, scene: &Scene, output: &str) {
    grab(harness, output, "numbers inside");
    number(harness, output, "numbers inside").click();
    let listed = wait_for(|| {
        scene.imu.pose.send(Pose::default()).expect("send");
        harness.step();
        let entry = number(harness, output, "numbers inside").parent().and_then(|inside| inside.parent());
        entry.is_some_and(|entry| entry.query_all_by_label("translation[0]").next().is_some())
    });
    assert!(listed, "the numbers inside the messages of {output} are listed");
}

/// Choose some of the sensor's numbers in the list, in that order, and put them down on a chart.
fn put_down(harness: &mut Window<'_>, sensor: &str, numbers: &[&str], chart: &str) {
    for number in numbers {
        let at = grab(harness, sensor, number);
        click_holding(harness, at, egui::Modifiers::COMMAND);
    }
    let (from, onto) = (grab(harness, sensor, numbers[0]), room(harness, chart).center());
    drag(harness, from, onto);
    // They are let go of again, for the next that are chosen.
    for number in numbers {
        let at = grab(harness, sensor, number);
        click_holding(harness, at, egui::Modifiers::COMMAND);
    }
}

/// A window with a chart on a plane beside the first chart: `Chart 2`. On it is where the sensor
/// is, its first number along x and its second along y.
fn plane(scene: &Scene) -> triq::Result<(Window<'static>, String)> {
    let mut harness = arranging(scene, Charts::default())?;
    harness.get_by_label("New 2D chart").click();
    harness.run_steps(3);
    let sensor = opened(&mut harness, scene);
    put_down(&mut harness, &sensor, &["translation[0]", "translation[1]"], "Chart 2");
    Ok((harness, sensor))
}

/// The sensor says where it is, and the charts are drawn once the workbench has read that.
fn is_at(harness: &mut Window<'_>, scene: &Scene, place: [f32; 3]) {
    let sensor = format!("{}/imu/pose", scene.robot.name());
    scene.imu.pose.send(Pose { translation: place, ..Pose::default() }).expect("send");
    let read = wait_for(|| {
        let latest = |axis: usize| harness.state().signals.latest(&sensor, &format!("translation[{axis}]"));
        (0..3).all(|axis| latest(axis) == Some(f64::from(place[axis])))
    });
    assert!(read, "the workbench reads the sensor's messages");
    // Two messages are told apart by when they were sent.
    std::thread::sleep(std::time::Duration::from_millis(2));
    harness.state_mut().now = present();
    harness.run_steps(2);
}

/// The slots on a chart that say what is along its axes, in the order they are read in: a row
/// that the chart is too narrow for goes on in the next line.
fn slots<'a>(harness: &'a Window<'_>, chart: &str) -> Vec<egui_kittest::Node<'a>> {
    let room = room(harness, chart);
    let slots = ["x", "y", "z"].into_iter().flat_map(|axis| harness.query_all_by_role_and_label(Role::Button, axis));
    let mut slots: Vec<_> = slots.filter(|slot| room.contains(slot.rect().center())).collect();
    slots.sort_by(|a, b| (a.rect().top(), a.rect().left()).partial_cmp(&(b.rect().top(), b.rect().left())).unwrap());
    slots
}

/// What the slots on a chart say is along its axes: x first.
fn axes(harness: &Window<'_>, chart: &str) -> Vec<String> {
    slots(harness, chart).into_iter().map(|axis| axis.value().unwrap_or_default()).collect()
}

/// Put one of the sensor's numbers in the slot of an axis of a chart, by dragging it there out
/// of the list: 0 is the slot of x.
fn along(harness: &mut Window<'_>, sensor: &str, chart: &str, axis: usize, number: &str) {
    let (from, onto) = (grab(harness, sensor, number), slots(harness, chart)[axis].rect().center());
    drag(harness, from, onto);
    harness.run_steps(3);
}

/// Open what can be said of how a curve is drawn, and tick or untick one of the boxes there.
fn told(harness: &mut Window<'_>, curve: &str, chart: &str, what: &str) {
    let room = room(harness, chart);
    let name = harness.query_all_by_label(curve).find(|name| room.contains(name.rect().center()));
    name.expect("the curve's name").parent().expect("its row").get_by_label("…").click();
    harness.run_steps(3);
    harness.get_by_label(what).click();
    harness.run_steps(3);
    // A click elsewhere closes it again.
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
}

/// Tell the second chart to show all there is.
fn fit(harness: &mut Window<'_>) {
    let place = room(harness, "Chart 2");
    let fit = harness.query_all_by_label("Fit").find(|fit| place.contains(fit.rect().center()));
    fit.expect("the button on the chart").click();
    harness.run_steps(3);
}

/// Where the trail of the one curve on a chart begins and where it ends.
fn ends(harness: &Window<'_>, chart: &str) -> (egui::Pos2, egui::Pos2) {
    let trails = pieces(harness, chart);
    assert_eq!(trails.len(), 1, "one trail, in one piece");
    (trails[0].0[0], trails[0].0[trails[0].0.len() - 1])
}

#[test]
fn two_numbers_of_one_message_are_drawn_against_each_other() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let mut harness = arranging(&scene, Charts::default())?;

    // A chart of numbers against each other is made with a button of its own, and says what it takes.
    harness.get_by_label("New 2D chart").click();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("two numbers of one message").is_some());
    // What a runtime counts is not inside a message: put down here, it is not drawn.
    let filtered = format!("{}/smoother/raw", scene.robot.name());
    let (from, onto) = (grab(&mut harness, &filtered, "rate"), room(&harness, "Chart 2").center());
    drag(&mut harness, from, onto);
    assert!(harness.query_by_label_contains("two numbers of one message").is_some());

    // Two numbers of the sensor's messages are chosen, the one along x first, and put down on it.
    // The curve is called by its topic, and a box for each axis says which number is along it.
    let sensor = opened(&mut harness, &scene);
    put_down(&mut harness, &sensor, &["translation[0]", "translation[1]"], "Chart 2");
    assert!(written_on(&harness, "Chart 2", &sensor) && harness.query_by_label_contains("two numbers").is_none());
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]"]);

    // The newest point is a dot. A chart that is told to show all there is has it in the
    // middle, when it is all there is.
    is_at(&mut harness, &scene, [1.0, 5.0, 0.0]);
    fit(&mut harness);
    let (place, dot) = (room(&harness, "Chart 2"), dots(&harness, "Chart 2"));
    assert!(dot.len() == 1 && (dot[0] - place.center()).length() < 12.0, "{dot:?} in {place:?}");
    // The chart stays where it is, and the point moves over it: to the right and up.
    is_at(&mut harness, &scene, [3.0, 9.0, 0.0]);
    let moved = dots(&harness, "Chart 2");
    assert!(moved.len() == 1 && moved[0].x > dot[0].x + 50.0 && moved[0].y < dot[0].y - 50.0, "{moved:?}");
    assert!(place.contains(moved[0]) && pieces(&harness, "Chart 2").is_empty(), "and no trail is drawn behind it");

    // Told to, the chart draws the path the point took since the curve was put on it. Told to
    // show all there is, it does so from edge to edge: the path goes from the lower left, where
    // x and y were smallest, to the upper right, and the dot is at its newest end.
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    fit(&mut harness);
    let (from, to) = ends(&harness, "Chart 2");
    assert!(to.x > from.x + 300.0 && to.y < from.y - 200.0, "from {from:?} to {to:?}");
    assert_eq!(dots(&harness, "Chart 2"), [to]);
    // Under the curve is a grid, with its lines at round values. Each says its value where it
    // leaves the chart: those of x along the bottom, and those of y at the right.
    let written = |harness: &Window<'_>, value: &str| {
        let found = harness.query_all_by_label(value).find(|written| place.contains(written.rect().center()));
        found.unwrap_or_else(|| panic!("{value} is written on the chart")).rect().center()
    };
    let lines = |harness: &Window<'_>| {
        let lines = drawn(harness).0.into_iter().filter(|line| line.width == 1.0 && place.contains_rect(line.within));
        lines.count()
    };
    let [left, right, low, high] = ["1.0", "3.0", "6.0", "9.0"].map(|value| written(&harness, value));
    assert!((left.x - from.x).abs() < 16.0 && (right.x - to.x).abs() < 16.0, "{left:?} and {right:?}");
    assert!((left.y - right.y).abs() < 1.0 && left.y > from.y - 16.0, "along the bottom: {left:?} and {right:?}");
    assert!(low.x > to.x - 30.0 && high.x > to.x - 30.0 && (high.y - to.y).abs() < 16.0 && low.y > high.y + 400.0);
    assert_eq!(lines(&harness), 5 + 9, "from 1 to 3 and from 5 to 9, half a unit apart");

    // Each axis has a slot that says which number is along it. A number is put in a slot by
    // dragging it there out of the list. One that was along the other axis trades places with
    // the one that was here: both keep their past, and the trail goes on, the other way up.
    along(&mut harness, &sensor, "Chart 2", 0, "translation[1]");
    assert_eq!(axes(&harness, "Chart 2"), ["translation[1]", "translation[0]"]);
    // A slot is taken, with its number, and put down on another slot of the curve: they trade places too.
    let place = |harness: &Window<'_>, axis: usize| slots(harness, "Chart 2")[axis].rect().center();
    let (from, onto) = (place(&harness, 1), place(&harness, 0));
    drag(&mut harness, from, onto);
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]"]);
    let (from, onto) = (place(&harness, 0), place(&harness, 1));
    drag(&mut harness, from, onto);
    assert_eq!(axes(&harness, "Chart 2"), ["translation[1]", "translation[0]"]);
    assert_eq!(
        harness
            .query_all_by_label(sensor.as_str())
            .filter(|name| room(&harness, "Chart 2").contains(name.rect().center()))
            .count(),
        1,
        "and no curve was begun for it"
    );
    fit(&mut harness);
    let [left, right, low, high] = ["5", "9", "2.0", "3.0"].map(|value| written(&harness, value));
    assert!(left.x < right.x - 300.0 && (left.y - right.y).abs() < 1.0 && high.y < low.y - 200.0);
    assert_eq!(pieces(&harness, "Chart 2").len(), 1);
    // A number that was on no chart has no past. The trail begins again with the next messages.
    along(&mut harness, &sensor, "Chart 2", 1, "translation[2]");
    assert!(pieces(&harness, "Chart 2").is_empty());
    is_at(&mut harness, &scene, [0.0, 2.0, 1.0]);
    is_at(&mut harness, &scene, [0.0, 4.0, 3.0]);
    fit(&mut harness);
    let (from, to) = ends(&harness, "Chart 2");
    assert!(to.x > from.x + 300.0 && to.y < from.y - 200.0, "from {from:?} to {to:?}");
    Ok(())
}

#[test]
fn a_chart_of_numbers_against_each_other_is_zoomed_moved_and_fitted() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (mut harness, sensor) = plane(&scene)?;
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    // The sensor goes from one place to another, and then back to halfway between them.
    for place in [[0.0, 0.0, 0.0], [4.0, 2.0, 0.0], [2.0, 1.0, 0.0]] {
        is_at(&mut harness, &scene, place);
    }
    // Where the first two of those places are drawn.
    let corners = |harness: &Window<'_>| {
        let trail = &pieces(harness, "Chart 2")[0].0;
        (trail[0], trail[1])
    };
    // Told to show all there is, the chart does so from edge to edge. The dot is in the middle of it.
    fit(&mut harness);
    let (from, to) = corners(&harness);
    let (all, middle) = (to - from, from + (to - from) / 2.0);
    assert!(all.x > 300.0 && all.y < -200.0, "{all:?}");
    assert!((dots(&harness, "Chart 2")[0] - middle).length() < 1.0);

    // With one scale for both axes, a unit is as long across as up: the trail goes 4 across and 2 up.
    harness.get_by_label("1:1").click();
    harness.run_steps(3);
    let (from, to) = corners(&harness);
    assert!(((to.x - from.x) / 4.0 + (to.y - from.y) / 2.0).abs() < 0.5, "from {from:?} to {to:?}");
    harness.get_by_label("1:1").click();
    harness.run_steps(3);
    let (from, to) = corners(&harness);
    assert!((from + all / 2.0 - middle).length() < 0.5 && (to - from - all).length() < 0.5, "edge to edge again");

    // The wheel zooms, about the middle of the chart.
    harness.hover_at(room(&harness, "Chart 2").center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(6);
    let (from, to) = corners(&harness);
    let nearer = to - from;
    assert!(nearer.x > all.x * 1.2 && nearer.y < all.y * 1.2, "{nearer:?} was {all:?}");
    assert!((from + nearer / 2.0 - middle).length() < 1.0, "what was in the middle still is");
    // From then on the chart stays as it was left, while it shows where the curve is now.
    is_at(&mut harness, &scene, [2.5, 1.0, 0.0]);
    assert!((corners(&harness).1 - to).length() < 1.0);
    // It does not lose sight of that. When the point goes where the chart does not reach, the
    // chart is seen from further away, about the same middle: the point is still on it.
    is_at(&mut harness, &scene, [8.0, 4.0, 0.0]);
    let (from, to) = corners(&harness);
    assert!((to - from).length() < all.length() * 0.6, "{:?} was {nearer:?}", to - from);
    assert!((from + (to - from) / 2.0 - middle).length() < 1.0, "what was in the middle still is");
    assert!(room(&harness, "Chart 2").contains(dots(&harness, "Chart 2")[0]));

    // By hand the chart goes no nearer, and is moved no further, than where it still shows that
    // point, which is on its very edge now: the wheel and a drag that would lose it do nothing.
    let grip = room(&harness, "Chart 2").center() + vec2(0.0, 60.0);
    harness.hover_at(grip);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(6);
    drag(&mut harness, grip, grip + vec2(60.0, -40.0));
    assert!((corners(&harness).0 - from).length() < 0.5 && (corners(&harness).1 - to).length() < 0.5);
    // Dragged the other way, it moves with the pointer.
    drag(&mut harness, grip, grip + vec2(-60.0, 40.0));
    assert!((corners(&harness).1 - to - vec2(-60.0, 40.0)).length() < 1.5);

    // `Fit` has it show all there is again, from edge to edge: twice as far now as at first.
    harness.get_by_label("Fit").click();
    harness.run_steps(3);
    let (from, to) = corners(&harness);
    assert!((to - from - all / 2.0).length() < 1.0 && (from + all / 2.0 - middle).length() < 1.0);

    // Told to keep the newest point of the curve in the middle, the chart does, wherever it goes.
    told(&mut harness, &sensor, "Chart 2", "Keep its newest point in the middle");
    assert!((dots(&harness, "Chart 2")[0] - middle).length() < 1.0);
    is_at(&mut harness, &scene, [20.0, -3.0, 0.0]);
    assert!((dots(&harness, "Chart 2")[0] - middle).length() < 1.0);
    // Moved by hand, the chart lets go of the point.
    drag(&mut harness, grip, grip + vec2(-60.0, 40.0));
    assert!((dots(&harness, "Chart 2")[0] - middle - vec2(-60.0, 40.0)).length() < 1.5);
    is_at(&mut harness, &scene, [21.0, -3.0, 0.0]);
    assert!((dots(&harness, "Chart 2")[0] - middle).length() > 10.0);

    // Whatever its scale, a chart is given more room as ever, by dragging the line between it
    // and its neighbour. With one scale for both axes, it has one still.
    harness.get_by_label("Fit").click();
    harness.get_by_label("1:1").click();
    harness.run_steps(3);
    let (before, border) = (room(&harness, "Chart 2"), name(&harness, "Chart 2").left_center() - vec2(2.0, 0.0));
    drag(&mut harness, border, border - vec2(150.0, 0.0));
    let wider = room(&harness, "Chart 2");
    assert!((wider.width() - before.width() - 150.0).abs() < 8.0, "{wider:?} was {before:?}");
    let trail = &pieces(&harness, "Chart 2")[0].0;
    let (from, to) = (trail[0], trail[1]);
    assert!(((to.x - from.x) / 4.0 + (to.y - from.y) / 2.0).abs() < 0.5, "from {from:?} to {to:?}");
    Ok(())
}

#[test]
fn the_axes_say_which_numbers_are_along_them_unless_told_not_to() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (mut harness, sensor) = plane(&scene)?;
    is_at(&mut harness, &scene, [1.0, 5.0, 0.0]);
    let named = |harness: &Window<'_>, number: &str| {
        let place = room(harness, "Chart 2");
        let names = harness.query_all_by_role_and_label(Role::Label, number);
        names.map(|name| name.rect().center()).find(|name| place.contains(*name))
    };

    // Under the chart, each axis says which number is along it, in the colour of the curve.
    let (x, y) = (named(&harness, "x  translation[0]").expect("x"), named(&harness, "y  translation[1]").expect("y"));
    let place = room(&harness, "Chart 2");
    assert!(x.y > place.bottom() - 20.0 && (x.y - y.y).abs() < 1.0 && y.x > x.x, "{x:?} and {y:?} in {place:?}");

    // The box beside the colour of a curve says whether the axes name its numbers.
    let name = harness.query_all_by_label(sensor.as_str()).find(|name| place.contains(name.rect().center()));
    name.expect("the curve").parent().expect("its row").get_by_role(Role::CheckBox).click();
    harness.run_steps(3);
    assert_eq!((named(&harness, "x  translation[0]"), named(&harness, "y  translation[1]")), (None, None));
    assert!(named(&harness, "x").is_some() && named(&harness, "y").is_some(), "the axes are still told apart");
    assert_eq!(dots(&harness, "Chart 2").len(), 1, "the curve is drawn as before");
    Ok(())
}

#[test]
fn several_curves_share_a_chart_each_with_its_own_numbers() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (mut harness, sensor) = plane(&scene)?;
    // The filter passes on where the sensor is, with half of its first number.
    let filtered = format!("{}/smoother/smooth", scene.robot.name());
    inside(&mut harness, &scene, &filtered);
    let place = room(&harness, "Chart 2");
    let rows = |harness: &Window<'_>| [&sensor, &filtered].map(|curve| written_on(harness, "Chart 2", curve));

    // A number that is put down alone begins a curve of its topic, along x. There is no point to
    // draw until a number is along y as well: the next of the topic that is put down.
    put_down(&mut harness, &filtered, &["translation[0]"], "Chart 2");
    assert_eq!(rows(&harness), [true, true]);
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]", "translation[0]", "drop a number"]);
    is_at(&mut harness, &scene, [4.0, 2.0, 0.0]);
    assert_eq!(dots(&harness, "Chart 2").len(), 1);
    put_down(&mut harness, &filtered, &["translation[1]"], "Chart 2");
    assert_eq!(axes(&harness, "Chart 2")[2..], ["translation[0]", "translation[1]"]);
    // The curve of the sensor has a number along each of its axes. One more of its numbers begins another curve.
    put_down(&mut harness, &sensor, &["translation[2]"], "Chart 2");
    assert_eq!(axes(&harness, "Chart 2")[4..], ["translation[2]", "drop a number"]);
    let last = harness.query_all_by_label("×").rfind(|cross| place.contains(cross.rect().center()));
    last.expect("the cross of the last curve").click();
    harness.run_steps(3);
    assert_eq!(axes(&harness, "Chart 2").len(), 4);

    // Each curve is a dot where it is now, in a colour of its own: the filter's is at half the
    // sensor's x, and level with it.
    let read = |harness: &mut Window<'_>, x: f64| {
        let read = wait_for(|| harness.state().signals.latest(&filtered, "translation[0]") == Some(x));
        assert!(read, "the workbench reads what the filter passes on");
        harness.state_mut().now = present();
        harness.run_steps(2);
    };
    is_at(&mut harness, &scene, [8.0, 2.0, 0.0]);
    read(&mut harness, 4.0);
    fit(&mut harness);
    let mut both = dots(&harness, "Chart 2");
    both.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!(both.len() == 2 && both[1].x > both[0].x + 100.0 && (both[1].y - both[0].y).abs() < 1.0, "{both:?}");
    assert_ne!(colour_of(&harness, &sensor), colour_of(&harness, &filtered));
    // The axes name the numbers of both, until one is told not to be named there.
    assert!(written_on(&harness, "Chart 2", "x  translation[0]  translation[0]"));
    let name = harness.query_all_by_label(filtered.as_str()).find(|name| place.contains(name.rect().center()));
    name.expect("the curve").parent().expect("its row").get_by_role(Role::CheckBox).click();
    harness.run_steps(3);
    assert!(
        written_on(&harness, "Chart 2", "x  translation[0]") && written_on(&harness, "Chart 2", "y  translation[1]")
    );

    // A chart keeps the newest point of one curve in its middle: told to follow another, it lets go of the first.
    let middle = both[0] + (both[1] - both[0]) / 2.0;
    told(&mut harness, &filtered, "Chart 2", "Keep its newest point in the middle");
    let mut followed = dots(&harness, "Chart 2");
    followed.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!((followed[0] - middle).length() < 1.0 && followed[1].x > middle.x + 50.0, "{followed:?}");
    told(&mut harness, &sensor, "Chart 2", "Keep its newest point in the middle");
    let mut followed = dots(&harness, "Chart 2");
    followed.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!((followed[1] - middle).length() < 1.0 && followed[0].x < middle.x - 50.0, "{followed:?}");

    // The eye beside the name of a curve hides it: its name stays, and it is not drawn.
    let named = |harness: &Window<'_>, curve: &str| {
        let names = harness.query_all_by_label(curve).map(|name| name.rect());
        names.into_iter().find(|name| place.contains(name.center())).expect("the curve's name")
    };
    let eye = |harness: &Window<'_>, curve: &str| {
        let name = harness.query_all_by_label(curve).find(|name| place.contains(name.rect().center()));
        name.expect("the curve").parent().expect("its row").get_by_label("Hide or show").click();
    };
    eye(&harness, &filtered);
    harness.run_steps(3);
    assert_eq!((dots(&harness, "Chart 2").len(), rows(&harness)), (1, [true, true]));
    eye(&harness, &filtered);
    harness.run_steps(3);
    assert_eq!(dots(&harness, "Chart 2").len(), 2);
    // Put down on the name of another curve of the chart, a curve has that curve's place among the names.
    assert!(named(&harness, &sensor).top() < named(&harness, &filtered).top());
    let (from, onto) = (named(&harness, &sensor).center(), named(&harness, &filtered).center());
    drag(&mut harness, from, onto);
    assert!(named(&harness, &filtered).top() < named(&harness, &sensor).top());
    assert_eq!(rows(&harness), [true, true]);
    Ok(())
}

/// The colour a curve on the second chart is drawn in, as the button beside its name says it.
fn colour_of(harness: &Window<'_>, curve: &str) -> String {
    let place = room(harness, "Chart 2");
    let name = harness.query_all_by_label(curve).find(|name| place.contains(name.rect().center()));
    let row = name.expect("the curve's name").parent().expect("its row");
    row.get_by_role(Role::ColorWell).value().expect("a colour")
}

#[test]
fn the_trail_of_a_curve_fades_into_the_past_when_told_to() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (mut harness, sensor) = plane(&scene)?;
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    told(&mut harness, &sensor, "Chart 2", "Fade into the past");
    show_the_last(&mut harness, "Chart 2", 2.0);
    harness.get_by_label("1:1").click();
    harness.run_steps(3);

    // The sensor goes round and outwards, for a moment or two.
    for step in 0..60 {
        let (turn, out) = (step as f32 * 0.3, 1.0 + step as f32 * 0.1);
        is_at(&mut harness, &scene, [out * turn.cos(), out * turn.sin(), 0.0]);
    }
    // The trail is drawn in pieces. None is stronger than the one that ends at the dot, where
    // the point is now, and the further back a piece is, the weaker.
    let (trail, dot) = (pieces(&harness, "Chart 2"), dots(&harness, "Chart 2")[0]);
    let strengths: Vec<u8> = trail.iter().map(|(_, color)| color.a()).collect();
    let newest = trail.iter().position(|(points, _)| points.last() == Some(&dot)).expect("a piece ends at the dot");
    assert!(trail.len() > 3 && strengths.iter().all(|strength| *strength <= strengths[newest]), "{strengths:?}");
    assert!(strengths.iter().min() < Some(&(strengths[newest] - 20)), "{strengths:?}");

    // A picture for a human to look at, of all there is.
    fit(&mut harness);
    harness.remove_cursor();
    harness.run_steps(2);
    match harness.render() {
        Ok(picture) => {
            let saved = picture.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("curves.png"));
            saved.map_err(|error| triq::Error::Other(error.to_string()))?;
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }
    Ok(())
}

#[test]
fn a_curve_is_moved_to_another_chart_of_its_kind_and_kept_for_the_next_run() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (mut harness, sensor) = plane(&scene)?;
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    harness.get_by_label("New 2D chart").click();
    harness.run_steps(3);
    let on = |harness: &Window<'_>| ["Chart 1", "Chart 2", "Chart 3"].map(|chart| written_on(harness, chart, &sensor));
    let name = |harness: &Window<'_>, chart: &str| {
        let place = room(harness, chart);
        let names = harness.query_all_by_label(sensor.as_str()).map(|name| name.rect().center());
        names.into_iter().find(|name| place.contains(*name)).expect("the curve's name")
    };
    assert_eq!(on(&harness), [false, true, false]);

    // Taken by its name and put down on the other chart of the kind, the curve is drawn there.
    let (from, third) = (name(&harness, "Chart 2"), room(&harness, "Chart 3").center());
    drag(&mut harness, from, third);
    assert_eq!(on(&harness), [false, false, true]);
    assert_eq!(axes(&harness, "Chart 3"), ["translation[0]", "translation[1]"]);
    // A chart of numbers over time has no place for it: put down there, it stays where it is.
    let (from, first) = (name(&harness, "Chart 3"), room(&harness, "Chart 1").center());
    drag(&mut harness, from, first);
    assert_eq!(on(&harness), [false, false, true]);

    // The charts are kept as they are, and the next run finds them so: the curve, its axes, and
    // that its trail is drawn.
    let third = room(&harness, "Chart 3");
    let equal = harness.query_all_by_label("1:1").find(|button| third.contains(button.rect().center()));
    equal.expect("the button on the chart").click();
    harness.run_steps(3);
    let kept = serde_json::to_string(&harness.state().charts).expect("charts as text");
    drop(harness);
    let mut harness = arranging(&scene, serde_json::from_str(&kept).expect("charts"))?;
    assert_eq!(on(&harness), [false, false, true]);
    assert_eq!(axes(&harness, "Chart 3"), ["translation[0]", "translation[1]"]);
    // The sensor is read for the curve, beside the filter that listens to it.
    let read = wait_for(|| {
        harness.step();
        scene.imu.pose.subscribers() == 2
    });
    assert!(read);
    is_at(&mut harness, &scene, [0.0, 0.0, 0.0]);
    is_at(&mut harness, &scene, [4.0, 2.0, 0.0]);
    let (from, to) = ends(&harness, "Chart 3");
    assert!(((to.x - from.x) / 4.0 + (to.y - from.y) / 2.0).abs() < 0.5, "with one scale for both axes");
    Ok(())
}

#[test]
fn three_numbers_of_one_message_are_drawn_in_space() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let mut harness = arranging(&scene, Charts::default())?;

    // A chart in space is made with a button of its own, and takes three numbers of a message.
    harness.get_by_label("New 3D chart").click();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("three numbers of one message").is_some());
    let sensor = opened(&mut harness, &scene);
    put_down(&mut harness, &sensor, &["translation[0]", "translation[1]", "translation[2]"], "Chart 2");
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]", "translation[2]"]);
    // Several numbers that are put down on a slot go along its axis and the ones after it, in
    // the order they were chosen in.
    let onto_first = |harness: &mut Window<'_>, numbers: &[&str]| {
        for number in numbers {
            let at = grab(harness, &sensor, number);
            click_holding(harness, at, egui::Modifiers::COMMAND);
        }
        let (from, onto) = (grab(harness, &sensor, numbers[0]), slots(harness, "Chart 2")[0].rect().center());
        drag(harness, from, onto);
        for number in numbers {
            let at = grab(harness, &sensor, number);
            click_holding(harness, at, egui::Modifiers::COMMAND);
        }
    };
    onto_first(&mut harness, &["translation[2]", "translation[0]"]);
    assert_eq!(axes(&harness, "Chart 2"), ["translation[2]", "translation[0]", "translation[1]"]);
    onto_first(&mut harness, &["translation[0]", "translation[1]", "translation[2]"]);
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]", "translation[2]"]);
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    // The sensor goes from where every axis is 0 to another place, and then back to halfway between them.
    for place in [[0.0, 0.0, 0.0], [4.0, 2.0, 1.0], [2.0, 1.0, 0.5]] {
        is_at(&mut harness, &scene, place);
    }
    // Where the first two of those places are drawn.
    let corners = |harness: &Window<'_>| {
        let trail = &pieces(harness, "Chart 2")[0].0;
        (trail[0], trail[1])
    };

    // The trail is drawn from the first place to the second, and back to the dot, in the middle.
    fit(&mut harness);
    let place = room(&harness, "Chart 2");
    let (from, to) = corners(&harness);
    let (across, middle) = (to - from, from + (to - from) / 2.0);
    assert!(across.length() > 100.0 && (dots(&harness, "Chart 2")[0] - middle).length() < 1.0, "{from:?} to {to:?}");
    // Under it is the ground, where z is 0, with lines at round values. Their values are
    // written along the axes: x runs to 4 and beyond.
    let lines = |harness: &Window<'_>, width: f32| {
        let lines = drawn(harness).0.into_iter().filter(|line| line.width == width && place.contains_rect(line.within));
        lines.collect::<Vec<_>>()
    };
    let written = |harness: &Window<'_>, value: &str| {
        let found = harness.query_all_by_label(value).find(|written| place.contains(written.rect().center()));
        found.unwrap_or_else(|| panic!("{value} is written on the chart")).rect().center()
    };
    assert!(lines(&harness, 1.0).len() > 8, "{} lines", lines(&harness, 1.0).len());
    let (two, four) = (written(&harness, "2"), written(&harness, "4"));
    // The ground is a part of the world. It stays where it is while the point moves over it.
    let was = dots(&harness, "Chart 2")[0];
    is_at(&mut harness, &scene, [3.0, 1.0, 0.5]);
    assert!((dots(&harness, "Chart 2")[0] - was).length() > 20.0, "the point has moved");
    assert_eq!((written(&harness, "2"), written(&harness, "4")), (two, four), "and the ground has not");
    is_at(&mut harness, &scene, [2.0, 1.0, 0.5]);
    // Where every axis is 0, each of them is drawn, a unit long: that is where the trail began.
    let axes_drawn = lines(&harness, 1.5);
    assert_eq!(axes_drawn.len(), 3);
    assert!(axes_drawn.iter().all(|axis| (axis.points[0] - from).length() < 0.5), "all three leave the origin");
    // The curve stands on the ground: a thin line goes straight down from the dot to a ring
    // there, and says how high the dot is.
    let ring = |harness: &Window<'_>| {
        let rings =
            drawn(harness).1.into_iter().filter(|(_, size, within)| *size == 3.0 && place.contains_rect(*within));
        rings.map(|(middle, ..)| middle).next().expect("a ring on the ground")
    };
    let stands = ring(&harness);
    assert!((stands.x - middle.x).abs() < 0.5 && stands.y > middle.y + 10.0, "{stands:?} under {middle:?}");
    assert!(written_on(&harness, "Chart 2", "z 0.50"));
    // The row under the chart says which number is along each axis.
    for (letter, number) in ["x", "y", "z"].into_iter().zip(axes(&harness, "Chart 2")) {
        assert!(written_on(&harness, "Chart 2", &format!("{letter}  {number}")), "{letter}");
    }

    // Dragged, the chart turns about its middle: the trail points another way, and what was in
    // the middle still is.
    let grip = place.center() + vec2(0.0, 150.0);
    drag(&mut harness, grip, grip + vec2(80.0, 0.0));
    let (from, to) = corners(&harness);
    assert!((to - from - across).length() > 20.0, "it was turned: {:?} was {across:?}", to - from);
    assert!((from + (to - from) / 2.0 - middle).length() < 1.0);
    let across = to - from;
    // With the other button of the mouse, it moves with the pointer instead.
    drag_with(&mut harness, egui::PointerButton::Secondary, grip, grip + vec2(-50.0, 30.0));
    let moved = corners(&harness);
    assert!((moved.0 - from - vec2(-50.0, 30.0)).length() < 1.5 && (moved.1 - moved.0 - across).length() < 0.5);

    // The wheel zooms about the middle, and `Fit` shows all there is again, from where it was turned to.
    harness.hover_at(grip);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(6);
    let nearer = corners(&harness);
    assert!((nearer.1 - nearer.0).length() > across.length() * 1.2);
    harness.get_by_label("Fit").click();
    harness.run_steps(3);
    let (from, to) = corners(&harness);
    assert!((to - from - across).length() < 0.5 && (from + across / 2.0 - middle).length() < 1.0);

    // Which two axes are the ground is said on the chart. With x and z, y is up: the curve
    // stands as high as its y, straight over its ring still.
    let ground = |harness: &mut Window<'_>, was: &str, is: &str| {
        let said = harness.query_all_by_role(Role::ComboBox).find(|said| said.value().as_deref() == Some(was));
        said.expect("the box that says what the ground is").click();
        harness.run_steps(3);
        harness.get_by_role_and_label(Role::Button, is).click();
        harness.run_steps(3);
    };
    ground(&mut harness, "xy", "xz");
    assert!(written_on(&harness, "Chart 2", "y 1.00") && !written_on(&harness, "Chart 2", "z 0.50"));
    let (stands, dot) = (ring(&harness), dots(&harness, "Chart 2")[0]);
    assert!((stands.x - dot.x).abs() < 0.5 && stands.y > dot.y + 10.0, "{stands:?} under {dot:?}");
    ground(&mut harness, "xz", "xy");

    // A picture for a human to look at: the sensor goes round and upwards.
    told(&mut harness, &sensor, "Chart 2", "Fade into the past");
    show_the_last(&mut harness, "Chart 2", 2.0);
    for step in 0..60 {
        let (turn, up) = (step as f32 * 0.3, step as f32 * 0.05);
        is_at(&mut harness, &scene, [2.0 * turn.cos(), 2.0 * turn.sin(), up]);
    }
    fit(&mut harness);
    harness.remove_cursor();
    harness.run_steps(2);
    match harness.render() {
        Ok(picture) => {
            let saved = picture.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("space.png"));
            saved.map_err(|error| triq::Error::Other(error.to_string()))?;
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }
    Ok(())
}

#[test]
fn a_chart_in_space_is_looked_at_from_where_it_was_turned_to() {
    // A room whose shorter side is as long as the diagonal of a box that is 200 points along every edge.
    let room = Rect::from_center_size(pos2(500.0, 400.0), vec2(600.0, 200.0 * 3f32.sqrt()));
    let frame = Frame { middle: [0.0; 3], spans: [2.0, 4.0, 1.0] };
    let quarter = std::f32::consts::FRAC_PI_2;
    let near = |lens: &Lens, point: [f64; 3], expected: egui::Vec2| {
        let found = lens.at(point) - room.center();
        assert!((found - expected).length() < 0.01, "{point:?} is at {found:?}, not at {expected:?}");
    };

    // What the chart reaches across fills the box, each axis along an edge. Looked at along x
    // and level, y is to the right and z is up. x points at the eye, and does not show.
    let level = Lens::space(frame, room, false, (0.0, 0.0), Ground::Xy);
    near(&level, [0.0, 0.0, 0.0], vec2(0.0, 0.0));
    near(&level, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    near(&level, [0.0, 0.0, 0.5], vec2(0.0, -100.0));
    near(&level, [1.0, 0.0, 0.0], vec2(0.0, 0.0));
    // A quarter of the way round, the eye is along y, and x is to the left.
    near(&Lens::space(frame, room, false, (quarter, 0.0), Ground::Xy), [1.0, 0.0, 0.0], vec2(-100.0, 0.0));
    // From straight above, z points at the eye, and x down the screen.
    let above = Lens::space(frame, room, false, (0.0, quarter), Ground::Xy);
    near(&above, [0.0, 0.0, 0.5], vec2(0.0, 0.0));
    near(&above, [1.0, 0.0, 0.0], vec2(0.0, 100.0));
    near(&above, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    // With one scale for every axis, the longest of them fills its edge: 50 points to a unit.
    let equal = Lens::space(frame, room, true, (0.0, 0.0), Ground::Xy);
    near(&equal, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    near(&equal, [0.0, 0.0, 0.5], vec2(0.0, -25.0));

    // Which two axes are the ground is said: the third is up. With x and z along the ground,
    // y is up, and from the same place x is to the right. With y and z, x is up.
    let upright = Lens::space(frame, room, false, (0.0, 0.0), Ground::Xz);
    near(&upright, [0.0, 2.0, 0.0], vec2(0.0, -100.0));
    near(&upright, [1.0, 0.0, 0.0], vec2(100.0, 0.0));
    near(&upright, [0.0, 0.0, 0.5], vec2(0.0, 0.0));
    let on_end = Lens::space(frame, room, false, (0.0, 0.0), Ground::Yz);
    near(&on_end, [1.0, 0.0, 0.0], vec2(0.0, -100.0));
    near(&on_end, [0.0, 0.0, 0.5], vec2(100.0, 0.0));
    assert_eq!([Ground::Xy, Ground::Xz, Ground::Yz].map(Ground::axes), [[0, 1, 2], [2, 0, 1], [1, 2, 0]]);

    // Moved by hand, every point is that much further along on the screen: the middle moves
    // in the plane that the screen is, and not towards the eye.
    let moved = level.moved(vec2(100.0, -100.0));
    let off = |axis: usize| (moved.middle[axis] - [0.0, -2.0, -0.5][axis]).abs();
    assert!((0..3).all(|axis| off(axis) < 1e-6) && moved.spans == frame.spans, "{moved:?}");
    near(&Lens::space(moved, room, false, (0.0, 0.0), Ground::Xy), [0.0, 0.0, 0.0], vec2(100.0, -100.0));
}

#[test]
fn the_lines_of_a_grid_are_at_round_values() {
    // The lines are no closer than asked, and 1, 2 or 5 times a power of ten apart.
    let asked = [0.26, 0.5, 0.51, 3.0, 40.0, 1000.1];
    assert_eq!(asked.map(charts::round_step), [0.5, 0.5, 1.0, 5.0, 50.0, 2000.0]);
    // They are at every multiple of that between two values, and written with as many decimals as it takes.
    assert_eq!(charts::marks(0.9, 3.1, 0.5), [1.0, 1.5, 2.0, 2.5, 3.0]);
    assert_eq!(charts::marks(-1.2, 1.2, 1.0), [-1.0, 0.0, 1.0]);
    assert!(charts::marks(0.0, 1e9, 1.0).is_empty(), "more lines than anyone could tell apart are not drawn");
    let written = [(1.5, 0.5), (20.0, 5.0), (0.25, 0.05), (-0.0, 1.0), (1200.0, 200.0), (-3.0, 0.5)];
    assert_eq!(written.map(|(value, step)| charts::tick(value, step)), ["1.5", "20", "0.25", "0", "1200", "-3.0"]);
}

#[test]
fn a_place_on_the_screen_is_a_point_of_the_ground() {
    let room = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
    let frame = Frame { middle: [2.0, 20.0, 7.0], spans: [4.0, 20.0, 2.0] };
    let near = |found: Option<[f64; 3]>, expected: [f64; 3]| {
        let found = found.expect("a point");
        assert!((0..3).all(|axis| (found[axis] - expected[axis]).abs() < 1e-4), "{found:?}, not {expected:?}");
    };
    // On a plane, the ground is the chart itself: x across and y up, and the third axis at what is asked.
    let flat = Lens::flat(frame, room, false);
    near(flat.on_plane(room.center(), [0, 1, 2], 0.0), [2.0, 20.0, 0.0]);
    near(flat.on_plane(room.left_bottom(), [0, 1, 2], 0.0), [0.0, 10.0, 0.0]);

    // In space, it is where the eye sees the ground through that place: the point of it that is drawn there.
    let space = Lens::space(frame, room, true, (-1.0, 0.5), Ground::Xy);
    for place in [room.center(), room.left_top(), room.right_bottom() + vec2(30.0, 10.0)] {
        let ground = space.on_plane(place, Ground::Xy.axes(), 0.0).expect("a point of the ground");
        assert!(ground[2] == 0.0 && (space.at(ground) - place).length() < 0.01, "{ground:?} for {place:?}");
    }
    // The ground is whichever two axes it is told: with x and z along it, y is up, and 0 there.
    let upright = Lens::space(frame, room, true, (-1.0, 0.5), Ground::Xz);
    let ground = upright.on_plane(room.center(), Ground::Xz.axes(), 0.0).expect("a point of the ground");
    assert!(ground[1] == 0.0 && (upright.at(ground) - room.center()).length() < 0.01, "{ground:?}");
    // Seen from its own level, the ground is a line, and no place on the screen is one point of it.
    assert_eq!(Lens::space(frame, room, true, (-1.0, 0.0), Ground::Xy).on_plane(room.center(), [0, 1, 2], 0.0), None);
}

#[test]
fn a_line_is_cut_to_what_of_it_is_on_the_chart() {
    let room = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
    // A line that crosses the room is cut at its edges, in the direction it had.
    let cut = |line: [egui::Pos2; 2], expected: [egui::Pos2; 2]| {
        let found = charts::clipped(line, room).expect("a part of it");
        assert!((0..2).all(|end| (found[end] - expected[end]).length() < 0.01), "{found:?}, not {expected:?}");
    };
    cut([pos2(0.0, 150.0), pos2(600.0, 150.0)], [pos2(100.0, 150.0), pos2(500.0, 150.0)]);
    cut([pos2(300.0, 400.0), pos2(300.0, 0.0)], [pos2(300.0, 250.0), pos2(300.0, 50.0)]);
    cut([pos2(0.0, 0.0), pos2(600.0, 300.0)], [pos2(100.0, 50.0), pos2(500.0, 250.0)]);
    // One that is inside is as it was, and one that passes the room by is not there.
    cut([pos2(200.0, 100.0), pos2(300.0, 200.0)], [pos2(200.0, 100.0), pos2(300.0, 200.0)]);
    assert_eq!(charts::clipped([pos2(0.0, 0.0), pos2(90.0, 300.0)], room), None);
}

#[test]
fn how_far_out_a_point_is_drawn_says_how_much_further_away_a_chart_must_be_to_show_it() {
    let room = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
    let frame = Frame { middle: [2.0, 20.0, 0.0], spans: [4.0, 20.0, 1.0] };
    let lens = Lens::flat(frame, room, false);
    // In the middle of the room a point is not out at all, and on its edge it is as far out as it can be seen.
    assert_eq!((lens.out(frame.middle), lens.out([4.0, 20.0, 0.0]), lens.out([2.0, 10.0, 0.0])), (0.0, 1.0, 1.0));
    assert_eq!(lens.out([3.0, 25.0, 0.0]), 0.5);
    // One that is twice as far from the middle as the edge is on the edge from twice as far away:
    // about the same middle, and no further than it takes.
    assert_eq!(lens.out([6.0, 20.0, 0.0]), 2.0);
    assert_eq!(Lens::flat(frame.zoomed(1.0 / 2.0), room, false).out([6.0, 20.0, 0.0]), 1.0);
    // The same goes in space, whichever way the chart is turned.
    let turned = |frame: Frame| Lens::space(frame, room, true, (-1.0, 0.5), Ground::Xy);
    let out = turned(frame).out([60.0, 20.0, 30.0]);
    assert!(out > 1.0 && (turned(frame.zoomed(1.0 / out)).out([60.0, 20.0, 30.0]) - 1.0).abs() < 1e-4, "{out}");

    // A chart is shown no nearer than its values can be told apart at: a millionth of how far
    // its middle is from where they are 0.
    let nearest = frame.zoomed(1e30);
    assert!((nearest.spans[2] - 2e-5).abs() < 1e-12 && nearest.spans[0] == 4.0 * nearest.spans[2], "{nearest:?}");
}

#[test]
fn the_ground_of_a_chart_in_space_is_a_part_of_the_world_ten_times_as_large_as_what_is_on_it() {
    // The ground reaches as far from where every axis is 0 as ten times the furthest that any
    // point is from there along an axis, as a round number. It does not depend on how the chart
    // is looked at: it is as large from near by as from far away.
    assert_eq!(charts::ground_reach([[-6.83, 5.15, 2.54]].into_iter()), 100.0);
    assert_eq!(charts::ground_reach([[0.0, 0.0, 2.54], [1.0, -2.0, 0.0]].into_iter()), 50.0);
    assert_eq!(charts::ground_reach([[0.3, 0.0, 0.0]].into_iter()), 5.0);
    // With nothing away from there, it reaches as far as if something were 1 away.
    assert_eq!(charts::ground_reach([[0.0; 3]].into_iter()), 10.0);
    assert_eq!(charts::ground_reach(std::iter::empty()), 10.0);

    // A room whose shorter side is as long as the diagonal of a box that is 200 points along every edge.
    let room = Rect::from_center_size(pos2(500.0, 400.0), vec2(600.0, 200.0 * 3f32.sqrt()));
    let frame = Frame { middle: [10.0, 20.0, 5.0], spans: [2.0, 4.0, 1.0] };
    let near = |found: (f64, f64), expected: (f64, f64)| {
        assert!((found.0 - expected.0).abs() < 1e-4 && (found.1 - expected.1).abs() < 1e-4, "{found:?}");
    };
    // How far apart its lines are goes by how long a unit is on the screen. An axis that is
    // seen from the side is drawn at its full scale. One that points towards the eye is drawn
    // shorter, and one that points straight at it is not drawn at all.
    let (level, above) = ((0.0, 0.0), (0.0, 0.5));
    let level = Lens::space(frame, room, false, level, Ground::Xy);
    near((level.on_screen(1), level.on_screen(2)), (50.0, 200.0));
    assert!(level.on_screen(0) < 1e-3);
    let above = Lens::space(frame, room, false, above, Ground::Xy);
    near((above.on_screen(1), above.on_screen(0)), (50.0, 100.0 * 0.5f64.sin()));
}

#[test]
fn a_trail_with_more_points_than_can_be_told_apart_is_thinned() {
    // Ten thousand points round a circle that is twenty points of the screen across.
    let turn = |step: usize| step as f32 / 10_000.0 * std::f32::consts::TAU;
    let trail: Vec<(egui::Pos2, f32)> =
        (0..10_000).map(|step| (pos2(10.0 * turn(step).cos(), 10.0 * turn(step).sin()), 0.0)).collect();
    let thin = charts::sparse(&trail);
    assert!(thin.len() < 80, "{} points", thin.len());
    assert_eq!((thin[0], thin[thin.len() - 1]), (trail[0], trail[9_999]), "it begins and ends where it did");
    assert!(thin.windows(2).all(|pair| pair[0].0.distance(pair[1].0) < 2.5), "and leaves out no stretch");
    // A point that stays where it is, is drawn once.
    assert_eq!(charts::sparse(&[trail[0]; 500]).len(), 2);
}

#[test]
fn a_chart_shows_all_of_its_points_until_it_is_told_otherwise() {
    let points = [[0.0, 10.0, 0.0], [4.0, 30.0, 0.0], [2.0, 20.0, 0.0]];
    // The middle of them is in the middle of the chart, and it reaches across all of them. Where
    // they do not differ, as on the third axis of a plane, it reaches across 1.
    let all = Frame::around(points.into_iter(), None).expect("points");
    assert_eq!(all, Frame { middle: [2.0, 20.0, 0.0], spans: [4.0, 20.0, 1.0] });
    // Around a point of its own choosing, it reaches as far to either side as the furthest point.
    let followed = Frame::around(points.into_iter(), Some([4.0, 30.0, 0.0])).expect("points");
    assert_eq!(followed, Frame { middle: [4.0, 30.0, 0.0], spans: [8.0, 40.0, 1.0] });
    // A single point has room around it, and no points at all are not shown.
    let one = Frame::around([[1.0, 1.0, 1.0]].into_iter(), None);
    assert_eq!(one, Some(Frame { middle: [1.0; 3], spans: [1.0; 3] }));
    assert_eq!(Frame::around(std::iter::empty(), None), None);
}

#[test]
fn a_chart_on_a_plane_has_x_across_and_y_up() {
    let room = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
    let frame = Frame { middle: [2.0, 20.0, 0.0], spans: [4.0, 20.0, 1.0] };
    // What it shows fills the room: 100 points of the screen to one of x, and 10 to one of y.
    let lens = Lens::flat(frame, room, false);
    assert_eq!(lens.at([2.0, 20.0, 0.0]), room.center());
    assert_eq!((lens.at([0.0, 10.0, 0.0]), lens.at([4.0, 30.0, 0.0])), (room.left_bottom(), room.right_top()));
    assert_eq!(lens.edges(), [(0.0, 4.0), (10.0, 30.0)]);

    // With one scale for both, a unit is as long across as up: 10 points, which the 20 of y
    // need. All that was asked for is still in the room, and more of x to either side of it.
    let lens = Lens::flat(frame, room, true);
    assert_eq!(lens.at([3.0, 21.0, 0.0]) - lens.at([2.0, 20.0, 0.0]), vec2(10.0, -10.0));
    assert_eq!((lens.at([2.0, 10.0, 0.0]), lens.at([2.0, 30.0, 0.0])), (room.center_bottom(), room.center_top()));
    assert_eq!(lens.edges(), [(-18.0, 22.0), (10.0, 30.0)]);
}

#[test]
fn a_chart_is_moved_by_hand_and_zoomed_about_its_middle() {
    let room = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
    let frame = Frame { middle: [2.0, 20.0, 0.0], spans: [4.0, 20.0, 1.0] };
    let lens = Lens::flat(frame, room, false);

    // Dragged 100 points to the right and 20 down, everything is that much further right and down.
    let moved = lens.moved(vec2(100.0, 20.0));
    assert_eq!(moved, Frame { middle: [1.0, 22.0, 0.0], spans: frame.spans });
    assert_eq!(Lens::flat(moved, room, false).at(frame.middle), room.center() + vec2(100.0, 20.0));

    // Zoomed to twice the size, it shows half as much, about the same middle.
    assert_eq!(frame.zoomed(2.0), Frame { middle: frame.middle, spans: [2.0, 10.0, 0.5] });
    assert_eq!(Lens::flat(frame.zoomed(2.0), room, false).at(frame.middle), room.center());
}
