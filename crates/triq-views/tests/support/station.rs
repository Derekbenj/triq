//! Two kinds of view for the tests, and a pretend weather station that sends what they show.
//! Included with `#[path]`.
#![allow(dead_code)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_views::Latest;

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Reading")]
pub struct Reading {
    pub celsius: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Count")]
pub struct Count {
    pub value: u32,
}

#[derive(Ports)]
pub struct Station {
    pub reading: Output<Reading>,
    pub count: Output<Count>,
}

/// A kind of view called `gauge`: the newest reading, in words.
pub fn gauges(runtime: &Runtime) -> triq::Result<Latest<Reading>> {
    Latest::new(runtime, "gauge", |ui, reading: &Reading| _ = ui.label(format!("{:.1} degrees", reading.celsius)))
}

pub fn unique(tag: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("v{}-{}-{tag}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Try until `done` holds; false when three seconds pass first.
pub fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    done()
}
