//! What a line on a chart is drawn with: its colour, how it fades into the past, and whether it
//! is drawn at all.
use crate::colors;
use eframe::egui::color_picker::{Alpha, color_edit_button_srgba};
use eframe::egui::containers::menu::{MenuButton, MenuConfig};
use eframe::egui::{self, Color32, Painter, PopupCloseBehavior, Pos2, Role, Sense, Shape, Stroke, Ui, WidgetInfo};
use serde::{Deserialize, Serialize};

/// How wide a line is drawn.
const WIDTH: f32 = 2.0;
/// By when a line is gone when it is first told to fade: by the oldest moment its chart
/// shows, which is all of the time that the chart shows.
const USUAL: f32 = 1.0;
/// In how many steps a line goes from unseen to its full colour.
const STEPS: f32 = 32.0;
/// The soonest that a line is gone: at this age, where 1 is as long as its chart shows.
const SOONEST: f32 = 0.001;
/// The most seconds a line that fades can be told to be seen for: as many as are kept.
const LONGEST: f32 = crate::history::History::LONGEST as f32;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Pen {
    /// Which of the colours that lines take in turn it was given.
    pub(super) shade: usize,
    /// The colour that was chosen for it instead.
    pub(super) chosen: Option<Color32>,
    /// How much of the time its chart shows the line can be seen for, from 0 to 1, if it
    /// fades into the past: by then it is gone. Where it is newest it has its full colour.
    #[serde(default)]
    pub(super) fade: Option<f32>,
    /// How many seconds a line that fades can be seen for, if that was said in seconds: then
    /// it does not go by how much of the past the chart shows.
    #[serde(default)]
    pub(super) seconds: Option<f32>,
    /// Whether the line is left out of the drawing, with its name still on the chart.
    #[serde(default)]
    pub(super) hidden: bool,
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

    /// The button beside a name that hides the line or draws it again: an eye, struck through
    /// while the line is hidden. It is drawn here, as the fonts have no such sign.
    pub(super) fn eye(&mut self, ui: &mut Ui) {
        let side = ui.spacing().interact_size.y;
        let (rect, button) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
        button.widget_info(|| WidgetInfo::selected(Role::Button, true, !self.hidden, "Hide or show"));
        if button.clicked() {
            self.hidden = !self.hidden;
        }
        let (middle, ink) = (rect.center(), ui.style().interact(&button).fg_stroke.color);
        ui.painter().add(Shape::ellipse_stroke(middle, egui::vec2(6.0, 3.5), Stroke::new(1.0, ink)));
        ui.painter().circle_filled(middle, 1.8, ink);
        if self.hidden {
            let across = egui::vec2(6.0, -5.0);
            ui.painter().line_segment([middle - across, middle + across], Stroke::new(1.5, ink));
        }
        button.on_hover_text(if self.hidden {
            "Draw it again"
        } else {
            "Hide it: it keeps its name here, and is not drawn"
        });
    }

    /// Whether the line fades into the past, and by when it is gone: a box to tick, and under
    /// it two numbers that say the same. The first is how much of the time the chart shows,
    /// `shown` seconds, in hundredths, and the second is that in seconds. Either can be typed
    /// into, and the one that was is the one that holds when the chart shows more or less.
    pub(super) fn fading(&mut self, ui: &mut Ui, shown: f64) {
        let mut fades = self.fade.is_some();
        if ui.checkbox(&mut fades, "Fade into the past").changed() {
            (self.fade, self.seconds) = (fades.then_some(USUAL), None);
        }
        let (Some(share), shown) = (self.fade, shown as f32) else { return };
        let (mut percent, mut seconds) = match self.seconds {
            Some(seconds) => (seconds / shown * 100.0, seconds),
            None => (share * 100.0, share * shown),
        };
        // The one that holds is written plainly, and the other faintly.
        let said = |ui: &mut Ui, holds: bool, words: &str| if holds { ui.label(words) } else { ui.weak(words) };
        ui.horizontal(|ui| {
            let label = said(ui, self.seconds.is_none(), "Gone after");
            let number = egui::DragValue::new(&mut percent).range(1.0..=100.0).max_decimals(1).suffix(" %");
            if ui.add(number).labelled_by(label.id).changed() {
                (self.fade, self.seconds) = (Some(percent / 100.0), None);
            }
            said(ui, self.seconds.is_none(), "of the time shown");
        });
        ui.horizontal(|ui| {
            let label = said(ui, self.seconds.is_some(), "or after");
            let number = egui::DragValue::new(&mut seconds).range(0.01..=LONGEST).max_decimals(2).suffix(" s");
            if ui.add(number).labelled_by(label.id).changed() {
                self.seconds = Some(seconds);
            }
        });
    }

    /// Draw a line through some places, on a chart that shows so many seconds. Each place
    /// comes with how old it is: 0 where the line is newest, and 1 as long before that as the
    /// chart shows.
    pub(super) fn draw(&self, painter: &Painter, line: &[(Pos2, f32)], shown: f64, dark: bool) {
        // The age at which it is gone: so many seconds, or so much of what the chart shows.
        let gone = self.fade.map(|share| self.seconds.map_or(share, |seconds| seconds / shown as f32));
        let color = self.color(dark);
        for (piece, strength) in pieces(line, gone) {
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
/// unseen, to 1. `line` is the places it passes through, each with how old it is: 0 where it
/// is newest, 1 as long before that as its chart shows.
///
/// `gone` is the age at which a line that fades has faded to nothing: its strength falls
/// evenly from its newest point to there. At 1 that is the oldest moment the chart shows, at
/// a half the middle of it. A line that does not fade is one piece.
///
/// A piece begins where the one before it ends, and a long stretch between two places is cut
/// where the strength changes: the line fades evenly however few points it has.
pub fn pieces(line: &[(Pos2, f32)], gone: Option<f32>) -> Vec<(Vec<Pos2>, f32)> {
    // The strength at an age, counted in steps, as if it could fall below nothing.
    let fall = gone.map_or(0.0, |gone| 1.0 / gone.max(SOONEST));
    let level = |age: f32| (1.0 - fall * age) * STEPS;
    let mut pieces: Vec<(Vec<Pos2>, f32)> = Vec::new();
    for pair in line.windows(2) {
        let ((from, to), (first, last)) = ((pair[0].0, pair[1].0), (level(pair[0].1), level(pair[1].1)));
        // The strength changes where the level is halfway between two steps: so far along the stretch.
        let halfway = |step: usize| (step as f32 + 0.5 - first) / (last - first);
        let mut ends: Vec<f32> =
            (0..STEPS as usize).map(halfway).filter(|along| *along > 0.0 && *along < 1.0).collect();
        ends.sort_by(f32::total_cmp);
        ends.push(1.0);
        let mut begin = 0.0;
        for end in ends {
            let strength = egui::lerp(first..=last, (begin + end) / 2.0).clamp(0.0, STEPS).round() / STEPS;
            let place = if end < 1.0 { from.lerp(to, end) } else { to };
            match pieces.last_mut() {
                Some((piece, drawn)) if *drawn == strength => piece.push(place),
                Some((piece, _)) => {
                    let joint = piece[piece.len() - 1];
                    pieces.push((vec![joint, place], strength));
                }
                None => pieces.push((vec![from, place], strength)),
            }
            begin = end;
        }
    }
    pieces
}

/// The colour the first, second, third line on a chart is given.
pub fn color(line: usize, dark: bool) -> Color32 {
    colors::shade(line, dark)
}
