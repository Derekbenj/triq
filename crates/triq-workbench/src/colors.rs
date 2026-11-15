//! One colour per runtime, so that a glance at the graph tells which program a node runs in.
use eframe::egui::Color32;
use std::collections::BTreeMap;

/// Hues that can be told apart from each other.
const PALETTE: [(u8, u8, u8); 16] = [
    (87, 163, 233),
    (235, 134, 84),
    (91, 198, 144),
    (191, 126, 233),
    (236, 105, 137),
    (107, 201, 211),
    (219, 183, 78),
    (132, 165, 241),
    (196, 212, 107),
    (239, 126, 197),
    (114, 212, 173),
    (244, 159, 102),
    (170, 154, 235),
    (123, 190, 243),
    (231, 119, 105),
    (154, 204, 133),
];

/// Which colour each running runtime has.
#[derive(Default)]
pub struct Colors {
    /// A place in the palette, by runtime name.
    given: BTreeMap<String, usize>,
}

impl Colors {
    /// Tell it which runtimes are running. One that has gone gives its colour
    /// back, a new one takes the first that is free, and the others keep theirs.
    pub fn update<'a>(&mut self, running: impl IntoIterator<Item = &'a str>) {
        let running: Vec<&str> = running.into_iter().collect();
        self.given.retain(|runtime, _| running.contains(&runtime.as_str()));
        for runtime in running {
            if !self.given.contains_key(runtime) {
                let free = (0..PALETTE.len()).find(|place| !self.given.values().any(|given| given == place));
                // With more runtimes than colours, colours are shared.
                self.given.insert(runtime.to_owned(), free.unwrap_or(self.given.len() % PALETTE.len()));
            }
        }
    }

    /// The colour of a runtime. On a light background it is darkened to stand out.
    pub fn of(&self, runtime: &str, dark: bool) -> Color32 {
        let Some(place) = self.given.get(runtime) else { return Color32::GRAY };
        let (r, g, b) = PALETTE[*place];
        let shade = |channel: u8| if dark { channel } else { (f32::from(channel) * 0.73) as u8 };
        Color32::from_rgb(shade(r), shade(g), shade(b))
    }
}
