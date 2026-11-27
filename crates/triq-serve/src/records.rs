//! What is kept on disk of each program that was started, so that a program that is left
//! running when this one ends is found again by the next: a small file that says what it is,
//! and beside it the file the program prints into. Both are named by the number the program is
//! known by, `7.json` and `7.log`, in one folder, which a person may choose.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Make a folder, and those it is in that are not there yet, each for its owner to use and
/// for others to read.
///
/// A folder is made with what the mask of the process leaves of the mode asked for, and that
/// mask is the whole process's: iceoryx2 narrows it for the moment in which it binds a socket,
/// on whatever thread that is. A folder made in that moment could not be gone into. So each
/// folder that is made here is given its mode again once it is there.
pub fn make(folder: &Path) -> std::io::Result<()> {
    let made: Vec<&Path> = folder.ancestors().take_while(|part| !part.exists()).collect();
    std::fs::create_dir_all(folder)?;
    #[cfg(unix)]
    for part in made {
        std::fs::set_permissions(part, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    }
    Ok(())
}

/// What is kept of one program that was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: u64,
    /// The name it is run by, and where the program is.
    pub program: String,
    pub path: PathBuf,
    /// What it was told when it was started.
    pub told: Vec<String>,
    pub pid: u32,
    /// When it was started, in seconds since 1970.
    pub began: u64,
}

impl Record {
    fn read(text: &str) -> Option<Self> {
        let kept: Value = serde_json::from_str(text).ok()?;
        let told =
            kept["told"].as_array()?.iter().map(|word| Some(word.as_str()?.to_owned())).collect::<Option<_>>()?;
        Some(Self {
            id: kept["id"].as_u64()?,
            program: kept["program"].as_str()?.to_owned(),
            path: kept["path"].as_str()?.into(),
            told,
            pid: kept["pid"].as_u64()?.try_into().ok()?,
            began: kept["began"].as_u64()?,
        })
    }
}

/// The folder the records are in.
pub struct Records {
    folder: PathBuf,
}

impl Records {
    /// The records of a folder, which need not be there yet: it is made when one is kept.
    pub fn in_folder(folder: impl Into<PathBuf>) -> Self {
        Self { folder: folder.into() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The file the program of that number prints into.
    pub fn log(&self, id: u64) -> PathBuf {
        self.folder.join(format!("{id}.log"))
    }

    fn file(&self, id: u64) -> PathBuf {
        self.folder.join(format!("{id}.json"))
    }

    /// The numbers that files of the folder are named by, each with the file.
    fn numbered(&self) -> Vec<(u64, PathBuf)> {
        let files = std::fs::read_dir(&self.folder).into_iter().flatten().flatten().map(|file| file.path());
        files.filter_map(|file| Some((file.file_stem()?.to_str()?.parse().ok()?, file))).collect()
    }

    /// A number that nothing in the folder is named by: one more than the highest.
    pub fn free(&self) -> u64 {
        self.numbered().into_iter().map(|(id, _)| id).max().unwrap_or(0) + 1
    }

    pub fn keep(&self, record: &Record) -> std::io::Result<()> {
        make(&self.folder)?;
        let kept = json!({
            "id": record.id,
            "program": record.program,
            "path": record.path,
            "told": record.told,
            "pid": record.pid,
            "began": record.began,
        });
        std::fs::write(self.file(record.id), format!("{kept:#}\n"))
    }

    /// Every record of the folder, in the order of their numbers. What is no record is passed over.
    pub fn all(&self) -> Vec<Record> {
        let records =
            self.numbered().into_iter().filter(|(_, file)| file.extension().is_some_and(|kind| kind == "json"));
        let mut all: Vec<Record> =
            records.filter_map(|(_, file)| Record::read(&std::fs::read_to_string(file).ok()?)).collect();
        all.sort_by_key(|record| record.id);
        all
    }

    /// Forget a program: its record, and what it printed.
    pub fn forget(&self, id: u64) {
        for file in [self.file(id), self.log(id)] {
            let _ = std::fs::remove_file(file);
        }
    }

    /// Have the records in another folder from now on, which is made if it is not there. Those
    /// there are go with it, and what was printed: a program that runs prints on into its file
    /// wherever that is moved, as long as it stays on the same disk.
    pub fn move_to(&mut self, folder: &Path) -> std::io::Result<()> {
        make(folder)?;
        if folder.canonicalize()? != self.folder.canonicalize().unwrap_or_default() {
            for (_, file) in self.numbered() {
                let kept = file.extension().is_some_and(|kind| kind == "json" || kind == "log");
                if let (true, Some(name)) = (kept, file.file_name()) {
                    std::fs::rename(&file, folder.join(name))?;
                }
            }
        }
        self.folder = folder.to_owned();
        Ok(())
    }
}

/// Where triq-serve keeps what is its own: the folder the list of registered folders is in,
/// `~/.config/triq` unless that was set otherwise. None for a person with no home.
fn own() -> Option<PathBuf> {
    Some(triq::registry()?.parent()?.to_owned())
}

/// The file that remembers which folder was chosen for the records.
pub fn choice() -> PathBuf {
    own().unwrap_or_else(std::env::temp_dir).join("records")
}

/// The folder the records are in when none was chosen.
pub fn usual() -> PathBuf {
    own().unwrap_or_else(std::env::temp_dir).join("started")
}

/// The folder that was chosen for the records, as that file remembers it, if one was.
pub fn chosen(file: &Path) -> Option<PathBuf> {
    let kept = std::fs::read_to_string(file).ok()?;
    kept.lines().map(str::trim).find(|line| !line.is_empty()).map(PathBuf::from)
}

/// Remember the folder that was chosen for the records.
pub fn choose(file: &Path, folder: &Path) -> std::io::Result<()> {
    if let Some(within) = file.parent() {
        make(within)?;
    }
    std::fs::write(file, format!("{}\n", folder.display()))
}
