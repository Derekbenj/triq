//! A node's settings as they are edited, and which of them to ask its runtime to change.
//!
//! A runtime can be asked to change some of a node's settings and leave the rest
//! (`Remote::change_params`). What is asked for is what was typed that differs from what the
//! node had when the text was made, and nothing else. A setting that was not touched is never
//! sent, so one that something else has changed meanwhile is not put back.
use serde_json::{Map, Value};

/// What to change in the settings a node `has` for them to be those `wanted`: the settings that
/// differ, named as `Remote::change_params` takes them, with `null` for one that is no longer
/// written. None if nothing differs.
pub fn differences(has: &Value, wanted: &Value) -> Option<Value> {
    if same(has, wanted) {
        return None;
    }
    let (Value::Object(has), Value::Object(wanted)) = (has, wanted) else { return Some(wanted.clone()) };
    let mut named = Map::new();
    for (name, wanted) in wanted {
        let change = has.get(name).map_or_else(|| Some(wanted.clone()), |has| differences(has, wanted));
        if let Some(change) = change {
            named.insert(name.clone(), change);
        }
    }
    // A setting that is no longer written is to be its default again.
    for name in has.keys().filter(|name| !wanted.contains_key(*name)) {
        named.insert(name.clone(), Value::Null);
    }
    Some(Value::Object(named))
}

/// Whether two settings are the same, however their numbers are written: 1 and 1.0 are one number.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b)),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(name, a)| b.get(name).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// `changes` without those that put a setting back to its default; None if nothing is left.
fn without_resets(changes: Value) -> Option<Value> {
    match changes {
        Value::Null => None,
        Value::Object(named) => {
            let kept = named.into_iter().filter_map(|(name, change)| Some((name, without_resets(change)?)));
            let kept: Map<String, Value> = kept.collect();
            (!kept.is_empty()).then_some(Value::Object(kept))
        }
        other => Some(other),
    }
}

/// Settings as text for a person to read and edit.
pub(crate) fn pretty(settings: &Value) -> String {
    serde_json::to_string_pretty(settings).unwrap_or_default()
}

/// A node's settings as the user is editing them.
#[derive(Debug)]
pub struct Editing {
    /// The settings the node had on the machine when the text was last made from them: what a
    /// change is a change from. Null for a node that is not on the machine yet.
    had: Value,
    pub text: String,
}

impl Editing {
    /// For a node that `has` these settings on the machine, or null if it is not there yet, and
    /// is `drawn` with these: the same, unless a draft changes them.
    pub fn new(has: Value, drawn: &Value) -> Self {
        Self { had: has, text: pretty(drawn) }
    }

    /// What the text says, if it is JSON.
    pub fn typed(&self) -> serde_json::Result<Value> {
        serde_json::from_str(&self.text)
    }

    /// What to ask the node's runtime to change for the node to have what is `typed`; None if
    /// it has it all. For a node that is not on the machine yet, that is all of what is typed.
    pub fn changes(&self, typed: &Value) -> Option<Value> {
        differences(&self.had, typed)
    }

    /// Keep up with the node, which now `has` these settings on the machine and is `drawn` with
    /// these. Once its settings have changed, by what was asked here or by anything else, the
    /// text is made again from them, with what was typed and is still to ask for in it. A
    /// setting that was put back to its default is not kept so: only the node knows its
    /// default, and what it shows for the setting now is written.
    ///
    /// While they have not changed the text is left alone: what was asked may not be answered
    /// yet, or was refused, and is there to mend and ask again.
    pub fn follow(&mut self, has: &Value, drawn: &Value) {
        if *has == self.had {
            return;
        }
        // Text that is not JSON is being typed. It waits until it can be read.
        let Ok(typed) = self.typed() else { return };
        let unsent = self.changes(&typed).and_then(without_resets);
        self.had = has.clone();
        let wanted = unsent.map_or_else(|| drawn.clone(), |unsent| triq::changed(drawn.clone(), unsent));
        // If the node has all of it now, nothing is left to ask, and the text is the node's own.
        self.text = pretty(if self.changes(&wanted).is_some() { &wanted } else { drawn });
    }
}
