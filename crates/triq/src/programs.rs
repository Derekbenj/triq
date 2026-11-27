//! The programs of triq on this machine: those called `triq-<name>`, in the folder of the
//! program that asks or on the `PATH`. The tool runs one by its name, and so does whatever
//! else offers them to a person.
use std::env::consts::EXE_SUFFIX;
use std::path::PathBuf;

/// The folders a program of triq is looked for in: the one this program is in, and then the `PATH`.
fn folders() -> impl Iterator<Item = PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|program| Some(program.parent()?.to_owned()));
    let path: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    beside.into_iter().chain(path)
}

/// Where the program `triq-<name>` is: beside this program, or else on the `PATH`.
pub fn program(name: &str) -> Option<PathBuf> {
    let file = format!("triq-{name}{EXE_SUFFIX}");
    folders().map(|folder| folder.join(&file)).find(|program| program.is_file())
}

/// Every program of triq there is here, by the name it is run by, in the order of their names.
/// Of two of one name, it is the one found first, as [`program`] finds it.
pub fn programs() -> Vec<(String, PathBuf)> {
    let mut programs: Vec<(String, PathBuf)> = Vec::new();
    for folder in folders() {
        let files = std::fs::read_dir(folder).into_iter().flatten().flatten();
        for file in files.filter(|file| file.path().is_file()) {
            let name = file.file_name().to_string_lossy().into_owned();
            let program = name.strip_prefix("triq-").and_then(|name| name.strip_suffix(EXE_SUFFIX));
            // A file with something after a dot is not a program: `triq-workbench.d` says what one was built from.
            let Some(program) = program.filter(|program| !program.is_empty() && !program.contains('.')) else {
                continue;
            };
            if !programs.iter().any(|(known, _)| known == program) {
                programs.push((program.to_owned(), file.path()));
            }
        }
    }
    programs.sort();
    programs
}
