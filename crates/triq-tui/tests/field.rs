//! A line of text as it is typed.
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use triq_tui::field::Field;

fn press(field: &mut Field, keys: &[KeyCode]) {
    for key in keys {
        assert!(field.key(KeyEvent::new(*key, KeyModifiers::NONE)), "{key:?} is a key of the line");
    }
}

fn typed(field: &mut Field, text: &str) {
    let keys: Vec<KeyCode> = text.chars().map(KeyCode::Char).collect();
    press(field, &keys);
}

#[test]
fn letters_go_where_the_line_is_at_and_the_keys_move_and_take_away() {
    let mut field = Field::new("alph");
    typed(&mut field, "a");
    assert_eq!(field.halves(), ("alpha".to_owned(), String::new()));
    // Back two, a letter in the middle, and the one after it taken away.
    press(&mut field, &[KeyCode::Left, KeyCode::Left]);
    typed(&mut field, "X");
    assert_eq!(field.halves(), ("alpX".to_owned(), "ha".to_owned()));
    press(&mut field, &[KeyCode::Delete, KeyCode::Backspace]);
    assert_eq!(field.text(), "alpa");
    // Home and End, and nothing is taken away where there is nothing.
    press(&mut field, &[KeyCode::Home, KeyCode::Backspace, KeyCode::Left]);
    typed(&mut field, "é ");
    press(&mut field, &[KeyCode::End, KeyCode::Delete, KeyCode::Right]);
    typed(&mut field, "!");
    assert_eq!(field.text(), "é alpa!");
}

#[test]
fn ctrl_u_empties_the_line_and_keys_that_are_not_the_lines_are_left_to_others() {
    let mut field = Field::new("some words");
    assert!(field.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)));
    assert_eq!(field.text(), "");
    for key in [KeyCode::Enter, KeyCode::Esc, KeyCode::Tab, KeyCode::Up] {
        assert!(!field.key(KeyEvent::new(key, KeyModifiers::NONE)), "{key:?}");
    }
    // A letter with Ctrl held is a command to someone else, not a letter.
    assert!(!field.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert_eq!(field.text(), "");
}
