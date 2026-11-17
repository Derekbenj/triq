//! A views program, complete. Copy it to start one of your own.
//!
//! ```text
//! cargo run -p triq-views --example template
//! ```
//!
//! It is one program with two things in it. A thermometer, which stands for
//! whatever a project has to show: an ordinary node that sends a reading ten
//! times a second. And a kind of view called `gauge`, which draws the newest
//! reading it received. So that there is something to see at once, the
//! program creates one gauge itself and wires the thermometer to it.
//!
//! Start the workbench beside it, and it shows this program's runtime,
//! `template`, with both nodes and the wire. `New gauge` there creates
//! another view, which gets a pane of its own here; drag the thermometer's
//! `reading` to its `input`.
//!
//! ```text
//! cargo run -p triq-workbench
//! ```
use serde_json::json;
use std::time::Duration;
use triq::prelude::*;
use triq_views::{Latest, Views, egui};

fn main() -> triq_views::eframe::Result {
    // The runtime the views are nodes of. A program whose views do work gives it a pool for them.
    triq_views::run(Runtime::builder("template"), kinds)
}

/// The kinds of view this program has, and what it starts with.
pub fn kinds(views: &mut Views) -> triq::Result {
    // A kind of view for a small message: what it is called, and how the newest message is drawn.
    // A project adds one such line for each of its kinds.
    views.add(Latest::new(views.runtime(), "gauge", degrees)?);

    // Something to look at: a thermometer, and one gauge that is wired to it. In a project the
    // nodes are other programs' as a rule, and the views are created and wired from the workbench.
    let runtime = views.runtime();
    runtime.spawn::<Thermometer>("thermometer", Placement::Dedicated, ())?;
    runtime.spawn_kind("gauge", "gauge1", Placement::Dedicated, json!(null))?;
    runtime.connect("thermometer/reading", "gauge1/input")
}

/// A message: what the thermometer says. Plain data, read where it lies in shared memory.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Reading")]
pub struct Reading {
    pub celsius: f32,
}

/// How a gauge draws a reading, in the room its pane has. This is all of the view.
fn degrees(ui: &mut egui::Ui, reading: &Reading) {
    ui.heading(format!("{:.1} °C", reading.celsius));
    // From fifteen degrees, empty, to twenty-five, full.
    ui.add(egui::ProgressBar::new((reading.celsius - 15.0) / 10.0).desired_width(200.0));
}

#[derive(Ports)]
struct ThermometerPorts {
    reading: Output<Reading>,
}

/// Stands for whatever a project has to show. It sends a reading ten times a
/// second, of a temperature that goes up and down around twenty degrees.
struct Thermometer {
    seconds: f32,
}

impl Node for Thermometer {
    type Ports = ThermometerPorts;
    type Params = ();

    fn new(_: ()) -> triq::Result<Self> {
        Ok(Self { seconds: 0.0 })
    }

    fn trigger(&self) -> Trigger {
        Trigger::Every(Duration::from_millis(100))
    }

    fn run(&mut self, ports: &mut ThermometerPorts) -> triq::Result {
        self.seconds += 0.1;
        ports.reading.send(Reading { celsius: 20.0 + 3.0 * (self.seconds / 2.0).sin() })?;
        Ok(())
    }
}
