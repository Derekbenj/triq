//! `triq <program>` runs a program of triq: the one called `triq-<program>` that is beside the
//! tool, in a folder that was registered, or on the PATH, as `triq workbench` runs
//! `triq-workbench`. The programs here are small scripts in a folder of the test's own, which
//! it puts on the PATH of the tool it runs, or registers. The file that lists the folders
//! registered is one of the test's own too, so that a person's own is neither read nor changed.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A folder of the test's own, with a program `triq-<name>` in it for each of `programs`: a
/// script that says its name and what it was told, and ends with the status given.
fn folder(tag: &str, programs: &[(&str, u8)]) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("triq-programs-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder");
    for (name, status) in programs {
        let path = folder.join(format!("triq-{name}"));
        std::fs::write(&path, format!("#!/bin/sh\necho \"{name} was told: $*\"\nexit {status}\n")).expect("a script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("a program");
    }
    // What is not a program is not taken for one: a file that says what a program was built from.
    std::fs::write(folder.join("triq-hello.d"), "").expect("a file");
    folder
}

/// Run the tool with `folder` on its PATH, before what is there already, and with no folder
/// registered but those in a file beside `folder`.
fn triq(folder: &Path, args: &[&str]) -> Output {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(folder.to_owned()).chain(std::env::split_paths(&path))).expect("a PATH");
    let registered = folder.with_extension("registered");
    let tool =
        Command::new(env!("CARGO_BIN_EXE_triq")).args(args).env("PATH", path).env("TRIQ_FOLDERS", registered).output();
    tool.expect("run triq")
}

fn said(output: &Output) -> (String, String) {
    (String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn a_program_of_triq_is_run_by_its_name_told_what_was_typed_after_it_and_the_tool_ends_as_it_does() {
    let folder = folder("run", &[("hello", 0), ("sulky", 3)]);
    let output = triq(&folder, &["hello", "there", "--loudly", "x y"]);
    assert_eq!(said(&output).0, "hello was told: there --loudly x y\n");
    assert!(output.status.success());
    // How the program ends is how the tool ends.
    let output = triq(&folder, &["sulky"]);
    assert_eq!((said(&output).0.as_str(), output.status.code()), ("sulky was told: \n", Some(3)));
}

#[test]
fn the_tool_says_which_programs_there_are_and_when_there_is_none_of_a_name() {
    let folder = folder("list", &[("hello", 0), ("workbench", 0)]);
    // Told nothing, it says what it can be told, and the programs it has found, by name. That
    // is what was asked of it: it is said plainly, and nothing has gone wrong.
    let output = triq(&folder, &[]);
    let (usage, problem) = said(&output);
    assert!(output.status.success() && problem.is_empty(), "{problem}");
    assert!(usage.starts_with("usage:") && usage.contains("triq <program>"), "{usage}");
    assert_eq!(said(&triq(&folder, &["help"])).0, usage);
    // Each has a line of its own, under what says so, as the commands above have.
    let (_, programs) = usage.split_once("programs here").unwrap_or_else(|| panic!("{usage}"));
    let programs: Vec<&str> = programs.lines().skip(1).take_while(|line| !line.is_empty()).collect();
    for program in ["  hello", "  workbench"] {
        assert!(programs.contains(&program), "{program:?} is not a line of {programs:?}");
    }
    assert!(
        programs.iter().all(|line| line.starts_with("  ") && !line.contains(',') && !line.contains(".d")),
        "{programs:?}"
    );

    // A name that is neither a command nor a program is said to be neither.
    let output = triq(&folder, &["nope", "x"]);
    let (_, error) = said(&output);
    assert!(!output.status.success());
    assert!(error.contains("triq-nope") && error.contains("PATH"), "{error}");
}

#[test]
fn a_command_of_the_tool_is_never_taken_for_a_program() {
    // A program called as a command is does not take its place, whatever it is told.
    let folder = folder("own", &[("list", 0), ("spawn", 0)]);
    let output = triq(&folder, &["list"]);
    assert!(output.status.success());
    assert!(!said(&output).0.contains("was told"), "{:?}", said(&output));
    // A command told too little says what it can be told. It is not handed to a program.
    let output = triq(&folder, &["spawn", "robot"]);
    let (out, usage) = said(&output);
    assert!(!output.status.success() && out.is_empty() && usage.contains("usage:"), "{out}{usage}");
}

#[test]
fn a_folder_is_registered_once_and_its_programs_are_found_from_then_on_with_nothing_on_the_path() {
    // The programs are in a folder that is not on the PATH: the folder on it is an empty one.
    let (empty, kept) = (folder("nopath", &[]), folder("kept", &[("greeter", 0)]));
    assert!(!triq(&empty, &["greeter"]).status.success(), "it is not found before its folder is registered");

    // Registered, by a path that need not be whole, its programs are run by name and listed.
    let part = kept.parent().unwrap().join(".").join(kept.file_name().unwrap());
    let output = triq(&empty, &["register", part.to_str().unwrap()]);
    assert!(output.status.success(), "{:?}", said(&output));
    assert_eq!(said(&triq(&empty, &["greeter", "now"])).0, "greeter was told: now\n");
    assert!(said(&triq(&empty, &[])).0.contains("\n  greeter\n"), "it is listed");
    // The file has the folder once, whole, however often it is registered.
    assert!(triq(&empty, &["register", kept.to_str().unwrap()]).status.success());
    let file = std::fs::read_to_string(empty.with_extension("registered")).expect("the file");
    let whole = kept.canonicalize().unwrap();
    assert_eq!(file.lines().collect::<Vec<_>>(), [whole.to_str().unwrap()]);
    // The tool says where it looks, with the file named.
    assert!(said(&triq(&empty, &[])).0.contains(empty.with_extension("registered").to_str().unwrap()));

    // A folder that is not there is not registered, and that is said.
    let output = triq(&empty, &["register", "/no/such/folder/anywhere"]);
    assert!(!output.status.success() && said(&output).1.contains("/no/such/folder/anywhere"), "{:?}", said(&output));

    // Unregistered, its programs are found no more.
    assert!(triq(&empty, &["unregister", kept.to_str().unwrap()]).status.success());
    assert!(!triq(&empty, &["greeter"]).status.success());
    assert_eq!(std::fs::read_to_string(empty.with_extension("registered")).expect("the file"), "");
}
