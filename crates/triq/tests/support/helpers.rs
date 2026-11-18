//! Shared by the integration tests. Included with `#[path]`, so no `mod.rs`.
#![allow(dead_code)]
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use triq::prelude::*;

/// A child process that is killed when the test ends, however it ends.
pub struct Program(pub Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `examples/station.rs`: a runtime that offers `counter`, `doubler` and `logger` nodes.
pub fn station(runtime: &str) -> Program {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_triq"));
    path.pop();
    path.push("examples/station");
    Program(Command::new(path).arg(runtime).stdout(Stdio::null()).spawn().expect("start a station"))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Pose")]
pub struct Pose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

pub fn pose(x: f32) -> Pose {
    Pose { translation: [x, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0] }
}

/// Service names are machine-wide, so every test runtime gets its own name.
pub fn unique(tag: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("t{}-{}-{tag}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Poll until `done` holds; false when two seconds pass first.
pub fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    done()
}

/// Wait for `count` samples on an input an application holds.
pub fn collect<T: Copy + triq::Payload>(input: &Input<T>, count: usize) -> Vec<T> {
    let mut seen = Vec::new();
    wait_for(|| {
        while let Some(sample) = input.recv().expect("recv") {
            seen.push(*sample);
        }
        seen.len() >= count
    });
    seen
}
