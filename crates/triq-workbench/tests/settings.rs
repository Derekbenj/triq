//! A node's settings as they are edited: which of them to ask a runtime to change, so that the
//! ones that were not touched are not sent, and how the text keeps up with the node.
use serde_json::{Value, json};
use triq_workbench::settings::{Editing, differences};

/// What a node has, in these tests: settings of every sort.
fn has() -> Value {
    json!({ "factor": 2, "level": 0.5, "steps": [1, 2], "limits": { "low": 3, "high": 40 }, "name": "a" })
}

#[test]
fn only_the_settings_that_differ_are_named() {
    assert_eq!(differences(&has(), &has()), None, "nothing differs");

    let mut wanted = has();
    wanted["level"] = json!(0.7);
    assert_eq!(differences(&has(), &wanted), Some(json!({ "level": 0.7 })));

    // Any number of them, and those within settings one by one.
    wanted["name"] = json!("b");
    wanted["limits"]["high"] = json!(60);
    assert_eq!(differences(&has(), &wanted), Some(json!({ "level": 0.7, "name": "b", "limits": { "high": 60 } })));

    // A list has no names in it: it is named whole.
    let mut wanted = has();
    wanted["steps"] = json!([1, 2, 3]);
    assert_eq!(differences(&has(), &wanted), Some(json!({ "steps": [1, 2, 3] })));
}

#[test]
fn a_setting_that_is_no_longer_written_goes_back_to_its_default_and_a_new_one_is_named() {
    let mut wanted = has();
    wanted.as_object_mut().unwrap().remove("factor");
    wanted["limits"].as_object_mut().unwrap().remove("low");
    wanted["extra"] = json!(true);
    assert_eq!(differences(&has(), &wanted), Some(json!({ "factor": null, "limits": { "low": null }, "extra": true })));
}

#[test]
fn a_number_written_another_way_is_the_same_number() {
    // The node shows 2 and 0.5; typed as 2.0 and 5e-1 they are what it has.
    let mut wanted = has();
    wanted["factor"] = json!(2.0);
    wanted["level"] = json!(5e-1);
    wanted["steps"] = json!([1.0, 2.0]);
    assert_eq!(differences(&has(), &wanted), None);
    // Whole numbers too large for a fraction to hold are still told apart.
    let (seed, next) = (json!({ "seed": 12345678901234567890u64 }), json!({ "seed": 12345678901234567891u64 }));
    assert_eq!(differences(&seed, &next), Some(next.clone()));
}

#[test]
fn settings_that_are_not_named_ones_are_changed_whole() {
    assert_eq!(differences(&json!(4), &json!(4)), None);
    assert_eq!(differences(&json!(4), &json!(7)), Some(json!(7)));
    assert_eq!(
        differences(&json!({ "limits": 5 }), &json!({ "limits": { "low": 1 } })),
        Some(json!({ "limits": { "low": 1 } }))
    );
    assert_eq!(differences(&json!({ "limits": { "low": 1 } }), &json!({ "limits": 5 })), Some(json!({ "limits": 5 })));
}

#[test]
fn the_differences_made_to_what_a_node_has_give_what_was_wanted() {
    // Whatever was typed, the runtime makes of the differences what was typed.
    let typed = [
        json!({ "factor": 9, "level": 0.5, "steps": [], "limits": { "low": 3, "high": 41 }, "name": "a" }),
        json!({ "level": 0.25, "limits": { "high": 40 }, "new": { "deep": [1, { "x": 2 }] } }),
        json!({}),
        json!({ "limits": 7, "steps": { "now": "named" } }),
        has(),
    ];
    for wanted in typed {
        let made = differences(&has(), &wanted).map_or(has(), |changes| triq::changed(has(), changes));
        assert_eq!(made, wanted);
    }
}

/// The text of settings, as the workbench writes it.
fn written(settings: &Value) -> String {
    serde_json::to_string_pretty(settings).unwrap()
}

#[test]
fn what_is_typed_and_differs_from_what_the_node_had_is_what_is_asked_for() {
    let mut editing = Editing::new(has(), &has());
    assert_eq!(editing.text, written(&has()));
    assert_eq!(editing.changes(&editing.typed().unwrap()), None, "nothing typed, nothing to ask");

    editing.text = editing.text.replace("0.5", "0.75");
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(json!({ "level": 0.75 })));

    editing.text.push_str("not json");
    assert!(editing.typed().is_err());
}

#[test]
fn a_setting_that_changed_on_the_node_meanwhile_is_not_among_the_changes() {
    // The text was made when the factor was 2. Something else has since made it 8, and the
    // text, which still says 2, was not touched there: asking for 2 would undo that change.
    let mut editing = Editing::new(has(), &has());
    editing.text = editing.text.replace("0.5", "0.75");
    let mut now = has();
    now["factor"] = json!(8);
    editing.follow(&now, &now);
    // The text has the node's new factor, and still what was typed and not yet sent.
    let typed = editing.typed().unwrap();
    assert_eq!((&typed["factor"], &typed["level"]), (&json!(8), &json!(0.75)));
    assert_eq!(editing.changes(&typed), Some(json!({ "level": 0.75 })));
}

#[test]
fn once_the_node_has_what_was_typed_the_text_is_the_nodes_own_and_nothing_is_left_to_ask() {
    let mut editing = Editing::new(has(), &has());
    // Typed as a whole number, where the node will show a fraction.
    editing.text = editing.text.replace("0.5", "1");
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(json!({ "level": 1 })));
    let mut now = has();
    now["level"] = json!(1.0);
    editing.follow(&now, &now);
    assert_eq!(editing.text, written(&now));
    assert_eq!(editing.changes(&editing.typed().unwrap()), None);
}

#[test]
fn a_setting_put_back_to_its_default_is_written_as_the_node_then_shows_it() {
    // The factor is no longer written: the node is asked for its default, which only it knows.
    let mut editing = Editing::new(has(), &has());
    let mut typed = has();
    typed.as_object_mut().unwrap().remove("factor");
    typed["limits"].as_object_mut().unwrap().remove("low");
    editing.text = written(&typed);
    assert_eq!(editing.changes(&typed), Some(json!({ "factor": null, "limits": { "low": null } })));
    // What the node shows once it has changed is what it made of that, and nothing is left to ask.
    let mut now = has();
    now["factor"] = json!(1);
    now["limits"]["low"] = json!(0);
    editing.follow(&now, &now);
    assert_eq!((editing.text.clone(), editing.changes(&editing.typed().unwrap())), (written(&now), None));
}

#[test]
fn while_the_node_has_not_changed_the_text_is_left_alone() {
    // Asked and refused, or not yet answered: what was typed stays, to be mended and asked again.
    let mut editing = Editing::new(has(), &has());
    editing.text = editing.text.replace("0.5", "\"half\"");
    let typed = editing.text.clone();
    editing.follow(&has(), &has());
    assert_eq!(editing.text, typed);

    // Text that is not JSON is being typed: it waits, and keeps up once it can be read.
    let mut now = has();
    now["factor"] = json!(8);
    editing.text = "{ \"level\": ".to_owned();
    editing.follow(&now, &now);
    assert_eq!(editing.text, "{ \"level\": ");
    editing.text = written(&has()).replace("0.5", "0.75");
    editing.follow(&now, &now);
    let typed = editing.typed().unwrap();
    assert_eq!((&typed["factor"], &typed["level"]), (&json!(8), &json!(0.75)));
}

#[test]
fn while_drafting_the_changes_are_from_what_the_node_has_on_the_machine() {
    // A draft already changes the level, and the drawing shows it changed. Another setting is
    // typed: the changes are both, from the machine, so that the later draft of this node's
    // settings holds the earlier one too.
    let mut drawn = has();
    drawn["level"] = json!(0.75);
    let mut editing = Editing::new(has(), &drawn);
    assert_eq!(editing.text, written(&drawn));
    editing.text = editing.text.replace("\"a\"", "\"b\"");
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(json!({ "level": 0.75, "name": "b" })));

    // The draft is applied: the machine has both, and nothing is left to ask.
    let mut now = drawn.clone();
    now["name"] = json!("b");
    editing.follow(&now, &now);
    assert_eq!((editing.text.clone(), editing.changes(&editing.typed().unwrap())), (written(&now), None));
}

#[test]
fn a_node_that_is_not_on_the_machine_yet_is_asked_for_all_that_is_typed() {
    let drawn = json!({ "alpha": 0.1 });
    let mut editing = Editing::new(Value::Null, &drawn);
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(drawn.clone()));
    // It is created: from then on it is a node like any other.
    editing.follow(&drawn, &drawn);
    assert_eq!(editing.changes(&editing.typed().unwrap()), None);
}
