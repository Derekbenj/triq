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
        let made = differences(&has(), &wanted).map_or(has(), |changes| triq::merged(has(), changes));
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
fn text_that_is_being_typed_in_is_never_rewritten_and_only_what_was_typed_is_asked_for() {
    // The text was made when the factor was 2, and the level is being typed. Something else
    // then changes the node, again and again. The text is left exactly as it is, to the
    // character: text rewritten under a caret puts the next key in the wrong place. It still
    // says 2 for the factor, which nobody typed, and 2 is not among the changes: asking for it
    // would undo what the something else did.
    let mut editing = Editing::new(has(), &has());
    editing.text = editing.text.replace("0.5", "0.7");
    let typed = editing.text.clone();
    for factor in 3..13 {
        let mut now = has();
        now["factor"] = json!(factor);
        editing.follow(&now, &now);
        assert_eq!(editing.text, typed);
        assert_eq!(editing.changes(&editing.typed().unwrap()), Some(json!({ "level": 0.7 })));
    }
    // So is a setting that was taken out of the text and not yet asked for, and text that is
    // not JSON at this moment.
    let mut without = has();
    without.as_object_mut().unwrap().remove("name");
    for text in [written(&without), "{ \"level\": ".to_owned()] {
        let mut editing = Editing::new(has(), &has());
        editing.text.clone_from(&text);
        let mut now = has();
        now["factor"] = json!(8);
        editing.follow(&now, &now);
        assert_eq!(editing.text, text);
    }
}

#[test]
fn once_it_is_asked_for_and_the_node_has_answered_the_text_is_the_nodes_own() {
    let mut editing = Editing::new(has(), &has());
    // Typed as a whole number, where the node will show a fraction; and the factor taken out,
    // which asks the node for its default, 1, that only it knows.
    let mut typed = has();
    typed["level"] = json!(1);
    typed.as_object_mut().unwrap().remove("factor");
    editing.text = written(&typed);
    assert_eq!(editing.changes(&typed), Some(json!({ "level": 1, "factor": null })));
    editing.asked();
    // Until the node answers, nothing moves: the text is what was asked for.
    editing.follow(&has(), &has());
    assert_eq!(editing.text, written(&typed));
    // It answers. The text is made again from what it shows, and nothing is left to ask.
    let mut now = has();
    (now["level"], now["factor"]) = (json!(1.0), json!(1));
    editing.follow(&now, &now);
    assert_eq!((editing.text.clone(), editing.changes(&editing.typed().unwrap())), (written(&now), None));
}

#[test]
fn what_was_asked_for_and_refused_stays_to_be_mended_and_asked_again() {
    let mut editing = Editing::new(has(), &has());
    editing.text = editing.text.replace("0.5", "\"half\"").replace("\"a\"", "\"b\"");
    editing.asked();
    // The node refuses all of it, so its settings are what they were, and the text stays.
    editing.follow(&has(), &has());
    assert!(editing.text.contains("\"half\""));
    // Mended, it asks again for both settings: the one that was wrong and the one that was not.
    editing.text = editing.text.replace("\"half\"", "0.25");
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(json!({ "level": 0.25, "name": "b" })));
}

#[test]
fn text_that_nobody_has_typed_in_keeps_up_with_the_node() {
    let mut editing = Editing::new(has(), &has());
    let mut now = has();
    now["factor"] = json!(8);
    editing.follow(&now, &now);
    assert_eq!((editing.text.clone(), editing.changes(&editing.typed().unwrap())), (written(&now), None));
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
    let both = json!({ "level": 0.75, "name": "b" });
    assert_eq!(editing.changes(&editing.typed().unwrap()), Some(both.clone()));

    // It is drafted: the drawing has both, the machine neither, and the changes are still both.
    editing.asked();
    let mut drafted = drawn.clone();
    drafted["name"] = json!("b");
    editing.follow(&has(), &drafted);
    assert_eq!((editing.text.clone(), editing.changes(&editing.typed().unwrap())), (written(&drafted), Some(both)));

    // The draft is applied: the machine has both, and nothing is left to ask.
    let mut applied = editing_after(&has(), &drafted);
    applied.follow(&drafted, &drafted);
    assert_eq!((applied.text.clone(), applied.changes(&applied.typed().unwrap())), (written(&drafted), None));

    // Or the draft is thrown away: the text goes back to the machine's, and asks for nothing
    // of what was drafted.
    let mut discarded = editing_after(&has(), &drafted);
    discarded.follow(&has(), &has());
    assert_eq!((discarded.text.clone(), discarded.changes(&discarded.typed().unwrap())), (written(&has()), None));
}

/// The details of a node that has `has` on the machine and is drawn with `drawn`, untouched.
fn editing_after(has: &Value, drawn: &Value) -> Editing {
    Editing::new(has.clone(), drawn)
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
