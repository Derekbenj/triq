//! What a line on a chart is drawn with: its colour, and how it fades into the past.
use crate::colors;
use eframe::egui::color_picker::{Alpha, color_edit_button_srgba};
use eframe::egui::containers::menu::{MenuButton, MenuConfig};
use eframe::egui::{self, Color32, Painter, PopupCloseBehavior, Pos2, Stroke, Ui};
use serde::{Deserialize, Serialize};

/// How wide a line is drawn.
const WIDTH: f32 = 2.0;
/// How see-through the oldest point of a line is when the line is first told to fade.
const USUAL: f32 = 0.8;
/// In how many steps a line goes from unseen to its full colour.
const STEPS: f32 = 32.0;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Pen {
    /// Which of the colours that lines take in turn it was given.
    pub(super) shade: usize,
    /// The colour that was chosen for it instead.
    pub(super) chosen: Option<Color32>,
    /// How see-through the line is at the oldest moment its chart shows, from 0 to 1, if it
    /// fades into the past. At the present it has its full colour.
    #[serde(default)]
    pub(super) fade: Option<f32>,
}

impl Pen {
    pub(super) fn color(&self, dark: bool) -> Color32 {
        self.chosen.unwrap_or_else(|| color(self.shade, dark))
    }

    /// The button beside a name that shows the colour and opens a picker.
    pub(super) fn well(&mut self, ui: &mut Ui) {
        let mut color = self.color(ui.visuals().dark_mode);
        let well = ui.scope(|ui| {
            ui.spacing_mut().interact_size = egui::vec2(16.0, 14.0);
            color_edit_button_srgba(ui, &mut color, Alpha::Opaque)
        });
        if well.inner.on_hover_text("Choose the colour of this line").changed() {
            self.chosen = Some(color);
        }
    }

    /// Whether the line fades into the past, and how far: a box to tick, and a slider under it.
    pub(super) fn fading(&mut self, ui: &mut Ui) {
        let mut fades = self.fade.is_some();
        if ui.checkbox(&mut fades, "Fade into the past").changed() {
            self.fade = fades.then_some(USUAL);
        }
        if let Some(fade) = &mut self.fade {
            let percent = |fade: f64, _| format!("{:.0} %", fade * 100.0);
            let slider = egui::Slider::new(fade, 0.0..=1.0).custom_formatter(percent);
            ui.add(slider.text("see-through at its oldest"));
        }
    }

    /// Draw a line through some places. Each comes with how old it is: 0 at the present, and 1
    /// at the oldest moment the chart shows.
    pub(super) fn draw(&self, painter: &Painter, line: &[(Pos2, f32)], dark: bool) {
        let color = self.color(dark);
        for (piece, strength) in pieces(line, self.fade.unwrap_or(0.0)) {
            if strength > 0.0 {
                painter.line(piece, Stroke::new(WIDTH, color.gamma_multiply(strength)));
            }
        }
    }
}

/// The button beside a name that opens what else can be said of how a line is drawn. What it
/// opens stays open while it is used, and closes at a click elsewhere.
pub(super) fn more(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    let stays = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let button = MenuButton::from_button(egui::Button::new("…").small()).config(stays);
    button.ui(ui, contents).0.on_hover_text("How this line is drawn");
}

/// A line cut into the pieces it is drawn in, each in one strength of its colour: from 0,
/// unseen, to 1. `line` is the places it passes through, each with how old it is, and `fade` is
/// how see-through it is at the age of 1. A line that does not fade is one piece.
///
/// A piece begins where the one before it ends, and a long stretch between two places is cut
/// where the strength changes: the line fades evenly however few points it has.
pub fn pieces(line: &[(Pos2, f32)], fade: f32) -> Vec<(Vec<Pos2>, f32)> {
    let level = |age: f32| (1.0 - fade * age.clamp(0.0, 1.0)) * STEPS;
    let mut pieces: Vec<(Vec<Pos2>, f32)> = Vec::new();
    for pair in line.windows(2) {
        let ((from, to), (first, last)) = ((pair[0].0, pair[1].0), (level(pair[0].1), level(pair[1].1)));
        let parts = (last - first).abs().ceil().max(1.0);
        // How far along the stretch each part ends, and where that is.
        for end in (1..=parts as usize).map(|part| part as f32 / parts) {
            let strength = egui::lerp(first..=last, end - 0.5 / parts).round() / STEPS;
            let end = if end < 1.0 { from.lerp(to, end) } else { to };
            match pieces.last_mut() {
                Some((piece, drawn)) if *drawn == strength => piece.push(end),
                Some((piece, _)) => {
                    let begin = piece[piece.len() - 1];
                    pieces.push((vec![begin, end], strength));
                }
                None => pieces.push((vec![from, end], strength)),
            }
        }
    }
    pieces
}

/// The colour the first, second, third line on a chart is given.
pub fn color(line: usize, dark: bool) -> Color32 {
    colors::shade(line, dark)
}
