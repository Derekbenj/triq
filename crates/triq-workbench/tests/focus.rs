//! Clicking a card shows where its data comes from and where it goes: every
//! card that neither feeds it nor is fed by it goes grey, and is in its colour
//! again when the click is beside the cards.
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use egui_kittest::kittest::{NodeT, Queryable};
use scene::*;
use triq::prelude::*;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, PointerButton};
use window::*;

/// A node that listens to the sensor, as the filter does.
#[derive(Ports)]
#[allow(dead_code)]
struct ListenerPorts {
    listens: Input<Pose>,
}

/// The colour of the wire that ends at a pin.
fn wire(harness: &Bench<'_>, end: egui::Pos2) -> egui::Color32 {
    fn lines(shape: &egui::Shape, found: &mut Vec<(egui::Pos2, egui::Color32)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| lines(shape, found)),
            egui::Shape::Path(path) if !path.closed => {
                if let (Some(last), egui::epaint::ColorMode::Solid(color)) = (path.points.last(), &path.stroke.color) {
                    found.push((*last, *color));
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    harness.output().shapes.iter().for_each(|clipped| lines(&clipped.shape, &mut found));
    let nearest = found.into_iter().min_by(|a, b| a.0.distance(end).total_cmp(&b.0.distance(end)));
    let (at, color) = nearest.expect("a wire");
    assert!(at.distance(end) < 12.0, "the nearest wire ends at {at:?}, not at the pin at {end:?}");
    color
}

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
fn clicking_a_card_greys_every_card_that_neither_feeds_it_nor_is_fed_by_it() -> triq::Result {
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    // A node that is wired to nothing, and one that hears the sensor as the filter does.
    let _spare: ImuPorts = scene.desk.attach("spare")?;
    let _beside: ListenerPorts = scene.desk.attach("beside")?;
    scene.desk.connect(&format!("{robot}/imu/pose"), "beside/listens")?;
    let mut harness = open()?;
    let shown = |harness: &Bench<'_>| {
        ["spare", "listens: Pose"].iter().all(|label| harness.query_all_by_label(label).next().is_some())
    };
    assert!(until(&mut harness, shown), "the cards appear");
    harness.run_steps(4);
    let cards =
        [("imu", robot.as_str()), ("viewer", desk.as_str()), ("spare", desk.as_str()), ("beside", desk.as_str())];
    let Some(coloured) = dots(&mut harness, cards) else { return Ok(()) };
    let grey = |[red, green, blue, _]: [u8; 4]| red == green && green == blue;
    assert!(!coloured.into_iter().any(grey), "every card wears the colour of its runtime: {coloured:?}");
    let empty = harness.get_by_label("Fit").rect().left_bottom() + egui::vec2(10.0, 20.0);

    let to_the_listener = |harness: &Bench<'_>| wire(harness, pin(harness, "listens: Pose", false));
    assert!(!grey(to_the_listener(&harness).to_array()), "a wire wears the colour of the runtime that sends");

    // Click the filter. The sensor feeds it and it feeds the viewer. The listener only hears
    // the same sensor: none of what passes through the filter comes from it or goes to it.
    let filter = seen(&harness, &text(&harness, "raw: Pose"));
    click(&mut harness, filter, PointerButton::Primary);
    harness.run_steps(3);
    let [imu, viewer, spare, beside] = dots(&mut harness, cards).expect("a picture, as before");
    // A picture of it, for a human to look at.
    let picture = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("focus.png");
    harness.render().expect("a picture").save(&picture).map_err(|error| triq::Error::Other(error.to_string()))?;
    assert_eq!([imu, viewer], coloured[..2], "what feeds it and what it feeds keep their colour");
    assert!(grey(spare) && grey(beside), "and what does neither goes grey: {spare:?} {beside:?}");
    // So does the wire to a card that is grey, though the card it comes from is not.
    assert!(grey(to_the_listener(&harness).to_array()), "{:?}", to_the_listener(&harness));

    // Click the spare node: now it is the others that are grey.
    let spare = seen(&harness, &text(&harness, "spare"));
    click(&mut harness, spare, PointerButton::Primary);
    harness.run_steps(3);
    let [imu, viewer, spare, beside] = dots(&mut harness, cards).expect("a picture, as before");
    assert!(grey(imu) && grey(viewer) && grey(beside) && spare == coloured[2], "{imu:?} {viewer:?} {spare:?}");

    // Click beside the cards: everything is in its colour again.
    click(&mut harness, empty, PointerButton::Primary);
    harness.run_steps(3);
    assert_eq!(dots(&mut harness, cards), Some(coloured));
    Ok(())
}
