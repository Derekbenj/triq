//! Windows that are made narrow. What a window holds is laid out in the room
//! the window has, down to a width below which nothing is squeezed any
//! further: from there on, what does not fit is out of sight.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{PointerButton, Rect};
use window::*;

/// The workbench in a window so wide, with the details about a group: where the button
/// beside the group's name is, and where what is written beside the name of the robot is.
fn laid_out(scene: &Scene, width: f32) -> triq::Result<(Rect, Rect)> {
    let mut harness = open_as_wide_as(width)?;
    assert!(until(&mut harness, |harness| harness.query_by_label("smoother.smooth: Pose").is_some()));
    harness.run_steps(4);
    // The card of the group is the first thing called by its name.
    let card = harness.query_all_by_role_and_label(Role::Label, "sensing").next().expect("the card of the group");
    let card = seen(&harness, &card);
    click(&mut harness, card, PointerButton::Primary);
    harness.run_steps(3);
    // A picture of it, for a human to look at. Drawing it needs a GPU.
    match harness.render() {
        Ok(image) => {
            let name = format!("narrow-{width}.png");
            image.save(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).expect("save a picture");
        }
        Err(reason) => eprintln!("no picture: {reason}"),
    }
    let rename = harness.get_by_label("Rename").rect();
    // The robot's name has the number of its process beside it, and so has the desk's. The
    // dot on a card is called by the name of its runtime too: the list of runtimes comes first.
    let robot = harness.query_all_by_label(scene.robot.name()).next().expect("the robot, in the list").rect();
    let process = format!("process {}", std::process::id());
    let beside = harness.query_all_by_label(&process).map(|process| process.rect()).min_by(|a, b| {
        let apart = |rect: &Rect| (rect.center().y - robot.center().y).abs() + (rect.left() - robot.left()).abs();
        apart(a).total_cmp(&apart(b))
    });
    Ok((rename, beside.expect("the number of its process")))
}

#[test]
fn what_a_window_holds_fits_it_until_the_window_is_too_narrow() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    scene.robot.set_group("imu", "sensing")?;
    scene.robot.set_group("smoother", "sensing")?;

    // The windows are side by side: the runtimes on the left, a little under a sixth of the
    // whole across, and the details on the right, a good quarter. In a wide window all is in sight.
    let (wide, process) = laid_out(&scene, 1500.0)?;
    assert!(wide.right() <= 1500.0 && process.right() <= 0.16 * 1500.0);

    // Narrower, each still holds what it held: the name of the group has less room, and its
    // button is still there; the number of the process is under the name of the runtime, not cut off.
    let (fitted, process) = laid_out(&scene, 1100.0)?;
    assert!(fitted.right() <= 1100.0, "the button is in sight: {fitted:?}");
    assert_eq!(fitted.width(), wide.width());
    assert!(process.right() <= 0.16 * 1100.0, "what is beside the runtime's name is in sight: {process:?}");

    // Too narrow, nothing is squeezed any further. The button is as wide as it was, and where it
    // would be in the least room that the details are laid out in: out of sight, in part or whole.
    let (cut, _) = laid_out(&scene, 600.0)?;
    assert_eq!(cut.width(), wide.width());
    assert!(cut.right() > 600.0, "the button is past the edge: {cut:?}");
    Ok(())
}

#[test]
fn the_list_of_numbers_gives_way_to_the_charts_beside_it() -> triq::Result {
    let _alone = alone();
    let _scene = Scene::new()?;
    // The charts are along the bottom, as wide as the window. What heads the list of numbers
    // is as wide as the list, and the button that adds a chart is over the charts beside it.
    let laid_out = |width: f32| -> triq::Result<(f32, Rect)> {
        let mut harness = open_as_wide_as(width)?;
        assert!(until(&mut harness, |harness| harness.query_by_label("New chart").is_some()));
        harness.run_steps(4);
        let list = harness.get_by_label_contains("Drag a number onto a chart").rect().right();
        Ok((list, harness.get_by_label("New chart").rect()))
    };
    let (wide, _) = laid_out(1500.0)?;
    // In a narrow window the list is narrower, and the button is still in sight.
    let (narrow, button) = laid_out(500.0)?;
    assert!(narrow < wide - 50.0, "the list gave way: {narrow} against {wide}");
    assert!(button.right() <= 500.0, "the button is in sight: {button:?}");
    // The list gives way down to a least width, and no further.
    let (narrowest, _) = laid_out(300.0)?;
    assert!(narrowest > 150.0, "the list is still laid out as a list: {narrowest}");
    Ok(())
}
