//! One colour per runtime, so that a glance at the graph tells which program a node runs in.
use std::collections::{BTreeMap, HashSet};
use triq_workbench::colors::Colors;
use triq_workbench::egui::Color32;

#[test]
fn a_runtime_has_a_colour_of_its_own_for_as_long_as_it_runs() {
    let mut colors = Colors::default();
    colors.update(["bevy", "xbox"]);
    let (bevy, xbox) = (colors.of("bevy", true), colors.of("xbox", true));
    assert_ne!(bevy, xbox);

    // Others come and go, even ones listed before it: a runtime keeps its colour.
    colors.update(["alpha", "bevy", "xbox"]);
    let alpha = colors.of("alpha", true);
    assert_eq!((colors.of("bevy", true), colors.of("xbox", true)), (bevy, xbox));
    assert!(alpha != bevy && alpha != xbox);
    colors.update(["alpha", "xbox"]);
    assert_eq!((colors.of("alpha", true), colors.of("xbox", true)), (alpha, xbox));

    // A colour that was given back goes to the next runtime to start.
    colors.update(["alpha", "views", "xbox"]);
    assert_eq!(colors.of("views", true), bevy);
}

#[test]
fn colours_are_shared_only_when_there_are_no_more() {
    let names: Vec<String> = (0..40).map(|number| format!("runtime{number}")).collect();
    let mut colors = Colors::default();
    colors.update(names.iter().map(String::as_str));
    let distinct: HashSet<_> = names.iter().map(|name| colors.of(name, true)).collect();
    assert_eq!(distinct.len(), 16, "sixteen colours that can be told apart");
    let first: HashSet<_> = names[..16].iter().map(|name| colors.of(name, true)).collect();
    assert_eq!(first.len(), 16, "and the first sixteen runtimes have one each");
}

#[test]
fn a_colour_is_darker_on_a_light_background() {
    let mut colors = Colors::default();
    colors.update(["bevy"]);
    let brightness = |dark: bool| {
        let color = colors.of("bevy", dark);
        u32::from(color.r()) + u32::from(color.g()) + u32::from(color.b())
    };
    assert!(brightness(false) < brightness(true), "so that it stands out against white");
}

#[test]
fn a_colour_that_was_chosen_for_a_runtime_is_its_colour_until_it_is_given_back() {
    let mut colors = Colors::default();
    colors.update(["bevy", "xbox"]);
    let (given, xbox) = (colors.of("bevy", true), colors.of("xbox", true));
    let pink = Color32::from_rgb(250, 20, 160);
    colors.choose("bevy", Some(pink));
    // It is drawn as it was chosen, on a dark background and on a light one. The others keep theirs.
    assert_eq!((colors.of("bevy", true), colors.of("bevy", false)), (pink, pink));
    assert_eq!(colors.of("xbox", true), xbox);

    // It is the runtime's colour by name: also when its program has stopped and runs again.
    colors.update(["xbox"]);
    colors.update(["bevy", "xbox"]);
    assert_eq!(colors.of("bevy", true), pink);
    assert_eq!(colors.chosen(), &BTreeMap::from([("bevy".to_owned(), pink)]), "what is kept between runs");

    // Given back, the runtime has a colour of the palette again.
    colors.choose("bevy", None);
    assert!(colors.chosen().is_empty());
    assert_eq!(colors.of("bevy", true), given);
}

#[test]
fn a_group_has_the_colours_of_its_runtimes_mixed() {
    let mut colors = Colors::default();
    colors.update(["bevy", "xbox"]);
    colors.choose("bevy", Some(Color32::from_rgb(0, 0, 200)));
    colors.choose("xbox", Some(Color32::from_rgb(200, 100, 0)));
    // Nodes of one runtime: that runtime's colour. Of two: what lies halfway between them.
    assert_eq!(colors.blend(["bevy"], true), Some(Color32::from_rgb(0, 0, 200)));
    assert_eq!(colors.blend(["bevy", "xbox"], true), Some(Color32::from_rgb(100, 50, 100)));
    assert_eq!(colors.blend([], true), None);
}
