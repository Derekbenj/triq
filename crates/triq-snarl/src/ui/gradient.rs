//! triq: a fill that fades through several colors from left to right, for
//! the header of a node that belongs to more than one thing.

use egui::{Color32, CornerRadius, Mesh, Rect, Shape, pos2};

/// The color at `along`, from 0 at the left to 1 at the right. Every color
/// has a band of the same width with the color itself at its middle, and
/// fades into its neighbors from there. So each takes up as much room as
/// any other.
fn color_at(colors: &[Color32], along: f32) -> Color32 {
    #[allow(clippy::cast_precision_loss)]
    let band = (along * colors.len() as f32 - 0.5).clamp(0.0, (colors.len() - 1) as f32);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let left = (band.floor() as usize).min(colors.len() - 1);
    let right = (left + 1).min(colors.len() - 1);
    colors[left].lerp_to_gamma(colors[right], band.fract())
}

/// How far a rounded corner cuts into the edge it ends, at a distance
/// `from_side` from the side the corner is on.
fn cut(radius: u8, from_side: f32) -> f32 {
    let radius = f32::from(radius);
    if from_side >= radius {
        return 0.0;
    }
    let inward = radius - from_side;
    radius - (radius * radius - inward * inward).sqrt()
}

/// A rectangle with rounded corners, filled with `colors` fading into one
/// another from left to right. It is made of upright strips: narrow ones
/// where a corner is, and one from the middle of each color's band to the next.
pub fn faded_rect(rect: Rect, corners: CornerRadius, colors: &[Color32]) -> Shape {
    const STEPS: u8 = 8;
    let left = f32::from(corners.nw.max(corners.sw));
    let right = f32::from(corners.ne.max(corners.se));

    let mut columns: Vec<f32> = Vec::new();
    for step in 0..=STEPS {
        let part = f32::from(step) / f32::from(STEPS);
        columns.push(rect.left() + left * part);
        columns.push(rect.right() - right * part);
    }
    for band in 0..colors.len() {
        #[allow(clippy::cast_precision_loss)]
        columns.push(rect.left() + rect.width() * (band as f32 + 0.5) / colors.len() as f32);
    }
    columns.sort_by(f32::total_cmp);

    let mut mesh = Mesh::default();
    for (column, x) in columns.into_iter().enumerate() {
        let (from_left, from_right) = (x - rect.left(), rect.right() - x);
        let top = rect.top() + cut(corners.nw, from_left).max(cut(corners.ne, from_right));
        let bottom = rect.bottom() - cut(corners.sw, from_left).max(cut(corners.se, from_right));
        let color = color_at(colors, from_left / rect.width());
        mesh.colored_vertex(pos2(x, top), color);
        mesh.colored_vertex(pos2(x, bottom), color);
        // The strip between this column and the one before it.
        if let Some(before) = column.checked_sub(1) {
            #[allow(clippy::cast_possible_truncation)]
            let (a, b) = (before as u32 * 2, column as u32 * 2);
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b, b + 1);
        }
    }
    Shape::mesh(mesh)
}
