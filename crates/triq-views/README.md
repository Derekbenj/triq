# triq-views

A window for a project's own views of its messages, for programs built on
[triq](https://crates.io/crates/triq).

A **view** draws what only your project knows how to draw: a controller's
sticks, a camera's picture. It is two ordinary things: a kind of node with an
input, and something that draws what a node of that kind last received.
`triq-views` is the program around your kinds of view: a runtime, and a window
with a list of the current views and a pane for each, which the user arranges.

```toml
[dependencies]
triq = "0.1"
triq-views = "0.1"
```

```rust
use triq::prelude::*;
use triq_views::{Latest, egui};

/// A message: plain data, read where it lies in shared memory.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Reading")]
struct Reading {
    celsius: f32,
}

fn main() -> triq_views::eframe::Result {
    triq_views::run(Runtime::builder("views"), |views| {
        // A kind of view called `gauge`: the message it listens to, and how the newest one is drawn.
        views.add(Latest::new(views.runtime(), "gauge", degrees)?);
        Ok(())
    })
}

fn degrees(ui: &mut egui::Ui, reading: &Reading) {
    ui.label(format!("{:.1} degrees", reading.celsius));
}
```

The views are nodes of the program's runtime. So the triq workbench, the
`triq` tool, or any other program can list the kinds, create views and wire
them, as with any other node.

- `run` is the whole program: it starts the runtime it is given and opens the
  window, which remembers how the views were arranged.
- `Latest` is a kind of view made in one call, for a message small enough to
  be read as the window draws.
- `Kind` is the trait that a kind of your own implements: which views there
  are, and how one is drawn.
- `Views` is for a project that already has a window: make one with
  `Views::new` and draw it with `Views::show`.

A crate that only defines a kind of view should depend on `triq-views` without
its default features; it then builds no window.

[`examples/template.rs`](examples/template.rs) is a complete program to copy.
["Views of your own"](https://github.com/Derekbenj/triq#views-of-your-own) in
the triq README says more. On macOS, add the `[patch.crates-io]` section that
["Platforms"](https://github.com/Derekbenj/triq#platforms) describes there.

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
