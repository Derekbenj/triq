//! `triq <program>` runs a program of triq: the one called `triq-<program>` that is beside the
//! tool or on the PATH, as `triq workbench` runs `triq-workbench`. The programs here are small
//! scripts in a folder of the test's own, which it puts on the PATH of the tool it runs.
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

/// Run the tool with `folder` on its PATH, before what is there already.
fn triq(folder: &Path, args: &[&str]) -> Output {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(folder.to_owned()).chain(std::env::split_paths(&path))).expect("a PATH");
    Command::new(env!("CARGO_BIN_EXE_triq")).args(args).env("PATH", path).output().expect("run triq")
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
    // Told nothing, it says what it can be told, and the programs it has found, by name.
    let output = triq(&folder, &[]);
    let (_, usage) = said(&output);
    assert!(!output.status.success());
    assert!(usage.contains("triq <program>"), "{usage}");
    let programs = usage.lines().find(|line| line.contains("programs here")).unwrap_or_else(|| panic!("{usage}"));
    assert!(programs.contains("hello") && programs.contains("workbench"), "{programs}");
    assert!(!programs.contains("hello.d"), "{programs}");

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
