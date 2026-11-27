//! One line of text as it is typed: the letters, where the next one goes, and what a key does.
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Field {
    letters: Vec<char>,
    /// How many letters are before the place where the next one goes.
    at: usize,
}

impl Field {
    /// A line that says `text`, with the next letter to go at its end.
    pub fn new(text: &str) -> Self {
        let letters: Vec<char> = text.chars().collect();
        Self { at: letters.len(), letters }
    }

    pub fn text(&self) -> String {
        self.letters.iter().collect()
    }

    /// The text before the place where the next letter goes, and the text from there on.
    pub fn halves(&self) -> (String, String) {
        (self.letters[..self.at].iter().collect(), self.letters[self.at..].iter().collect())
    }

    /// What a key does to the line. False for a key that is not the line's: Enter, Escape, Tab.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let held = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Ctrl+U empties the line, as a shell does.
            KeyCode::Char('u') if held => (self.letters, self.at) = (Vec::new(), 0),
            KeyCode::Char(letter) if !held => {
                self.letters.insert(self.at, letter);
                self.at += 1;
            }
            KeyCode::Backspace if self.at > 0 => {
                self.at -= 1;
                self.letters.remove(self.at);
            }
            KeyCode::Delete if self.at < self.letters.len() => _ = self.letters.remove(self.at),
            KeyCode::Left => self.at = self.at.saturating_sub(1),
            KeyCode::Right => self.at = (self.at + 1).min(self.letters.len()),
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.at = self.letters.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}
