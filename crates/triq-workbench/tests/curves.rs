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
use triq_workbench::charts::{self, Charts, Frame, Lens};
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

/// The boxes on a chart that say what is along its axes, in the order they are read in: a row
/// that the chart is too narrow for goes on in the next line.
fn boxes<'a>(harness: &'a Window<'_>, chart: &str) -> Vec<egui_kittest::Node<'a>> {
    let room = room(harness, chart);
    let boxes = harness.query_all_by_role(Role::ComboBox).filter(|axis| room.contains(axis.rect().center()));
    let mut boxes: Vec<_> = boxes.collect();
    boxes.sort_by(|a, b| (a.rect().top(), a.rect().left()).partial_cmp(&(b.rect().top(), b.rect().left())).unwrap());
    boxes
}

/// What the boxes on a chart say is along its axes: x first.
fn axes(harness: &Window<'_>, chart: &str) -> Vec<String> {
    boxes(harness, chart).into_iter().map(|axis| axis.value().unwrap_or_default()).collect()
}

/// Tell the box of an axis of a chart which number is along it: 0 is the box of x.
fn along(harness: &mut Window<'_>, chart: &str, axis: usize, number: &str) {
    boxes(harness, chart)[axis].click();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::Button, number).click();
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

    // The newest point is a dot. While it is all there is to show, it is in the middle.
    is_at(&mut harness, &scene, [1.0, 5.0, 0.0]);
    is_at(&mut harness, &scene, [3.0, 9.0, 0.0]);
    let (place, dot) = (room(&harness, "Chart 2"), dots(&harness, "Chart 2"));
    assert!(dot.len() == 1 && (dot[0] - place.center()).length() < 12.0, "{dot:?} in {place:?}");
    assert!(pieces(&harness, "Chart 2").is_empty(), "and no trail is drawn behind it");

    // Told to, the chart draws the path the point took since the curve was put on it: from the
    // lower left, where x and y were smallest, to the upper right. The dot is at its newest end.
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    let (from, to) = ends(&harness, "Chart 2");
    assert!(to.x > from.x + 300.0 && to.y < from.y - 200.0, "from {from:?} to {to:?}");
    assert_eq!(dots(&harness, "Chart 2"), [to]);
    // The edges say what they are: x along the foot, and y at the right edge.
    let written = |harness: &Window<'_>, value: &str| {
        let found = harness.query_all_by_label(value).find(|written| place.contains(written.rect().center()));
        found.unwrap_or_else(|| panic!("{value} is written on the chart")).rect().center()
    };
    let [left, right, bottom, top] = ["1.00", "3.00", "5.00", "9.00"].map(|value| written(&harness, value));
    assert!(left.x < from.x + 30.0 && right.x > to.x - 30.0 && (left.y - right.y).abs() < 1.0 && left.y > from.y);
    assert!(top.y < bottom.y - 200.0 && top.x > to.x - 40.0 && bottom.x > to.x - 40.0);

    // A box is told which number is along its axis. One that was along the other axis trades
    // places with the one that was here: both keep their past, and the trail goes on, the
    // other way up.
    along(&mut harness, "Chart 2", 0, "translation[1]");
    assert_eq!(axes(&harness, "Chart 2"), ["translation[1]", "translation[0]"]);
    let [left, right, bottom, top] = ["5.00", "9.00", "1.00", "3.00"].map(|value| written(&harness, value));
    assert!(left.x < right.x - 300.0 && (left.y - right.y).abs() < 1.0 && top.y < bottom.y - 200.0);
    assert_eq!(pieces(&harness, "Chart 2").len(), 1);
    // A number that was on no chart has no past. The trail begins again with the next messages.
    along(&mut harness, "Chart 2", 1, "translation[2]");
    assert!(pieces(&harness, "Chart 2").is_empty());
    is_at(&mut harness, &scene, [0.0, 2.0, 1.0]);
    is_at(&mut harness, &scene, [0.0, 4.0, 3.0]);
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
    is_at(&mut harness, &scene, [0.0, 0.0, 0.0]);
    is_at(&mut harness, &scene, [4.0, 2.0, 0.0]);
    // To begin with it shows all there is, from edge to edge.
    let (from, to) = ends(&harness, "Chart 2");
    let (all, middle) = (to - from, from + (to - from) / 2.0);
    assert!(all.x > 300.0 && all.y < -200.0, "{all:?}");

    // With one scale for both axes, a unit is as long across as up: the trail goes 4 across and 2 up.
    harness.get_by_label("1:1").click();
    harness.run_steps(3);
    let (from, to) = ends(&harness, "Chart 2");
    assert!(((to.x - from.x) / 4.0 + (to.y - from.y) / 2.0).abs() < 0.5, "from {from:?} to {to:?}");
    harness.get_by_label("1:1").click();
    harness.run_steps(3);
    let (from, to) = ends(&harness, "Chart 2");
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
    let (from, to) = ends(&harness, "Chart 2");
    let nearer = to - from;
    assert!(nearer.x > all.x * 1.2 && nearer.y < all.y * 1.2, "{nearer:?} was {all:?}");
    assert!((from + nearer / 2.0 - middle).length() < 1.0, "what was in the middle still is");
    // From then on the chart stays as it was left: a point further out does not make it show more.
    is_at(&mut harness, &scene, [8.0, 4.0, 0.0]);
    assert!((pieces(&harness, "Chart 2")[0].0[1] - to).length() < 1.0);

    // Dragged, it moves with the pointer.
    let grip = room(&harness, "Chart 2").center() + vec2(0.0, 60.0);
    drag(&mut harness, grip, grip + vec2(-60.0, 40.0));
    assert!((pieces(&harness, "Chart 2")[0].0[1] - to - vec2(-60.0, 40.0)).length() < 1.5);

    // `Fit` has it show all there is again: three points now, from edge to edge.
    harness.get_by_label("Fit").click();
    harness.run_steps(3);
    let (from, to) = ends(&harness, "Chart 2");
    assert!((to - from - all).length() < 1.0 && (from + all / 2.0 - middle).length() < 1.0);

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

    // The number along x is named under the chart, and the one along y at its right edge.
    let (x, y) = (named(&harness, "translation[0]").expect("x"), named(&harness, "translation[1]").expect("y"));
    let place = room(&harness, "Chart 2");
    assert!(x.y > place.bottom() - 20.0 && (x.x - place.center().x).abs() < 60.0, "{x:?} in {place:?}");
    assert!(y.x > place.right() - 80.0 && (y.y - place.center().y).abs() < 40.0, "{y:?} in {place:?}");

    // The box beside the colour of a curve says whether the axes name its numbers.
    let name = harness.query_all_by_label(sensor.as_str()).find(|name| place.contains(name.rect().center()));
    name.expect("the curve").parent().expect("its row").get_by_role(Role::CheckBox).click();
    harness.run_steps(3);
    assert_eq!((named(&harness, "translation[0]"), named(&harness, "translation[1]")), (None, None));
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
    assert_eq!(axes(&harness, "Chart 2"), ["translation[0]", "translation[1]", "translation[0]", "choose"]);
    is_at(&mut harness, &scene, [4.0, 2.0, 0.0]);
    assert_eq!(dots(&harness, "Chart 2").len(), 1);
    put_down(&mut harness, &filtered, &["translation[1]"], "Chart 2");
    assert_eq!(axes(&harness, "Chart 2")[2..], ["translation[0]", "translation[1]"]);
    // The curve of the sensor has a number along each of its axes. One more of its numbers begins another curve.
    put_down(&mut harness, &sensor, &["translation[2]"], "Chart 2");
    assert_eq!(axes(&harness, "Chart 2")[4..], ["translation[2]", "choose"]);
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
    let mut both = dots(&harness, "Chart 2");
    both.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!(both.len() == 2 && both[1].x > both[0].x + 100.0 && (both[1].y - both[0].y).abs() < 1.0, "{both:?}");
    assert_ne!(colour_of(&harness, &sensor), colour_of(&harness, &filtered));
    // The axes name the numbers of both, until one is told not to be named there.
    assert!(written_on(&harness, "Chart 2", "translation[0]  translation[0]"));
    let name = harness.query_all_by_label(filtered.as_str()).find(|name| place.contains(name.rect().center()));
    name.expect("the curve").parent().expect("its row").get_by_role(Role::CheckBox).click();
    harness.run_steps(3);
    assert!(written_on(&harness, "Chart 2", "translation[0]") && written_on(&harness, "Chart 2", "translation[1]"));

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

    // A picture for a human to look at.
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
    told(&mut harness, &sensor, "Chart 2", "Draw its trail");
    is_at(&mut harness, &scene, [0.0, 0.0, 0.0]);
    is_at(&mut harness, &scene, [4.0, 2.0, 1.0]);

    // The trail is drawn from the first place to the second, with the dot at its newest end, in
    // a box that says how far the chart reaches along each axis.
    let place = room(&harness, "Chart 2");
    let (from, to) = ends(&harness, "Chart 2");
    let (across, middle) = (to - from, from + (to - from) / 2.0);
    assert!(across.length() > 100.0 && dots(&harness, "Chart 2") == [to], "from {from:?} to {to:?}");
    let edges = drawn(&harness).0.into_iter().filter(|line| line.width == 1.0 && place.contains_rect(line.within));
    assert_eq!(edges.count(), 12, "the edges of a box");
    for reach in ["x 0.00 to 4.00", "y 0.00 to 2.00", "z 0.00 to 1.00"] {
        assert!(harness.query_by_label_contains(reach).is_some(), "{reach}");
    }
    // The ends of the axes say which they are, and which number is along them.
    for (letter, number) in ["x", "y", "z"].into_iter().zip(axes(&harness, "Chart 2")) {
        assert!(written_on(&harness, "Chart 2", &format!("{letter}  {number}")), "{letter}");
    }

    // Dragged, the chart turns about its middle: the trail points another way, and what was in
    // the middle still is.
    let grip = place.center() + vec2(0.0, 150.0);
    drag(&mut harness, grip, grip + vec2(80.0, 0.0));
    let (from, to) = ends(&harness, "Chart 2");
    assert!((to - from - across).length() > 20.0, "it was turned: {:?} was {across:?}", to - from);
    assert!((from + (to - from) / 2.0 - middle).length() < 1.0);
    let across = to - from;
    // With the other button of the mouse, it moves with the pointer instead.
    drag_with(&mut harness, egui::PointerButton::Secondary, grip, grip + vec2(-50.0, 30.0));
    let moved = ends(&harness, "Chart 2");
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
    let nearer = ends(&harness, "Chart 2");
    assert!((nearer.1 - nearer.0).length() > across.length() * 1.2);
    harness.get_by_label("Fit").click();
    harness.run_steps(3);
    let (from, to) = ends(&harness, "Chart 2");
    assert!((to - from - across).length() < 0.5 && (from + across / 2.0 - middle).length() < 1.0);

    // A picture for a human to look at: the sensor goes round and upwards.
    told(&mut harness, &sensor, "Chart 2", "Fade into the past");
    show_the_last(&mut harness, "Chart 2", 2.0);
    for step in 0..60 {
        let (turn, up) = (step as f32 * 0.3, step as f32 * 0.05);
        is_at(&mut harness, &scene, [2.0 * turn.cos(), 2.0 * turn.sin(), up]);
    }
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
    let level = Lens::space(frame, room, false, (0.0, 0.0));
    near(&level, [0.0, 0.0, 0.0], vec2(0.0, 0.0));
    near(&level, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    near(&level, [0.0, 0.0, 0.5], vec2(0.0, -100.0));
    near(&level, [1.0, 0.0, 0.0], vec2(0.0, 0.0));
    // A quarter of the way round, the eye is along y, and x is to the left.
    near(&Lens::space(frame, room, false, (quarter, 0.0)), [1.0, 0.0, 0.0], vec2(-100.0, 0.0));
    // From straight above, z points at the eye, and x down the screen.
    let above = Lens::space(frame, room, false, (0.0, quarter));
    near(&above, [0.0, 0.0, 0.5], vec2(0.0, 0.0));
    near(&above, [1.0, 0.0, 0.0], vec2(0.0, 100.0));
    near(&above, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    // With one scale for every axis, the longest of them fills its edge: 50 points to a unit.
    let equal = Lens::space(frame, room, true, (0.0, 0.0));
    near(&equal, [0.0, 2.0, 0.0], vec2(100.0, 0.0));
    near(&equal, [0.0, 0.0, 0.5], vec2(0.0, -25.0));

    // Moved by hand, every point is that much further along on the screen: the middle moves
    // in the plane that the screen is, and not towards the eye.
    let moved = level.moved(vec2(100.0, -100.0));
    let off = |axis: usize| (moved.middle[axis] - [0.0, -2.0, -0.5][axis]).abs();
    assert!((0..3).all(|axis| off(axis) < 1e-6) && moved.spans == frame.spans, "{moved:?}");
    near(&Lens::space(moved, room, false, (0.0, 0.0)), [0.0, 0.0, 0.0], vec2(100.0, -100.0));
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
