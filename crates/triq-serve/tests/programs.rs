//! Programs started from the page: each a process of its own, what it prints kept, and stopped
//! on request. The programs here are small scripts in a folder of the test's own.
#![cfg(unix)]
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use triq::Inventory;
use triq_serve::programs::{Programs, plain, words};

/// A program of the test's own: a script that does what `script` says.
fn program(tag: &str, name: &str, script: &str) -> (String, PathBuf) {
    let folder = std::env::temp_dir().join(format!("triq-serve-{}-{tag}", std::process::id()));
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

    assert_eq!(programs.start(0, "quadcopter  --speed 5").expect("start it"), 1);
    assert_eq!(programs.started[0].told, ["quadcopter", "--speed", "5"]);
    assert!(wait_for(|| programs.printed(0).len() == 2), "{:?}", programs.printed(0));
    let mut printed = programs.printed(0);
    printed.sort();
    assert_eq!(printed, ["and this is wrong", "told: quadcopter --speed 5"]);
    programs.poll();
    // The page is told of it: that it runs, for how long, and that it is no runtime.
    let told = programs.told(&[])["started"][0].clone();
    assert_eq!((&told["program"], &told["ended"], &told["runtime"]), (&json!("talker"), &Value::Null, &Value::Null));
    assert!(told["seconds"].is_u64() && told["failed"] == false && told["stopping"] == false, "{told}");

    // One that ends by itself is said to have, with how: it failed.
    programs.start(1, "").expect("start it");
    assert!(wait_for(|| {
        programs.poll();
        programs.started[1].ended.is_some()
    }));
    let told = programs.told(&[])["started"][1].clone();
    assert_eq!(
        (&told["ended"], &told["failed"], &told["printed"]),
        (&json!("ended with status 3"), &json!(true), &json!(["bye"]))
    );

    // Stopped, one that is no runtime is ended: there is nobody to ask. It was stopped, and did not fail.
    assert_eq!(programs.stop(0, &[]), None);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[0].ended.is_some()
    }));
    let told = programs.told(&[])["started"][0].clone();
    assert_eq!((&told["ended"], &told["failed"]), (&json!("stopped"), &json!(false)));
}

#[test]
fn several_of_one_program_run_at_once_each_known_by_a_number_of_its_own() {
    let talker = program("many", "talker", "echo \"told: $*\"\nsleep 30");
    let mut programs = Programs::new(vec![talker]);
    let ids: Vec<u64> = ["first", "second", "third"].map(|told| programs.start(0, told).expect("start it")).to_vec();
    assert_eq!(ids, [1, 2, 3]);
    let told = |programs: &Programs| -> Vec<(u64, String, bool)> {
        let started = programs.told(&[])["started"].as_array().unwrap().clone();
        let each = |one: &Value| {
            (one["id"].as_u64().unwrap(), one["told"][0].as_str().unwrap().to_owned(), one["ended"].is_null())
        };
        started.iter().map(each).collect()
    };
    assert_eq!(told(&programs), [(1, "first".into(), true), (2, "second".into(), true), (3, "third".into(), true)]);

    // One of them is stopped by its number, and the others run on.
    let second = programs.place(2).expect("the second");
    programs.stop(second, &[]);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[second].ended.is_some()
    }));
    assert_eq!(told(&programs), [(1, "first".into(), true), (2, "second".into(), false), (3, "third".into(), true)]);
    // One that runs is not forgotten; one that has ended is, by its number or with every other that has.
    programs.forget(Some(1));
    assert_eq!(told(&programs).len(), 3);
    programs.forget(Some(2));
    assert_eq!(told(&programs), [(1, "first".into(), true), (3, "third".into(), true)]);
    programs.forget(None);
    assert_eq!(told(&programs).len(), 2);
    programs.close(&[]);
    programs.forget(None);
    assert!(told(&programs).is_empty());
}

#[test]
fn one_that_is_a_runtime_is_asked_to_shut_down_and_ended_when_it_is_stopped_again() {
    let sleeper = program("runtime", "sleeper", "sleep 30");
    let mut programs = Programs::new(vec![sleeper]);
    programs.start(0, "").expect("start it");
    // The machine has a runtime in that process, with two nodes, as the feed would see it.
    let node = |name: &str| triq::NodeInfo {
        name: name.to_owned(),
        kind: String::new(),
        group: String::new(),
        placement: None,
        state: triq::NodeState::Running,
        params: Value::Null,
        runs: 0,
        errors: 0,
        last_error: None,
        inputs: Vec::new(),
        outputs: Vec::new(),
    };
    let machine = [Inventory {
        runtime: "sleepy".to_owned(),
        pid: programs.started[0].pid,
        revision: 0,
        taken_ns: 0,
        pools: Vec::new(),
        factories: Vec::new(),
        nodes: vec![node("a"), node("b")],
    }];
    let told = programs.told(&machine)["started"][0].clone();
    assert_eq!((&told["runtime"], &told["nodes"]), (&json!("sleepy"), &json!(2)));

    // Stopped, its runtime is the one to ask: it clears its own things away. It is not ended yet.
    assert_eq!(programs.stop(0, &machine), Some("sleepy".to_owned()));
    programs.poll();
    let told = programs.told(&machine)["started"][0].clone();
    assert_eq!((&told["ended"], &told["stopping"]), (&Value::Null, &json!(true)));
    // Stopped again, because it did not shut down, it is ended.
    assert_eq!(programs.stop(0, &machine), None);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[0].ended.is_some()
    }));
    assert_eq!(programs.told(&machine)["started"][0]["ended"], "stopped");
}

#[test]
fn what_is_typed_is_split_into_words_as_a_terminal_splits_it() {
    assert_eq!(words("connect  robot/imu/pose\tdesk/viewer/input"), ["connect", "robot/imu/pose", "desk/viewer/input"]);
    // What is between quotes is one word, spaces and all, and the quotes are not part of it.
    let spawn = r#"spawn physics multirotor quad --params '{"position": [0, 0, -1]}'"#;
    assert_eq!(words(spawn), ["spawn", "physics", "multirotor", "quad", "--params", r#"{"position": [0, 0, -1]}"#]);
    assert_eq!(words(r#"a "b c"d 'it''s' e\ f """#), ["a", "b cd", "its", "e f", ""]);
    assert_eq!(words("   "), Vec::<String>::new());
    // A program is told the words in the same way.
    let talker = program("words", "talker", "for word in \"$@\"; do echo \"<$word>\"; done");
    let mut programs = Programs::new(vec![talker]);
    programs.start(0, "one 'two three'").expect("start it");
    assert!(wait_for(|| programs.printed(0).len() == 2));
    assert_eq!(programs.printed(0), ["<one>", "<two three>"]);
}

#[test]
fn a_command_of_the_tool_is_run_as_a_process_and_listed_apart_from_the_programs() {
    let (_, tool) =
        program("tool", "tool", "echo \"asked: $*\"\nif [ \"$1\" = bad ]; then echo 'triq: no' >&2; exit 1; fi");
    let mut programs = Programs::new(Vec::new());
    let first = programs.ask(&tool, "connect a/b/c 'd e'").expect("run it");
    let second = programs.ask(&tool, "bad idea").expect("run it");
    assert_eq!((first, second), (1, 2));
    let mut told = Value::Null;
    assert!(wait_for(|| {
        programs.poll();
        told = programs.told(&[]);
        told["asked"].as_array().is_some_and(|asked| asked.iter().all(|one| !one["ended"].is_null()))
    }));
    // They are no programs that were started: the page lists them where the commands are typed.
    assert_eq!(told["started"], json!([]));
    let (done, refused) = (&told["asked"][0], &told["asked"][1]);
    assert_eq!(
        (&done["told"], &done["printed"]),
        (&json!(["connect", "a/b/c", "d e"]), &json!(["asked: connect a/b/c d e"]))
    );
    assert_eq!((&done["ended"], &done["failed"]), (&json!("ended"), &json!(false)));
    assert_eq!((&refused["ended"], &refused["failed"]), (&json!("ended with status 1"), &json!(true)));
    assert!(refused["printed"].as_array().unwrap().contains(&json!("triq: no")));

    // Only the last of them are kept: what was asked long ago is forgotten by itself.
    for _ in 0..30 {
        programs.ask(&tool, "list").expect("run it");
        assert!(wait_for(|| {
            programs.poll();
            programs.started.iter().all(|one| one.ended.is_some())
        }));
    }
    let kept = programs.told(&[])["asked"].as_array().unwrap().len();
    assert!((1..=20).contains(&kept), "{kept}");
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
