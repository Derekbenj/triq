//! The graph, handled with a pointer as a user would: move the view, drag a
//! card, click it, pause it from the card, open its menu.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq::NodeState;
use triq_workbench::egui::{self, PointerButton};
use window::*;

fn state(runtime: &triq::Runtime, node: &str) -> NodeState {
    runtime.inventory().nodes.into_iter().find(|info| info.name == node).expect("the node").state
}

#[test]
fn the_graph_is_handled_like_a_user_would() -> triq::Result {
    let scene = Scene::new()?;
    let robot = scene.robot.name().to_owned();
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_by_label("raw: Pose").is_some()), "the cards appear");
    harness.run_steps(4);

    // Scroll over the graph, as two fingers on a trackpad do: the view moves, and stays.
    let before = view(&harness);
    harness.hover_at(seen(&harness, &text(&harness, "raw: Pose")));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-60.0, 45.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(30);
    let after = view(&harness);
    let moved = after.translation - before.translation;
    assert!((moved - egui::vec2(-60.0, 45.0)).length() < 2.0, "the view moved by {moved:?}");
    std::thread::sleep(std::time::Duration::from_millis(600));
    harness.run_steps(4);
    assert_eq!(view(&harness), after, "the view is still there when the machine has reported again");
    // The cards were in the middle of the graph, and the view went down just now: its top corner is empty.
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);

    // A card keeps its shape when data starts to flow and the pins say how fast.
    // Every pin says how fast data passes through it, also when none does.
    let rate = |harness: &Bench<'_>, port| {
        let written = around(harness, port).query_by_label_contains("/s");
        written.and_then(|rate| rate.value())
    };
    let still = |harness: &Bench<'_>| ["raw: Pose", "smooth: Pose"].map(|port| rate(harness, port));
    assert!(until(&mut harness, |harness| still(harness) == [Some("0/s".to_owned()), Some("0/s".to_owned())]));
    let quiet = text(&harness, "smooth: Pose").rect();
    let flowing = until(&mut harness, |harness| {
        scene.imu.pose.send(Pose::default()).expect("send");
        still(harness).iter().all(|rate| rate.as_ref().is_some_and(|rate| rate != "0/s"))
    });
    assert!(flowing, "the pins show rates");
    // A card that grows does so a frame after what is on it changed.
    harness.run_steps(3);
    assert_eq!(text(&harness, "smooth: Pose").rect(), quiet, "nothing on the card moved");

    // Drag the card by the text of one of its ports.
    let place = |harness: &Bench<'_>| {
        harness.state().graph().nodes_pos().find(|(_, card)| card.node.name == "smoother").expect("its card").0
    };
    let (port, was) = (seen(&harness, &text(&harness, "raw: Pose")), place(&harness));
    harness.hover_at(port);
    harness.run_steps(2);
    press(&mut harness, port, PointerButton::Primary, true);
    for step in 1..=5 {
        harness.hover_at(port + egui::vec2(8.0, 6.0) * step as f32);
        harness.run_steps(1);
    }
    press(&mut harness, port + egui::vec2(40.0, 30.0), PointerButton::Primary, false);
    let moved = (place(&harness) - was) * view(&harness).scaling;
    assert!((moved - egui::vec2(40.0, 30.0)).length() < 3.0, "the card moved by {moved:?}");

    // Click it: the details say where the node runs.
    let port = seen(&harness, &text(&harness, "raw: Pose"));
    click(&mut harness, port, PointerButton::Primary);
    let facts = [format!("{robot}, process {}", std::process::id()), "pool dsp: one of its 2 threads".to_owned()];
    for fact in &facts {
        assert!(harness.query_by_label(fact).is_some(), "the details say {fact:?}");
    }
    // Click beside the cards: nothing is selected any more.
    click(&mut harness, empty, PointerButton::Primary);
    assert!(harness.query_by_label(&facts[0]).is_none(), "the details are about nothing");

    // Pause the node with the button on its card, and start it again.
    let button =
        |harness: &Bench<'_>, card, word| around(harness, card).query_by_label(word).map(|it| seen(harness, &it));
    assert!(button(&harness, "imu", "Pause").is_none(), "ports an application merely holds cannot be paused");
    let pause = button(&harness, "smoother", "Pause").expect("a pause button");
    click(&mut harness, pause, PointerButton::Primary);
    assert!(wait_for(|| state(&scene.robot, "smoother") == NodeState::Paused), "its runtime paused it");
    assert!(until(&mut harness, |harness| button(harness, "smoother", "Resume").is_some()), "and the card says so");
    assert!(around(&harness, "smoother").query_by_label("paused").is_some());
    let resume = button(&harness, "smoother", "Resume").unwrap();
    click(&mut harness, resume, PointerButton::Primary);
    assert!(wait_for(|| state(&scene.robot, "smoother") == NodeState::Running));
    assert!(until(&mut harness, |harness| button(harness, "smoother", "Pause").is_some()));

    // Right-click the card: its menu offers what can be done with the node.
    let offered = |harness: &Bench<'_>| ["Pause", "Remove"].map(|word| harness.query_all_by_label(word).count());
    let [pauses, removes] = offered(&harness);
    assert_eq!(removes, 0, "nothing is selected, so nothing offers to remove a node");
    let port = seen(&harness, &text(&harness, "raw: Pose"));
    click(&mut harness, port, PointerButton::Secondary);
    assert_eq!(offered(&harness), [pauses + 1, 1], "the menu opened");
    harness.key_press(egui::Key::Escape);
    harness.remove_cursor();
    harness.run_steps(3);

    // Asked to, the view takes in every card again.
    let scrolled = view(&harness);
    harness.get_by_label("Fit").click();
    harness.run_steps(3);
    assert_ne!(view(&harness), scrolled);

    // Cards wear the colour of the runtime they run in. Looking at colours takes a GPU.
    match harness.render() {
        Ok(picture) => {
            let saved = picture.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("graph.png"));
            saved.map_err(|error| triq::Error::Other(error.to_string()))?;
            // Each card has a dot that stands for its runtime.
            let dot = |card, runtime| {
                let dot = around(&harness, card).query_by_label(runtime).expect("a dot");
                let at = seen(&harness, &dot) * harness.ctx.pixels_per_point();
                *picture.get_pixel(at.x as u32, at.y as u32)
            };
            assert_eq!(dot("imu", &robot), dot("smoother", &robot), "two nodes of one runtime");
            assert_ne!(dot("imu", &robot), dot("viewer", scene.desk.name()), "and a node of another");
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }
    Ok(())
}
