//! Clicking a card shows what it is wired to: every card that no wire joins
//! to it goes grey, and is in its colour again when the click is beside the cards.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::{NodeT, Queryable};
use scene::*;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, PointerButton};
use window::*;

/// The colour of the dot on each of some cards, which stands for the card's runtime.
/// `None` where there is no GPU to draw a picture with.
fn dots<const N: usize>(harness: &mut Bench<'_>, cards: [(&str, &str); N]) -> Option<[[u8; 4]; N]> {
    let picture = harness.render().map_err(|reason| eprintln!("no picture: {reason}")).ok()?;
    Some(cards.map(|(card, runtime)| {
        // The details may be headed by the same name. On the card, the name stands beside the dot.
        let named = harness.query_all_by_role_and_label(Role::Label, card).filter_map(|name| name.parent());
        let dot = named.filter_map(|header| header.query_by_label(runtime)).next().expect("a dot");
        let at = seen(harness, &dot) * harness.ctx.pixels_per_point();
        picture.get_pixel(at.x as u32, at.y as u32).0
    }))
}

#[test]
fn clicking_a_card_greys_every_card_that_no_wire_joins_to_it() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // A node that is wired to nothing.
    let _spare: ImuPorts = scene.desk.attach("spare")?;
    let mut harness = open()?;
    assert!(until(&mut harness, |harness| harness.query_all_by_label("spare").next().is_some()), "the cards appear");
    harness.run_steps(4);
    let cards = [("imu", robot.as_str()), ("viewer", desk.as_str()), ("spare", desk.as_str())];
    let Some(coloured) = dots(&mut harness, cards) else { return Ok(()) };
    let grey = |[red, green, blue, _]: [u8; 4]| red == green && green == blue;
    assert!(!coloured.into_iter().any(grey), "every card wears the colour of its runtime: {coloured:?}");
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);

    // Click the filter. The sensor feeds it and it feeds the viewer; nothing joins the spare node to it.
    let filter = seen(&harness, &text(&harness, "raw: Pose"));
    click(&mut harness, filter, PointerButton::Primary);
    harness.run_steps(3);
    let [imu, viewer, spare] = dots(&mut harness, cards).expect("a picture, as before");
    // A picture of it, for a human to look at.
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("focus.png");
    harness.render().expect("a picture").save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?;
    assert_eq!([imu, viewer], coloured[..2], "what is wired to it keeps its colour");
    assert!(grey(spare), "and what is not goes grey: {spare:?}");

    // Click the spare node: now it is the others that are grey.
    let spare = seen(&harness, &text(&harness, "spare"));
    click(&mut harness, spare, PointerButton::Primary);
    harness.run_steps(3);
    let [imu, viewer, spare] = dots(&mut harness, cards).expect("a picture, as before");
    assert!(grey(imu) && grey(viewer) && spare == coloured[2], "{imu:?} {viewer:?} {spare:?}");

    // Click beside the cards: everything is in its colour again.
    click(&mut harness, empty, PointerButton::Primary);
    harness.run_steps(3);
    assert_eq!(dots(&mut harness, cards), Some(coloured));
    Ok(())
}
