//! What is kept on disk of the programs that were started: a small file for each, and beside
//! it what the program printed; and the file that remembers which folder was chosen for them.
use std::path::{Path, PathBuf};
use triq_serve::records::{Record, Records, choose, chosen, make};

/// A folder of the test's own, which is not there yet.
fn folder(tag: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("triq-serve-records-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

fn record(id: u64) -> Record {
    Record {
        id,
        program: "stack-quadcopter".to_owned(),
        path: PathBuf::from("/robot/target/release/triq-stack-quadcopter"),
        told: vec!["quadcopter2".to_owned(), "--speed".to_owned(), "5".to_owned()],
        pid: 4321,
        began: 1_795_000_000,
    }
}

#[test]
fn a_record_is_kept_read_again_and_forgotten() {
    let records = Records::in_folder(folder("kept"));
    // A folder that is not there holds nothing, and the first number is free.
    assert!(records.all().is_empty());
    assert_eq!(records.free(), 1);

    records.keep(&record(7)).expect("keep it");
    records.keep(&record(2)).expect("keep it");
    std::fs::write(records.log(7), "hello\n").expect("what it printed");
    // They are read again as they were kept, in the order of their numbers.
    assert_eq!(records.all(), [record(2), record(7)]);
    // The next number is past every one that is taken, also by what was printed alone.
    std::fs::write(records.log(11), "left behind\n").expect("a file");
    assert_eq!(records.free(), 12);
    // What is in the folder and is no record is passed over.
    std::fs::write(records.folder().join("notes.txt"), "mine").expect("a file");
    std::fs::write(records.folder().join("9.json"), "this is not a record").expect("a file");
    assert_eq!(records.all(), [record(2), record(7)]);

    // Forgotten, a record is gone with what was printed.
    records.forget(7);
    assert!(!records.log(7).exists());
    assert_eq!(records.all(), [record(2)]);
    records.forget(7);
}

#[test]
fn the_records_are_moved_to_another_folder_with_what_was_printed() {
    let mut records = Records::in_folder(folder("from"));
    for id in [1, 2] {
        records.keep(&record(id)).expect("keep it");
        std::fs::write(records.log(id), format!("printed by {id}\n")).expect("what it printed");
    }
    let (from, to) = (records.folder().to_owned(), folder("to").join("deeper"));
    records.move_to(&to).expect("move them");
    assert_eq!(records.folder(), to);
    assert_eq!(records.all(), [record(1), record(2)]);
    assert_eq!(std::fs::read_to_string(records.log(2)).unwrap(), "printed by 2\n");
    assert!(Records::in_folder(from).all().is_empty(), "nothing is left where they were");
    // Moved to where they are, they stay.
    records.move_to(&to).expect("nothing to do");
    assert_eq!(records.all().len(), 2);
}

#[test]
fn the_folder_that_was_chosen_is_remembered_in_a_file() {
    let file = folder("choice").join("records");
    assert_eq!(chosen(&file), None);
    choose(&file, Path::new("/somewhere/of my own")).expect("remember it");
    assert_eq!(chosen(&file), Some(PathBuf::from("/somewhere/of my own")));
    choose(&file, Path::new("/elsewhere")).expect("remember it");
    assert_eq!(chosen(&file), Some(PathBuf::from("/elsewhere")));
}

/// Another thread of the program may have narrowed, for a moment, what a new folder is let
/// have: iceoryx2 does as it binds a socket. A folder made here can be gone into regardless.
#[cfg(unix)]
#[test]
fn a_folder_that_is_made_can_be_used_whatever_the_mask_of_the_process_was() {
    use std::os::unix::fs::PermissionsExt;
    let folder = folder("made").join("one").join("two");
    make(&folder).expect("make it");
    for part in
        [folder.clone(), folder.parent().unwrap().to_owned(), folder.parent().unwrap().parent().unwrap().to_owned()]
    {
        let mode = std::fs::metadata(&part).expect("it is there").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "{}", part.display());
    }
    // One that is there already is left as it is.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).expect("its owner's alone");
    make(&folder).expect("nothing to do");
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
    std::fs::write(folder.join("a file"), "in it").expect("it can be written in");
}
