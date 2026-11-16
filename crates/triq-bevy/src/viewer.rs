//! A node that shows camera frames, for any program drawn with egui.
//!
//! A frame is large, and a window has one thread to draw with. So a frame
//! view is an ordinary node: the runtime runs it on the thread or the pool it
//! was placed on, where it scales the newest frame down to what a window can
//! use. The window only draws what its views left for it.
//!
//! triq does not know it is a viewer, and neither does the workbench.
use crate::Frame;
use egui::{Color32, ColorImage, Image, TextureHandle, TextureOptions, Ui, vec2};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use triq::prelude::*;

/// The kind of node this offers.
pub const FRAME_VIEW: &str = "frame_view";

/// How often a view looks for a new frame, and how often a window with a view in it is redrawn.
const REFRESH: Duration = Duration::from_millis(16);

/// How tall a picture is drawn, in points.
const HEIGHT: f32 = 180.0;

/// How many rows of pixels a picture is scaled down to: enough for `HEIGHT`
/// on a screen with two pixels to the point.
const ROWS: usize = 360;

#[derive(Ports)]
pub struct FrameViewPorts {
    pub image: Input<[u8], Frame>,
}

/// What every view of one program has for its window, by the view's name.
type Shown = Arc<Mutex<BTreeMap<String, Picture>>>;

/// What one view has to show.
#[derive(Default)]
struct Picture {
    /// The newest good frame.
    frame: Option<Frame>,
    /// Its pixels, scaled down, until the window has taken them.
    pixels: Option<ColorImage>,
    /// Why the newest frame could not be drawn, if it could not.
    problem: Option<String>,
}

/// Every frame view of one program. Make one, and call [`FrameViews::show`]
/// where the pictures should be drawn.
pub struct FrameViews {
    shown: Shown,
    /// The picture of each view, as the window draws it.
    textures: BTreeMap<String, TextureHandle>,
}

impl FrameViews {
    /// Offer `frame_view` nodes on `runtime`, which runs each where it is placed.
    pub fn new(runtime: &Runtime) -> triq::Result<Self> {
        let shown = Shown::default();
        runtime.register_with(FRAME_VIEW, {
            let shown = shown.clone();
            move |name, ()| Ok(FrameView::new(name, shown.clone()))
        })?;
        Ok(Self { shown, textures: BTreeMap::new() })
    }

    pub fn is_empty(&self) -> bool {
        self.shown.lock().unwrap().is_empty()
    }

    /// Draw every view, one after the other in the direction `ui` lays things out.
    /// Call it once per frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let mut shown = self.shown.lock().unwrap();
        self.textures.retain(|name, _| shown.contains_key(name));
        for (name, picture) in shown.iter_mut() {
            // Pixels a view left since the last frame go into its texture.
            if let Some(pixels) = picture.pixels.take() {
                match self.textures.get_mut(name) {
                    Some(texture) => texture.set(pixels, TextureOptions::LINEAR),
                    None => {
                        let texture = ui.ctx().load_texture(name, pixels, TextureOptions::LINEAR);
                        self.textures.insert(name.clone(), texture);
                    }
                }
            }
            ui.vertical(|ui| draw(ui, name, picture, self.textures.get(name)));
        }
        if !shown.is_empty() {
            ui.ctx().request_repaint_after(REFRESH);
        }
    }
}

fn draw(ui: &mut Ui, name: &str, picture: &Picture, texture: Option<&TextureHandle>) {
    ui.strong(name);
    if let Some(problem) = &picture.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
    }
    let (Some(frame), Some(texture)) = (&picture.frame, texture) else {
        ui.weak("no frame yet");
        return;
    };
    ui.weak(format!("{} x {}, frame {}", frame.width, frame.height, frame.number));
    let size = vec2(HEIGHT * frame.width as f32 / frame.height as f32, HEIGHT);
    ui.add(Image::new(texture).fit_to_exact_size(size));
}

/// The node behind one view. It runs on the runtime's threads.
struct FrameView {
    name: String,
    shown: Shown,
}

impl FrameView {
    fn new(name: &str, shown: Shown) -> Self {
        // The view is there for the window from the start, before any frame.
        shown.lock().unwrap().insert(name.to_owned(), Picture::default());
        Self { name: name.to_owned(), shown }
    }
}

impl Node for FrameView {
    type Ports = FrameViewPorts;
    type Params = ();

    /// A view that no window draws. Those of a window are made by its [`FrameViews`].
    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self::new("", Shown::default()))
    }

    /// Frames may come faster than a window is drawn, and each is large. So
    /// a view looks for the newest one as often as a window could show it.
    fn trigger(&self) -> Trigger {
        Trigger::Every(REFRESH)
    }

    fn run(&mut self, ports: &mut FrameViewPorts) -> triq::Result {
        let Some(sample) = ports.image.latest()? else { return Ok(()) };
        let frame = *sample.header();
        let (width, height) = (frame.width as usize, frame.height as usize);
        // The header is another program's word for what the bytes are. Check it before trusting it.
        let fits = !sample.is_empty() && width as u128 * height as u128 * 4 == sample.len() as u128;
        // The pixels are read where they lie in shared memory, and only as many are kept as a window needs.
        let pixels = fits.then(|| thumbnail(&sample, width, height, ROWS));
        let mut shown = self.shown.lock().unwrap();
        let picture = shown.entry(self.name.clone()).or_default();
        match pixels {
            Some(pixels) => *picture = Picture { frame: Some(frame), pixels: Some(pixels), problem: None },
            // The last good frame stays on screen, with the complaint above it.
            None => {
                let size = format!("{} bytes is not {width} x {height} pixels", sample.len());
                picture.problem = Some(format!("frame {}: {size}", frame.number));
            }
        }
        Ok(())
    }
}

impl Drop for FrameView {
    fn drop(&mut self) {
        self.shown.lock().unwrap().remove(&self.name);
    }
}

/// A picture of `width` by `height` pixels, four bytes each, scaled down to
/// at most `rows` rows and four times as many columns: every pixel of the
/// result is the average of a square of the original. A picture that is
/// small enough is taken as it is.
pub fn thumbnail(pixels: &[u8], width: usize, height: usize, rows: usize) -> ColorImage {
    let step = height.div_ceil(rows).max(width.div_ceil(4 * rows));
    if step <= 1 {
        return ColorImage::from_rgba_unmultiplied([width, height], pixels);
    }
    // A picture too thin to hold one square in a direction keeps one pixel there.
    let (wide, tall) = ((width / step).max(1), (height / step).max(1));
    let (across, down) = (step.min(width), step.min(height));
    let mut small = Vec::with_capacity(wide * tall);
    for row in 0..tall {
        for column in 0..wide {
            let mut sum = [0u32; 4];
            for line in 0..down {
                let start = ((row * step + line) * width + column * step) * 4;
                for pixel in pixels[start..start + across * 4].chunks_exact(4) {
                    sum.iter_mut().zip(pixel).for_each(|(total, part)| *total += u32::from(*part));
                }
            }
            let [red, green, blue, alpha] = sum.map(|total| (total / (across * down) as u32) as u8);
            small.push(Color32::from_rgba_unmultiplied(red, green, blue, alpha));
        }
    }
    ColorImage::new([wide, tall], small)
}
