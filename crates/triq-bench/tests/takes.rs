//! The program says what it takes when it is started with `--help`, and ends, as every program
//! of triq does that calls `triq::help` first in its `main`.
use std::process::Command;

#[test]
fn the_program_says_what_it_takes_with_a_line_about_help_and_ends() {
    let said = Command::new(env!("CARGO_BIN_EXE_triq-bench")).arg("--help").output().expect("run it");
    let text = String::from_utf8_lossy(&said.stdout);
    assert!(said.status.success() && text.starts_with("usage: "), "{text}");
    let last = text.lines().last().unwrap_or_default();
    assert!(last.trim_start().starts_with("--help"), "the last line says nothing of --help: {text}");
}

/// Whatever lists programs asks only one that has `--help` written in its file, since another
/// would take the word for something else, and start. What is found there is the line the
/// program says about it: the word it compares what it is told with is not kept as it is
/// written when the program is built for release. So this test is one to run with `--release` too.
#[test]
fn the_file_of_the_program_has_help_written_in_it() {
    let file = std::fs::read(env!("CARGO_BIN_EXE_triq-bench")).expect("the program's file");
    assert!(file.windows(6).any(|part| part == b"--help"), "--help is not written in the program's file");
}
