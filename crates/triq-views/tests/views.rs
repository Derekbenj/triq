//! The list of views and their panes, drawn without a window. The views are
//! created and wired from outside, as the workbench does it, and arranged with
//! a pointer as a user would.
#[path = "support/station.rs"]
mod station;
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, Node};
use serde_json::json;
use station::*;
use triq::prelude::*;
use triq::{NodeInfo, Remote};
use triq_views::egui::{self, PointerButton, accesskit::Role};
use triq_views::{Latest, Views};

/// The window's state: its views, made with the window, which is there from its first frame on.
type Window<'a> = Harness<'a, Option<Views>>;

/// A window of two kinds of view, in a runtime of that name: made as a program makes its own,
/// from a kind that was written elsewhere, and one that is written on the spot.
fn window(runtime: &str, layout: Option<&str>) -> Window<'static> {
    let (runtime, layout) = (runtime.to_owned(), layout.map(str::to_owned));
    let made = move |window: &egui::Context| -> triq::Result<Views> {
        let mut views = Views::new(Runtime::new(&runtime)?, window);
        views.add(gauges(views.runtime())?);
        let counted = |ui: &mut egui::Ui, count: &Count| _ = ui.label(format!("counted {}", count.value));
        views.add(Latest::new(views.runtime(), "counter", counted)?);
        if let Some(layout) = &layout {
            views.arrange(layout);
        }
        Ok(views)
    };
    let mut made = Some(made);
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).wgpu().build_ui_state(
        move |ui, views: &mut Option<Views>| {
            if let Some(made) = made.take() {
                *views = Some(made(ui.ctx()).expect("the views"));
            }
            views.as_mut().expect("made with the first frame").show(ui);
        },
        None,
    );
    harness.step();
    harness
}

/// How the views are arranged, as the program would keep it for its next run.
fn layout(harness: &Window<'_>) -> Option<String> {
    harness.state().as_ref().expect("made with the first frame").layout()
}

fn shows(harness: &Window<'_>, label: &str) -> bool {
    harness.query_all_by_label_contains(label).next().is_some()
}

/// Draw frames until `done` holds.
fn until(harness: &mut Window<'_>, mut done: impl FnMut(&Window<'_>) -> bool) -> bool {
    wait_for(|| {
        harness.step();
        done(harness)
    })
}

/// Everything in the window that is called by the name of a view or of a group, from the left.
fn called<'a>(harness: &'a Window<'_>, name: &'a str) -> Vec<Node<'a>> {
    let mut called: Vec<_> = harness.query_all_by_role_and_label(Role::Button, name).collect();
    called.sort_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
    called
}

/// A view in the list on the left: what opens and closes it, and what it is taken by to be put somewhere.
fn listed<'a>(harness: &'a Window<'_>, view: &'a str) -> Node<'a> {
    called(harness, view).into_iter().next().unwrap_or_else(|| panic!("no view is called {view:?}"))
}

/// Where the name of a view is on the bar above its pane. A view that is closed has none.
fn pane(harness: &Window<'_>, view: &str) -> Option<egui::Rect> {
    called(harness, view).get(1).map(|name| name.rect())
}

/// Where the bar of a group has its name. A group is not in the list.
fn group(harness: &Window<'_>, name: &str) -> Option<egui::Rect> {
    called(harness, name).first().map(|name| name.rect())
}

/// Press the cross beside the name of a view, on the bar above its pane.
fn close(harness: &mut Window<'_>, view: &str) {
    let name = pane(harness, view).expect("an open view");
    let cross = harness.query_all_by_label("Close").find(|cross| name.contains(cross.rect().center()));
    cross.expect("a cross beside the name").click();
    harness.run_steps(3);
}

/// Press on something, move to somewhere else in a few steps, and let go.
fn drag(harness: &mut Window<'_>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.run_steps(2);
    harness.event(button(from, true));
    harness.run_steps(2);
    for step in 1..=6 {
        harness.hover_at(from + (to - from) * (step as f32 / 6.0));
        harness.run_steps(1);
    }
    harness.event(button(to, false));
    harness.run_steps(10);
}

fn node(remote: &Remote, name: &str) -> Option<NodeInfo> {
    remote.inventory().expect("inventory").nodes.into_iter().find(|node| node.name == name)
}

/// The right and the lower edge of the window: every pane that is at the right or at the bottom ends there.
const EDGE: egui::Pos2 = egui::pos2(1192.0, 800.0);

#[test]
fn every_view_has_a_pane_that_the_user_arranges_and_the_next_run_remembers() -> triq::Result {
    let (home, there) = (unique("views"), unique("station"));
    let station_runtime = Runtime::new(&there)?;
    let station: Station = station_runtime.attach("station")?;
    let mut harness = window(&home, None);
    assert!(shows(&harness, "Views appear here") && shows(&harness, "There are no views yet"));
    assert!(layout(&harness).is_none(), "and there is nothing to remember");

    // The window has no graph. Its runtime offers the kinds of view to whoever looks, which is
    // how the workbench comes to list them. Any program may create one.
    let remote = Remote::open(&home)?;
    let offered: Vec<String> = remote.inventory()?.factories.into_iter().map(|factory| factory.kind).collect();
    assert_eq!(offered, ["counter", "gauge"]);
    remote.spawn("gauge", "attic", Placement::Dedicated, json!(null))?;
    remote.spawn("counter", "clicks", Placement::Dedicated, json!(null))?;
    // Each is listed as soon as it exists, and has a pane, open. They share the room side by side.
    assert!(until(&mut harness, |harness| pane(harness, "attic").is_some() && pane(harness, "clicks").is_some()));
    assert!(!shows(&harness, "Views appear here") && !shows(&harness, "There are no views yet"));
    harness.run_steps(3);
    let (attic, clicks) = (pane(&harness, "attic").unwrap(), pane(&harness, "clicks").unwrap());
    assert!(attic.top() == clicks.top() && clicks.left() > attic.left() + 300.0, "{attic:?} {clicks:?}");
    assert!(listed(&harness, "attic").rect().right() < attic.left(), "the list is on the left");

    // Wired to the station, as dragging in the workbench does it, each draws what it receives in its pane.
    triq::connect(&format!("{there}/station/reading"), &format!("{home}/attic/input"))?;
    triq::connect(&format!("{there}/station/count"), &format!("{home}/clicks/input"))?;
    let drawn = until(&mut harness, |harness| {
        station.reading.send(Reading { celsius: 21.5 }).expect("send");
        station.count.send(Count { value: 3 }).expect("send");
        shows(harness, "21.5 degrees") && shows(harness, "counted 3")
    });
    assert!(drawn);
    let (reading, count) = (harness.get_by_label("21.5 degrees").rect(), harness.get_by_label("counted 3").rect());
    assert!(reading.top() > attic.bottom() && reading.left() < clicks.left() && count.left() > clicks.left());
    let saved = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("views.png");
    match harness.render() {
        Ok(window) => window.save(&saved).map_err(|error| triq::Error::Other(error.to_string()))?,
        Err(reason) => eprintln!("no picture: {reason}"),
    }

    // The cross beside its name closes a view. Its node is still there, and still listed.
    close(&mut harness, "clicks");
    assert!(pane(&harness, "clicks").is_none() && !shows(&harness, "counted 3") && node(&remote, "clicks").is_some());
    // Its name in the list opens it again, where it was, and closes it.
    listed(&harness, "clicks").click();
    harness.run_steps(3);
    assert_eq!(pane(&harness, "clicks"), Some(clicks));
    listed(&harness, "clicks").click();
    harness.run_steps(3);
    assert!(pane(&harness, "clicks").is_none());

    // A view is taken from the list and put down where it should be. Taken, it is open where it
    // was, to the right. Put down on the left half of the other, it is on the other side of it.
    let entry = listed(&harness, "clicks").rect();
    drag(&mut harness, entry.center(), egui::pos2(attic.left() + (clicks.left() - attic.left()) * 0.25, 390.0));
    let (attic, clicks) = (pane(&harness, "attic").unwrap(), pane(&harness, "clicks").unwrap());
    assert!(attic.left() > clicks.left() + 300.0 && clicks.top() == attic.top(), "{attic:?} {clicks:?}");

    // A pane is taken by its name too. A second gauge appears beside the others, and is put down
    // high on the lower half of the first: one under the other.
    remote.spawn("gauge", "cellar", Placement::Dedicated, json!(null))?;
    assert!(until(&mut harness, |harness| pane(harness, "cellar").is_some()));
    harness.run_steps(3);
    let (attic, cellar) = (pane(&harness, "attic").unwrap(), pane(&harness, "cellar").unwrap());
    assert!(cellar.top() == attic.top() && cellar.left() > attic.left(), "beside the others: {cellar:?}");
    // The first gauge is in the middle now, between the counter and the new one.
    drag(&mut harness, cellar.center(), egui::pos2((attic.left() + cellar.left()) / 2.0, 580.0));
    let [attic, clicks, cellar] = ["attic", "clicks", "cellar"].map(|view| pane(&harness, view).unwrap());
    assert!(cellar.top() > attic.top() + 200.0 && cellar.left() == attic.left(), "{attic:?} {cellar:?}");
    assert!(group(&harness, "attic, cellar").is_none(), "neighbours, and no group");

    // Put down low on a half of another, where the highlight is all under that view's bar, a view
    // goes in with it: the two are a group, under a bar that names both.
    let lower_half =
        egui::pos2((clicks.left() + attic.left()) / 2.0, clicks.bottom() + (EDGE.y - clicks.bottom()) * 0.75 + 30.0);
    drag(&mut harness, cellar.center(), lower_half);
    let both = group(&harness, "clicks, cellar").expect("a bar for the two");
    let (inner, under) = (pane(&harness, "clicks").unwrap(), pane(&harness, "cellar").unwrap());
    assert!(both.top() == clicks.top() && inner.top() > both.bottom() - 1.0, "the bar of the one is under the group's");
    assert!(under.top() > inner.top() + 200.0 && under.left() == inner.left(), "and the other is under it");
    // Closed, a view leaves the group to the other, which is under its own bar and no other.
    close(&mut harness, "cellar");
    assert!(group(&harness, "clicks, cellar").is_none() && called(&harness, "clicks").len() == 2);
    // Opened again from the list, it is in the group it was in.
    listed(&harness, "cellar").click();
    harness.run_steps(3);
    assert_eq!((group(&harness, "clicks, cellar"), pane(&harness, "cellar")), (Some(both), Some(under)));

    // A view whose node is removed leaves the window and the list. Should a node of that name be
    // created again, its view is where the other was.
    remote.remove("cellar")?;
    assert!(until(&mut harness, |harness| called(harness, "cellar").is_empty()));
    assert!(group(&harness, "clicks, cellar").is_none() && called(&harness, "clicks").len() == 2);
    remote.spawn("gauge", "cellar", Placement::Dedicated, json!(null))?;
    assert!(until(&mut harness, |harness| pane(harness, "cellar").is_some()));
    assert_eq!((group(&harness, "clicks, cellar"), pane(&harness, "cellar")), (Some(both), Some(under)));

    // The arrangement can be asked for, to be kept for the next run of the program: here with the
    // first view closed, so that the group has the window.
    listed(&harness, "attic").click();
    harness.run_steps(3);
    let arranged =
        ["clicks, cellar", "clicks", "cellar"].map(|name| called(&harness, name).last().map(|name| name.rect()));
    assert!(pane(&harness, "attic").is_none() && arranged.iter().all(Option::is_some));
    let kept = layout(&harness).expect("an arrangement of three views");

    // With every view removed, the window is as it began, and there is nothing to remember:
    // what was kept before is worth more.
    for name in ["attic", "clicks", "cellar"] {
        remote.remove(name)?;
    }
    assert!(until(&mut harness, |harness| shows(harness, "Views appear here")));
    harness.run_steps(3);
    assert!(shows(&harness, "There are no views yet") && harness.query_all_by_label("Close").count() == 0);
    assert!(layout(&harness).is_none());
    drop(harness);

    // The next run: another program, told how the views were arranged. It has no views yet.
    let home = unique("views");
    let mut harness = window(&home, Some(&kept));
    assert!(shows(&harness, "Views appear here") && harness.query_all_by_label("Close").count() == 0);
    // Created again under the names they had, the views are where they were: the group, and
    // the one that was closed is closed.
    let remote = Remote::open(&home)?;
    for (kind, name) in [("gauge", "cellar"), ("gauge", "attic"), ("counter", "clicks")] {
        remote.spawn(kind, name, Placement::Dedicated, json!(null))?;
    }
    assert!(until(&mut harness, |harness| ["attic", "clicks", "cellar"]
        .iter()
        .all(|view| !called(harness, view).is_empty())));
    harness.run_steps(3);
    let again =
        ["clicks, cellar", "clicks", "cellar"].map(|name| called(&harness, name).last().map(|name| name.rect()));
    assert_eq!(again, arranged);
    assert!(pane(&harness, "attic").is_none(), "listed, and closed as it was");
    // Opened, it is beside the group, where it was before it was closed.
    listed(&harness, "attic").click();
    harness.run_steps(3);
    let (attic, both) = (pane(&harness, "attic").unwrap(), group(&harness, "clicks, cellar").unwrap());
    assert!(attic.left() > both.left() + 300.0 && attic.top() == both.top(), "{attic:?} {both:?}");
    // A view that was not there before is beside what is remembered.
    remote.spawn("counter", "steps", Placement::Dedicated, json!(null))?;
    assert!(until(&mut harness, |harness| pane(harness, "steps").is_some()));
    // What cannot be read as an arrangement is passed over, and the views are arranged as they come.
    harness.state_mut().as_mut().expect("the views").arrange("not an arrangement");
    harness.run_steps(3);
    assert!(pane(&harness, "steps").is_some() && pane(&harness, "attic").is_some());
    Ok(())
}
