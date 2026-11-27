//! Programs started from the page: each a process of its own, on record, printing into a file,
//! and stopped on request; left running, found again by the next that looks. The programs
//! here are small scripts in a folder of the test's own.
#![cfg(unix)]
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use triq::Inventory;
use triq_serve::programs::{Programs, plain, words};
use triq_serve::records::{Record, Records};

/// A program of the test's own: a script that does what `script` says.
fn program(tag: &str, name: &str, script: &str) -> (String, PathBuf) {
    let folder = std::env::temp_dir().join(format!("triq-serve-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder");
    let path = folder.join(format!("triq-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("a script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("a program");
    (name.to_owned(), path)
}

/// A folder for the records of a test, which is empty.
fn kept(tag: &str) -> Records {
    let folder = std::env::temp_dir().join(format!("triq-serve-{}-{tag}-kept", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    Records::in_folder(folder)
}

/// Whether a process is there.
fn alive(pid: u32) -> bool {
    let mut asked = std::process::Command::new("kill");
    asked.args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null());
    asked.status().unwrap().success()
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
    let mut programs = Programs::new(vec![talker, sulky], kept("run"));

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
    // What it printed last is read from its file a moment after it has ended.
    assert!(wait_for(|| programs.printed(1) == ["bye"]));
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
    let mut programs = Programs::new(vec![talker], kept("many"));
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
    programs.close(&[], false);
    programs.forget(None);
    assert!(told(&programs).is_empty());
}

#[test]
fn one_that_is_a_runtime_is_asked_to_shut_down_and_ended_when_it_is_stopped_again() {
    let sleeper = program("runtime", "sleeper", "sleep 30");
    let mut programs = Programs::new(vec![sleeper], kept("runtime"));
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
    let mut programs = Programs::new(vec![talker], kept("words"));
    programs.start(0, "one 'two three'").expect("start it");
    assert!(wait_for(|| programs.printed(0).len() == 2));
    assert_eq!(programs.printed(0), ["<one>", "<two three>"]);
    // A word that begins with ~ is in the person's own folder, as a terminal has it: but not
    // one that is in quotes, or that only has a ~ in it.
    let home = std::env::var("HOME").expect("a home");
    assert_eq!(
        words("register ~/robot/target ~ '~/kept' a~/b \\~/c"),
        ["register", &format!("{home}/robot/target"), &home, "~/kept", "a~/b", "~/c"]
    );
}

#[test]
fn a_command_of_the_tool_is_run_as_a_process_and_listed_apart_from_the_programs() {
    let (_, tool) =
        program("tool", "tool", "echo \"asked: $*\"\nif [ \"$1\" = bad ]; then echo 'triq: no' >&2; exit 1; fi");
    let mut programs = Programs::new(Vec::new(), kept("tool"));
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
fn closing_ends_every_program_that_was_started_and_leaves_no_record() {
    let sleeper = program("close", "sleeper", "sleep 30");
    let mut programs = Programs::new(vec![sleeper], kept("close"));
    programs.start(0, "").expect("start it");
    programs.start(0, "").expect("and another of it");
    let pids: Vec<u32> = programs.started.iter().map(|started| started.pid).collect();
    assert_eq!(programs.records.all().len(), 2);
    programs.close(&[], false);
    assert!(wait_for(|| pids.iter().all(|pid| !alive(*pid))), "{pids:?}");
    assert!(programs.records.all().is_empty() && !programs.records.log(1).exists());
}

#[test]
fn a_program_that_was_started_is_on_record_and_prints_into_a_file() {
    let talker = program("record", "talker", "echo \"told: $*\"\nsleep 30");
    let (name, path) = talker.clone();
    let mut programs = Programs::new(vec![talker], kept("record"));
    let id = programs.start(0, "loud --twice").expect("start it");
    assert!(wait_for(|| programs.printed(0).len() == 1));
    // What it prints is in a file of its own, beside its record.
    let log = programs.records.log(id);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "told: loud --twice\n");
    let records = programs.records.all();
    let record = Record { began: records[0].began, ..records[0].clone() };
    let told = vec!["loud".to_owned(), "--twice".to_owned()];
    assert_eq!(record, Record { id, program: name, path, told, pid: programs.started[0].pid, began: record.began });
    assert!(record.began > 1_700_000_000, "when it was started, by the clock");
    // The page is told where that file is.
    assert_eq!(programs.told(&[])["started"][0]["log"], log.to_str().unwrap());

    // Once it has ended and is forgotten, nothing of it is kept.
    programs.stop(0, &[]);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[0].ended.is_some()
    }));
    assert_eq!(programs.records.all().len(), 1, "one that has ended is on record until it is forgotten");
    programs.forget(Some(id));
    assert!(programs.records.all().is_empty() && !log.exists());
}

#[test]
fn closing_may_leave_what_runs_running_with_its_record() {
    let sleeper = program("leave", "sleeper", "echo hello\nsleep 30");
    let mut programs = Programs::new(vec![sleeper], kept("leave"));
    let id = programs.start(0, "left --alone").expect("start it");
    programs.start(0, "ends").expect("start another");
    programs.ask(std::path::Path::new("sleep"), "30").expect("a command that is still being done");
    let (pid, command) = (programs.started[0].pid, programs.started[2].pid);
    // The second has ended by the time this closes.
    programs.stop(1, &[]);
    assert!(wait_for(|| {
        programs.poll();
        programs.started[1].ended.is_some()
    }));
    programs.close(&[], true);
    // What ran still runs, and is on record for the next to find. Of what had ended nothing is
    // kept; and a command of the tool is this program's own, and ends with it.
    assert!(alive(pid), "it was left running");
    let left: Vec<u64> = programs.records.all().iter().map(|record| record.id).collect();
    assert_eq!(left, [id]);
    assert!(programs.records.log(id).exists() && !programs.records.log(id + 1).exists());
    assert!(wait_for(|| {
        programs.poll();
        programs.started[2].ended.is_some()
    }));
    let _ = command;
    programs.close(&[], false);
    assert!(programs.records.all().is_empty());
}

#[test]
fn a_program_that_another_left_running_is_found_by_its_record_and_stopped() {
    let (name, path) = program("left", "sleeper", "echo hello\nsleep 30");
    let records = kept("left");
    std::fs::create_dir_all(records.folder()).expect("the folder");
    // As a triq-serve that has ended left it: running, no child of this program, printing into
    // its file. A shell starts it and ends, and says which process it is.
    let start = format!("'{}' left --alone >> '{}' 2>&1 & echo $!", path.display(), records.log(4).display());
    let said = std::process::Command::new("sh").args(["-c", &start]).output().expect("a shell");
    let pid: u32 = String::from_utf8_lossy(&said.stdout).trim().parse().expect("the number of the process");
    let began = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let told = vec!["left".to_owned(), "--alone".to_owned()];
    records.keep(&Record { id: 4, program: name.clone(), path: path.clone(), told, pid, began }).expect("its record");

    // The next finds it by its record: the same number, what it was told, what it has printed.
    let mut next = Programs::new(vec![(name, path)], records);
    next.adopt();
    assert_eq!(next.started.len(), 1);
    let told = next.told(&[])["started"][0].clone();
    assert_eq!((&told["id"], &told["program"], &told["pid"]), (&json!(4), &json!("sleeper"), &json!(pid)));
    assert_eq!(
        (&told["told"], &told["ended"], &told["adopted"]),
        (&json!(["left", "--alone"]), &Value::Null, &json!(true))
    );
    assert!(wait_for(|| next.printed(0) == ["hello"]), "{:?}", next.printed(0));
    // One that it starts has a number past the one it found.
    assert_eq!(next.start(0, "newer").expect("start it"), 5);
    next.stop(1, &[]);

    // It is not this program's child, and is stopped all the same.
    assert_eq!(next.stop(0, &[]), None);
    assert!(wait_for(|| {
        next.poll();
        next.started[0].ended.is_some()
    }));
    assert!(!alive(pid));
    assert_eq!(next.told(&[])["started"][0]["ended"], "stopped");
    next.close(&[], false);
}

#[test]
fn a_record_of_a_program_that_is_gone_is_of_one_that_has_ended() {
    let (name, path) = program("gone", "sleeper", "sleep 30");
    let records = kept("gone");
    // A process that has ended: its number is no program's any more.
    let mut gone = std::process::Command::new("true").spawn().expect("a process");
    let pid = gone.id();
    gone.wait().expect("it ends");
    let began = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 100;
    records
        .keep(&Record { id: 5, program: name.clone(), path: path.clone(), told: Vec::new(), pid, began })
        .expect("a record");
    std::fs::write(records.log(5), "its last words\n").expect("what it printed");
    // And a process that is there, but is not the program the record is of.
    records
        .keep(&Record {
            id: 6,
            program: name.clone(),
            path: path.clone(),
            told: Vec::new(),
            pid: std::process::id(),
            began,
        })
        .expect("a record");

    let mut programs = Programs::new(vec![(name, path)], records);
    programs.adopt();
    let started = programs.told(&[])["started"].as_array().unwrap().clone();
    assert_eq!(started.len(), 2);
    for one in &started {
        assert_eq!(
            (&one["ended"], &one["failed"], &one["adopted"]),
            (&json!("ended"), &json!(false), &json!(true)),
            "{one}"
        );
    }
    // What it printed is still to be read, and it ran for about as long as it printed.
    assert!(wait_for(|| programs.printed(0) == ["its last words"]));
    assert!((95..=105).contains(&started[0]["seconds"].as_u64().unwrap()), "{}", started[0]["seconds"]);
    // Adopted again, nothing is there twice.
    programs.adopt();
    assert_eq!(programs.started.len(), 2);
}

#[test]
fn more_of_what_a_program_printed_is_read_from_the_end_of_its_file() {
    // It prints three thousand lines, of which five hundred are kept at hand.
    let prints = "i=0; while [ $i -lt 3000 ]; do echo \"line $i\"; i=$((i+1)); done\nsleep 30";
    let mut programs = Programs::new(vec![program("more", "talker", prints)], kept("more"));
    programs.start(0, "").expect("start it");
    assert!(wait_for(|| programs.printed(0).last().is_some_and(|last| last == "line 2999")));
    assert_eq!(programs.printed(0).len(), 500);
    // Asked for more, the last two thousand are had, whole lines, the newest last.
    let more = programs.printed_more(0);
    assert_eq!((more.len(), more[0].as_str(), more[1999].as_str()), (2000, "line 1000", "line 2999"));
    // One that printed little has all of it.
    let mut quiet = Programs::new(vec![program("more", "quiet", "echo one\necho two\nsleep 30")], kept("quiet"));
    quiet.start(0, "").expect("start it");
    assert!(wait_for(|| quiet.printed(0).len() == 2));
    assert_eq!(quiet.printed_more(0), ["one", "two"]);
    programs.close(&[], false);
    quiet.close(&[], false);
}

#[test]
fn a_line_is_read_without_the_marks_that_colour_it() {
    assert_eq!(
        plain("\u{1b}[2m2026-11-26T05:55Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m camera created"),
        "2026-11-26T05:55Z  INFO camera created"
    );
    assert_eq!(plain("plain as it is"), "plain as it is");
}
