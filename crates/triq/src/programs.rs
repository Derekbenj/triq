//! The programs of triq on this machine: those called `triq-<name>`, in the folder of the
//! program that asks, in a folder that was registered, or on the `PATH`. The tool runs one by
//! its name, and so does whatever else offers them to a person.
//!
//! A folder is registered once, and its programs are found from then on, from any terminal,
//! with nothing added to the `PATH`. The folders registered are the lines of one file.
use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};

/// The file that lists the folders registered, one to a line: the one `TRIQ_FOLDERS` names,
/// or else `triq/folders` among the person's own settings, `~/.config` unless
/// `XDG_CONFIG_HOME` says otherwise. None for a person with no home.
pub fn registry() -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("TRIQ_FOLDERS") {
        return Some(file.into());
    }
    let home = || std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let settings = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| Some(home()?.join(".config")))?;
    Some(settings.join("triq").join("folders"))
}

/// The folders registered, in the order they were registered in.
pub fn registered() -> Vec<PathBuf> {
    let listed = registry().and_then(|file| std::fs::read_to_string(file).ok()).unwrap_or_default();
    listed.lines().map(str::trim).filter(|line| !line.is_empty()).map(PathBuf::from).collect()
}

/// Write the folders registered, one to a line.
fn keep(folders: &[PathBuf]) -> std::io::Result<()> {
    let file = registry().ok_or_else(|| std::io::Error::other("there is no home to keep the list of folders in"))?;
    if let Some(within) = file.parent() {
        std::fs::create_dir_all(within)?;
    }
    let lines: String = folders.iter().map(|folder| format!("{}\n", folder.display())).collect();
    std::fs::write(file, lines)
}

/// Register a folder: the programs of triq in it are found from now on. It is kept by its
/// whole path, once however often it is registered, and that path is returned. A folder that
/// is not there is not registered.
pub fn register(folder: &Path) -> std::io::Result<PathBuf> {
    let whole = folder.canonicalize()?;
    if !whole.is_dir() {
        return Err(std::io::Error::other("it is not a folder"));
    }
    let mut folders = registered();
    if !folders.contains(&whole) {
        folders.push(whole.clone());
        keep(&folders)?;
    }
    Ok(whole)
}

/// Take a folder out of those registered. False if it was not among them.
pub fn unregister(folder: &Path) -> std::io::Result<bool> {
    let whole = folder.canonicalize().unwrap_or_else(|_| folder.to_owned());
    let mut folders = registered();
    let before = folders.len();
    folders.retain(|known| *known != whole && known != folder);
    if folders.len() != before {
        keep(&folders)?;
    }
    Ok(folders.len() != before)
}

/// The folders a program of triq is looked for in: the one this program is in, those that
/// were registered, and then the `PATH`.
fn folders() -> impl Iterator<Item = PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|program| Some(program.parent()?.to_owned()));
    let path: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    beside.into_iter().chain(registered()).chain(path)
}

/// Where the program `triq-<name>` is: beside this program, in a folder that was registered,
/// or else on the `PATH`.
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
