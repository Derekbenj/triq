//! The list of numbers beside the charts, node by node: what a node hears and sends is folded
//! away under its name until that is clicked, and the list is searched by name.
#[path = "support/charts.rs"]
mod charts;
#[path = "support/scene.rs"]
mod scene;
#[path = "support/window.rs"]
mod window;
use charts::*;
use egui_kittest::kittest::Queryable;
use scene::*;
use triq_workbench::charts::Charts;
use triq_workbench::egui::accesskit::Role;
use triq_workbench::egui::{self, Key, Modifiers};
use window::*;

/// Click the name of a node in the list: its numbers are shown, or folded away again. Other
/// programs' nodes may be listed above it, so it is brought into view first.
fn fold(harness: &mut Window<'_>, node: &str) {
    harness.get_by_label(node).scroll_to_me();
    harness.run_steps(3);
    harness.get_by_label(node).click();
    harness.run_steps(6);
}

/// Write in the box that searches the list, in place of what is written there.
fn search(harness: &mut Window<'_>, words: &str) {
    harness.get_by_role_and_label(Role::TextInput, "Search").click();
    harness.run_steps(2);
    key(harness, Modifiers::COMMAND, Key::A);
    harness.event(egui::Event::Key {
        key: Key::Backspace,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    if !words.is_empty() {
        harness.event(egui::Event::Text(words.to_owned()));
    }
    harness.run_steps(6);
}

#[test]
fn the_numbers_of_a_node_are_folded_away_under_its_name_until_it_is_clicked() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let [sensor, filter, viewer] = [format!("{robot}/imu"), format!("{robot}/smoother"), format!("{desk}/viewer")];
    let entries = [
        format!("{robot}/imu/pose"),
        format!("{robot}/smoother/raw"),
        format!("{robot}/smoother/smooth"),
        format!("{desk}/viewer/pose"),
    ];
    let shown = |harness: &Window<'_>| entries.clone().map(|entry| shows(harness, &entry));
    let mut harness = arranging(&scene, Charts::default())?;

    // The list names each node that has numbers, and nothing of what the nodes hear and send.
    for node in [&sensor, &filter, &viewer] {
        assert!(shows(&harness, node), "{node} is listed");
    }
    assert_eq!(shown(&harness), [false; 4]);

    // Click a node: what it hears and what it sends are listed under it, with their numbers.
    // The other nodes stay as they were.
    fold(&mut harness, &filter);
    assert_eq!(shown(&harness), [false, true, true, false]);
    assert!(number(&harness, &entries[1], "rate").rect().is_positive());
    fold(&mut harness, &viewer);
    assert_eq!(shown(&harness), [false, true, true, true]);
    // Click it again: they are folded away.
    fold(&mut harness, &filter);
    assert_eq!(shown(&harness), [false, false, false, true]);

    // With one runtime chosen in the box, its nodes are listed as they were, open or not, and
    // the others not at all.
    let only = |harness: &mut Window<'_>, runtime: &str| {
        harness.get_by_role(Role::ComboBox).click();
        harness.run_steps(3);
        harness.get_by_role_and_label(Role::Button, runtime).click();
        harness.run_steps(3);
    };
    only(&mut harness, &robot);
    assert!(shows(&harness, &sensor) && shows(&harness, &filter) && !shows(&harness, &viewer));
    assert_eq!(shown(&harness), [false; 4]);
    fold(&mut harness, &sensor);
    assert_eq!(shown(&harness), [true, false, false, false]);
    only(&mut harness, "every runtime");
    assert_eq!(shown(&harness), [true, false, false, true]);
    Ok(())
}

#[test]
fn the_list_is_searched_by_name_and_what_is_found_is_open() -> triq::Result {
    let _alone = alone();
    let scene = Scene::new()?;
    let (robot, desk) = (scene.robot.name().to_owned(), scene.desk.name().to_owned());
    let [sensor, filter, viewer] = [format!("{robot}/imu"), format!("{robot}/smoother"), format!("{desk}/viewer")];
    let entries = [
        format!("{robot}/imu/pose"),
        format!("{robot}/smoother/raw"),
        format!("{robot}/smoother/smooth"),
        format!("{desk}/viewer/pose"),
    ];
    let shown = |harness: &Window<'_>| entries.clone().map(|entry| shows(harness, &entry));
    let nodes = |harness: &Window<'_>| [&sensor, &filter, &viewer].map(|node| shows(harness, node));
    let mut harness = arranging(&scene, Charts::default())?;
    assert_eq!((nodes(&harness), shown(&harness)), ([true; 3], [false; 4]));

    // A runtime's name finds all that runs in it, and what is found is open: there to be dragged.
    search(&mut harness, &desk);
    assert_eq!((nodes(&harness), shown(&harness)), ([false, false, true], [false, false, false, true]));

    // A node's name finds what the node hears and sends, and what hears it: the viewer's input
    // is from the filter. The name is that of this test's robot, as other programs may run.
    search(&mut harness, &filter);
    assert_eq!((nodes(&harness), shown(&harness)), ([false, true, true], [false, true, true, true]));

    // A whole name finds the one thing called so, and what hears it.
    search(&mut harness, &entries[0]);
    assert_eq!((nodes(&harness), shown(&harness)), ([true, true, false], [true, true, false, false]));

    // Several words find what has each of them somewhere, in capitals or not.
    search(&mut harness, &format!("SMOOTH {}", robot.to_uppercase()));
    assert_eq!((nodes(&harness), shown(&harness)), ([false, true, true], [false, false, true, true]));

    // What is found can be folded away, and is dragged onto a chart as ever.
    search(&mut harness, &sensor);
    let (rate, chart) = (grab(&mut harness, &entries[0], "rate"), empty(&harness)[0]);
    drag(&mut harness, rate, chart);
    harness.run_steps(3);
    assert!(empty(&harness).is_empty(), "the number is on the chart");
    fold(&mut harness, &sensor);
    assert_eq!(shown(&harness)[0], false);

    // Nothing is called this: the list says so.
    search(&mut harness, "nothing-on-this-machine-is-called-this");
    assert_eq!(nodes(&harness), [false; 3]);
    assert!(shows(&harness, "Nothing is called that."));

    // With nothing searched for, every node is listed again, folded as it was before.
    search(&mut harness, "");
    assert_eq!((nodes(&harness), shown(&harness)), ([true; 3], [false; 4]));
    Ok(())
}
