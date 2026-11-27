//! Programs started from the app: each a process of its own, what it prints kept, and stopped
//! on request. The programs here are small scripts in a folder of the test's own.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use triq_tui::programs::{Listed, Programs, plain};

/// A program of the test's own: a script that does what `script` says.
fn program(tag: &str, name: &str, script: &str) -> (String, PathBuf) {
    let folder = std::env::temp_dir().join(format!("triq-tui-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder");
    let path = folder.join(format!("triq-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("a script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("a program");
    (name.to_owned(), path)
}

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

#[test]
fn a_program_is_started_as_a_process_of_its_own_and_what_it_prints_is_kept() {
    // It says what it was told, on both of its ways out, and goes on running.
    let talker = program("run", "talker", "echo \"told: $*\"\necho 'and this is wrong' >&2\nsleep 30");
    let sulky = program("run", "sulky", "echo bye\nexit 3");
    let mut programs = Programs::new(vec![talker, sulky]);
    assert_eq!(programs.listed(), [Listed::Program(0), Listed::Program(1)]);

    programs.start(0, "quadcopter  --speed 5").expect("start it");
    // It is listed under its program, and runs.
    assert_eq!(programs.listed(), [Listed::Program(0), Listed::Started(0), Listed::Program(1)]);
    assert_eq!(programs.started[0].told, ["quadcopter", "--speed", "5"]);
    assert!(wait_for(|| programs.printed(0).len() == 2), "{:?}", programs.printed(0));
    let mut printed = programs.printed(0);
    printed.sort();
    assert_eq!(printed, ["and this is wrong", "told: quadcopter --speed 5"]);
    programs.poll();
    assert_eq!(programs.started[0].ended, None);

    // One that ends by itself is said to have, with how.
    programs.start(1, "").expect("start it");
    assert!(wait_for(|| {
        programs.poll();
        programs.started[1].ended.is_some()
    }));
    assert_eq!(programs.started[1].ended.as_deref(), Some("ended with status 3"));
    assert_eq!(programs.printed(1), ["bye"]);

    // Stopped, one that is no runtime is ended: there is nobody to ask.
    assert_eq!(programs.stop(0, &[]), None);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[0].ended.is_some()
    }));
    // Those that have ended are forgotten when that is asked.
    programs.forget_ended();
    assert_eq!(programs.listed(), [Listed::Program(0), Listed::Program(1)]);
}

#[test]
fn closing_ends_every_program_that_was_started() {
    let sleeper = program("close", "sleeper", "sleep 30");
    let mut programs = Programs::new(vec![sleeper]);
    programs.start(0, "").expect("start it");
    programs.start(0, "").expect("and another of it");
    let pids: Vec<u32> = programs.started.iter().map(|started| started.pid).collect();
    programs.close(&[]);
    let alive =
        |pid: u32| std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().unwrap().success();
    assert!(wait_for(|| pids.iter().all(|pid| !alive(*pid))), "{pids:?}");
}

#[test]
fn a_line_is_read_without_the_marks_that_colour_it() {
    assert_eq!(
        plain("\u{1b}[2m2026-11-26T05:55Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m camera created"),
        "2026-11-26T05:55Z  INFO camera created"
    );
    assert_eq!(plain("plain as it is"), "plain as it is");
}
