# triq

Nodes, factories and runtimes on top of [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2).

You write a piece of code and say what goes in and out of it. triq runs it on
the thread you choose, makes it visible to every program on the machine, and
lets any of them wire its outputs to inputs elsewhere, while it runs.

triq adds no transport of its own:

- An **output** is one iceoryx2 publish-subscribe service.
- An **input** is a set of iceoryx2 subscribers, one per connected output.
- The thread that runs a **node** sleeps on those subscribers directly, so a
  message costs one thread wake-up and passes through nothing else.
- Messages are plain `#[repr(C)]` data in shared memory: written once, read in place.
- A small **control** thread per runtime answers requests from other programs.
  No message between nodes ever passes through it.

## A node

```rust
use triq::prelude::*;

#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[repr(C)]
#[type_name("Pose")]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Ports)]
struct FilterPorts {
    raw: Input<Pose>,
    smooth: Output<Pose>,
}

struct Filter {
    alpha: f32,
}

impl Node for Filter {
    type Ports = FilterPorts;
    type Params = FilterParams; // any serde type with a Default; `()` for none

    fn new(params: FilterParams) -> triq::Result<Self> {
        Ok(Self { alpha: params.alpha })
    }

    // Called when an input has data.
    fn run(&mut self, ports: &mut FilterPorts) -> triq::Result {
        while let Some(pose) = ports.raw.recv()? {
            ports.smooth.send(self.blend(&pose))?;
        }
        Ok(())
    }
}
```

The port struct is the whole declaration. The field names are the port names,
and a factory advertises them without creating a node. Optional parts of `Node`:

| Method | Purpose |
| --- | --- |
| `trigger()` | `Trigger::Input` (default), `Every(period)`, or `InputOrEvery(period)` |
| `tick()` | called on the period; does what `run` does unless overridden |
| `set_params()` | accept new settings while running |

## Running nodes

```rust
let rt = Runtime::builder("robot")
    .pool("dsp", 4)           // each node keeps one of 4 threads: the fastest arrangement
    .shared_pool("heavy", 8)  // any free thread takes a ready node: a blocked node delays nobody
    .start()?;                // does not take over the calling thread

rt.spawn::<Filter>("filter", Placement::pool("dsp"), FilterParams { alpha: 0.2 })?;
rt.spawn::<Imu>("imu", Placement::Dedicated, ())?;   // a thread of its own

rt.register::<Filter>("filter")?;  // a factory: any program may now ask for one
rt.connect("imu/pose", "filter/raw")?;
rt.wait();                         // or carry on with your own main loop
```

## Wiring

An output is named `runtime/node/port`, for example `robot/imu/pose`. That is
also the name of its iceoryx2 service, so iceoryx2's own tools list it.

- **Fan-out:** connect one output to many inputs.
- **Fan-in:** connect many outputs to one input; `sample.source()` says which.
- A connection is remembered by the receiving runtime as "this input wants
  that output". The output need not exist yet, and when its program stops and
  comes back, even after a crash, data flows again with nothing reconnected.
- iceoryx2 refuses a connection whose message types differ.

## From another program

```rust
for inventory in triq::discover()? { /* runtimes, pools, factories, nodes, ports, connections, counters */ }

let robot = triq::Remote::open("robot")?;
robot.spawn("filter", "filter_2", Placement::pool("dsp"), json!({ "alpha": 0.5 }))?;
triq::connect("robot/imu/pose", "robot/filter_2/raw")?;

let watch = triq::Watch::new()?;       // hears any runtime start, stop or change
watch.wait(Duration::from_secs(1))?;   // then call discover() again
```

The program asking owns nothing. Each request is carried out by the runtime
that owns the node, and a refusal comes back as the same error it would raise
locally. Opening a runtime costs many times what asking it something does, so
a thread keeps the runtimes it has opened: `Remote::open` and `discover` are
cheap from the second time on, and a runtime that stopped or started again is
noticed. The `triq` command-line tool does the same from a terminal:

```text
triq list                 # or `triq watch` to keep it on screen
triq spawn robot filter filter_2 --pool dsp --params '{"alpha":0.5}'
triq connect robot/imu/pose robot/filter_2/raw
triq set robot/filter_2 '{"alpha":0.1}'
triq pause | resume | remove robot/filter_2
```

## The workbench

```sh
cargo run -p triq-workbench
```

Run it and it shows every runtime on the machine: their pools, the kinds of
node they offer, their nodes as cards, and their connections as wires. It owns
no nodes and keeps no state of its own; the drawing follows what the runtimes
report and updates when they announce a change.

- Every runtime has a colour, shown beside its name on the left. A node wears the colour of the
  runtime it runs in: its card, its pins, and the wires that leave it.
- Click a kind on the left (or right-click the canvas) to create a node: name, thread or pool, settings.
- Drag from an output pin to an input pin to connect them. Only pins of the same message type snap together.
- Right-click a wire or a pin to disconnect it.
- Click a node to see where it runs (its runtime, the process, the thread or pool), its counters
  and its settings, and to disconnect single sources.
- Pause a node or start it again with the button on its card. Right-clicking the card offers the
  same, and removing it.
- Drag a card anywhere on it to move it, scroll to move the view, pinch to zoom; `Fit` brings every
  card back into view and `Tidy` lays them out again.
- An input whose output does not exist yet says "waiting", and gets its wire when the output appears.
- Beside each pin that data is passing through is its rate, such as `120/s`. The details panel adds
  what was received and lost, the latency of the newest sample, the longest gap between two samples
  of late, and a node's last error.

The `Charts` tab, beside `Graph`, shows what connections do over time:

- On the left is every connection with four numbers as they stand: its rate, how many samples a
  second it lost, the longest gap between two samples of late, and the latency of the newest sample.
- Drag a number onto a chart and it is drawn there as a line. Several numbers can share a chart, and
  `Add chart` and `Charts side by side` arrange as many charts as wanted in rows and columns.
- `Show the last` sets how many seconds of the past every chart shows, up to the hour that is kept.
- The top and bottom of a chart are always the largest and the smallest value in view, written in
  its corners. Hover over a chart to read the lines at that moment.
- A chart ends a moment before the present and moves on with every frame the window draws, so its
  lines slide along evenly between one reading of the machine and the next.

Nothing in it is measured by the workbench. A runtime reports counters for each of its connections,
and the workbench reads them ten times a second and keeps what they say. So a line has ten points a
second, whatever the rate of the data. A rate is taken over the half second before each point, and
timed by when the samples were sent: both ends of a connection report the send time of their newest
sample, which the sender stamps on it. Counted against the clock of the readings, sixty samples a
second would read 58, 60 or 62. The longest gap is the one thing a count cannot tell, so every
input keeps it: the longest time from one sample to the next, by when they were sent, over the last
half second to second. A hiccup of the sender shows there, and so does the hole that lost samples
leave.

`graph.rs` (the drawing as plain data), `colors.rs` (a colour per runtime),
`history.rs` (what connections did, as plain data), `charts.rs` (the charts),
`feed.rs` (two background threads) and `app.rs` (the window) are about 1,670
lines together. The tests drive the real window code without opening one, with
a pointer where it matters, and write pictures to `target/tmp/workbench.png`,
`target/tmp/graph.png` and `target/tmp/charts.png`.

## Programs with their own main loop

A renderer or a device driver keeps its main thread. It can hold ports itself:

```rust
let ports: CameraPorts = rt.attach("camera")?;   // the node lives as long as the ports
ports.image.send_slice(&pixels)?;                // whenever the application likes
```

Or offer a factory whose nodes it creates itself, picking up requests in its own loop:

```rust
let cameras = rt.local_factory::<CameraPorts, CameraParams>("camera")?;
// in the application's loop:
while let Some(request) = cameras.try_next() {
    match request {
        Local::Spawn { name, params, ports } => { /* keep the ports */ }
        Local::Params { name, params } => { /* new settings */ }
        Local::Pause { name } | Local::Resume { name } => { /* stop or start its work */ }
        Local::Remove { name } => { /* drop the ports */ }
    }
}
if rt.stopping() { /* another program asked this one to shut down */ }
```

A spawn request returns at once with the ports already bound, so nobody waits
for the application's loop. The application says what only it knows with
`rt.report(node, problem)`, which shows as the node's error, and
`rt.set_factory_info(kind, json)`, which shows beside the factory (the
controllers that are plugged in, for example). See `examples/app_loop.rs`.

## Programs built on it

`triq-xbox` and `triq-bevy` are ordinary users of the library, written to try
it on real work. Each keeps its own main loop and offers local factories.
`triq-demo` is the workbench with their two viewers under it.

```sh
cargo run -p triq-xbox                 # runtime "xbox": game controllers, read with SDL3
cargo run -p triq-bevy                 # runtime "bevy": a scene; add --headless for no window
cargo run -p triq-bevy -- --fps 120    # its frame rate when its window is not in front, or it has none
cargo run -p triq-demo                 # runtime "views": the workbench, and what the views show
```

In the window:

1. Create an `xbox` node. The factory lists the controllers it sees; put one's id in the settings, `{"device": 3}`.
2. Create a `rig` and one or two `camera` nodes (`{"mount": 0}` and `{"mount": 1}` sit side by side).
3. Drag `state` to the rig's `controller`, and the rig's `mounts` to each camera's `mounts`.
4. Create a `controller_view` and a `frame_view` per camera. Drag `state` to the controller view as
   well (an output feeds as many inputs as you like), and each camera's `image` to a frame view.

The left stick moves the rig, the right stick turns it, the triggers lower and
raise it. The Bevy window shows the scene from above and each camera's picture.
A camera sends its frames as `image: u8[] + Frame` to whatever is wired to it;
with nothing wired, it copies and sends nothing.

A camera cannot send more pictures than the program has frames. With its
window in front the program follows the display. Otherwise it keeps time
itself, sixty frames a second unless `--fps` says otherwise, where Bevy alone
would fall back to sixty whenever its window is not in front. Asked for as
many pictures as there are frames, a camera sends every frame; asked for
fewer, it sends them evenly.

| Crate | Files | What to read it for |
| --- | --- | --- |
| `triq-xbox` | `lib.rs` 91, `host.rs` 173, `sdl.rs` 93, `viewer.rs` 130 | a device as a node; which device is a setting, not a wire |
| `triq-bevy` | `lib.rs` 215, `rig.rs` 67, `scene.rs` 446, `viewer.rs` 140 | nodes inside an engine's own loop; large frames with a header |
| `triq-demo` | `lib.rs` 57, `main.rs` 22 | a project's own window: the workbench and its viewers together |

Their messages and settings build without SDL or Bevy (`default-features = false`),
so another program can use `XboxState` or `Frame` without either.

## Viewers

triq has no notion of a viewer, and the workbench cannot draw a message it
knows nothing about. Neither needs to. A viewer is two ordinary things, kept
in the crate that owns the message type:

- a kind of node with an input, offered as a local factory by whichever program has a window;
- a function that draws what those nodes last received.

```rust
let mut views = FrameViews::new(&runtime)?;   // offers `frame_view` nodes on this program's runtime
// every frame, wherever the pictures should go:
views.show(ui);                               // takes each node's newest frame and draws it
```

Because a view is a node, every tool already lists it, wires it, pauses it and
removes it, and iceoryx2 already refuses to wire it to the wrong kind of
output. Nothing was added to triq or to the workbench for `ControllerViews`
(`triq-xbox/src/viewer.rs`) or `FrameViews` (`triq-bevy/src/viewer.rs`); a
third crate adds a third viewer the same way.

A project that wants its views in the workbench's window writes that window.
The heart of `triq-demo` is this, around a runtime to hold the view nodes:

```rust
egui::Panel::bottom("views").show(ui, |ui| {
    self.controllers.show(ui);
    self.frames.show(ui);
});
self.workbench.show(ui);
```

A view can as well live in a window of its own, or inside the program that
produces the data; it is only a node and a drawing function.

## Delivery

The only queue is iceoryx2's, one per connected input. An output chooses how it behaves:

| `#[port(...)]` | On | Meaning | Default |
| --- | --- | --- | --- |
| `queue = N` | output | samples each input can have waiting | 32, or 2 for slices |
| `reliable_ms = N` | output | wait up to N ms for a full input, then fail with `Error::Undelivered` | off: a full queue drops its oldest sample and the sender never waits |
| `subscribers = N` | output | how many inputs may connect | 8 |
| `hold = N` | output | samples an input may hold at once | 2 |
| `depth = N` | input | samples to keep waiting, at most the output's `queue` | the output's `queue` |

A node that wants a different buffer builds it in plain Rust from what triq reports:
`send` returns how many inputs received the sample, `output.subscribers()` says
whether anyone is connected, and `input.lost()` counts samples that were overwritten.

Sending: `send(value)` copies a value into shared memory once; `loan()` gives
shared memory to fill in place; slices (`Output<[T]>`) take any length with no declared size.

## Headers

A slice says nothing about itself but its length. What else the receiver needs
goes in a header: a second, small `#[repr(C)]` type that travels in the same
sample, beside the data.

```rust
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[repr(C)]
#[type_name("Frame")]
struct Frame { width: u32, height: u32, number: u64 }

#[derive(Ports)]
struct CameraPorts {
    image: Output<[u8], Frame>,          // pixels, and what they are
}

let mut frame = ports.image.loan(width * height * 4)?;
render_into(&mut frame);                 // the pixels, written once, in place
*frame.header_mut() = Frame { width, height, number };
frame.send()?;

let frame = viewer.image.recv()?.unwrap();   // viewer.image: Input<[u8], Frame>
show(&frame, frame.header().width, frame.header().height);
```

The header is part of the message type, shown as `u8[] + Frame`, so an input
that expects another header, or none, is refused when it is connected. Without
it the size would have to be packed into the first bytes of the pixels, by a
convention each side has to know and nothing checks.

## Where the code is

| File | Lines | What it holds |
| --- | ---: | --- |
| `message.rs` | 194 | message types, headers, the stamp on every sample |
| `port.rs`, `port/output.rs`, `port/input.rs` | 710 | `Output`, `Input`, `Sample`, `Loan`, port declarations |
| `node.rs` | 252 | the `Node` trait, `Trigger`, `Placement` |
| `executor.rs` | 206 | the thread that waits and runs nodes |
| `executor/workers.rs` | 57 | the optional dispatcher behind shared pools |
| `runtime.rs` | 652 | `Runtime`: spawn, factories, connect, inventory |
| `runtime/local.rs` | 117 | factories whose nodes the application runs |
| `control.rs`, `remote.rs` | 438 | requests between programs, discovery, change announcements |
| `inventory.rs`, `error.rs`, `raw.rs`, `lib.rs` | 306 | report types, the error type, iceoryx2 type names |

About 2,930 lines including comments, plus the derive macro (66) and the command-line tool (158).
Tests are in `crates/triq/tests`, one file per subject.

## Measurements

`triq-bench` reproduces the shape of zegra's saved benchmarks: the same node
counts, rates, sizes, pairing and 200 µs deadline. It is a new program, not
zegra's harness, and each line is a single run on a desktop that was not idle
(Apple M5, macOS 27, release build, AC power). Latency is from just before a
node hands a message to its output until the receiving node has it.

| Workload | zegra (saved reports) | triq |
| --- | --- | --- |
| Two processes, 128 bytes at 10 kHz, 30 s | p50 17, p99 34, max 2,736 µs; 3 lost | p50 5, p99 8, max 105 µs; 0 lost |
| 30 nodes at 5 kHz, one process, 3 pools of 10, 60 s | through IPC: p50 2.19 s, 55% lost. With its direct path: p50 10, p99 88, max 6,551 µs; 15,211 late | p50 7, p99 26, max 439 µs; 0 lost; 9 late |
| Same, 5 pools of 6 | direct path: p50 11, p99 53, max 1,143 µs; 1,128 late | p50 7, p99 24, max 126 µs; 0 lost; 0 late |
| Same, 3 processes of 10 threads | p50 996, p99 70,300 µs; 321,774 lost | 0 lost in two runs. One: p50 7, p99 22, max 1,958 µs, 317 late. The other, which generated only 98.7% of its nominal rate: p50 7, p99 47, max 33,798 µs, 21,041 late |
| Same, 5 processes of 6 threads | p50 94, p99 692, max 10,182 µs; 53 lost | p50 7, p99 19, max 212 µs; 0 lost; 2 late |

Other findings:

- A 1 MiB frame filled in place takes as long as 128 bytes (p50 30 µs at 1 kHz); copied in, p50 111 µs.
- At 1 kHz the median is about 30 µs whatever the size. That is the operating
  system waking an idle thread; a receiver that polls sees under 1 µs.
- Shared pools cost more than expected under this load: p50 26, p99 66 µs
  against 8 and 44 for ordinary pools.

```sh
cargo run --release -p triq-bench -- hop --rate 10000 --bytes 128 --seconds 30
cargo run --release -p triq-bench -- mesh --groups 3 --threads 10 --seconds 60 [--processes] [--shared]
```

## Not there yet

- Anything across the network.
- The workbench does not remember where cards were placed between runs.
- `triq-xbox` is tested with pretend controllers; nothing in the test suite reads a real one.
- Serde-encoded messages as a convenience next to `#[repr(C)]` data.
- Tested on macOS only. Requests are served one at a time, so a slow `Node::new` delays other requests to that runtime.

## Build and test

```sh
cargo test -p triq                # the library alone
cargo test --workspace            # everything; the first build compiles Bevy and SDL3
cargo clippy --workspace --all-targets -- -D warnings
cargo run --example pipeline      # one process
cargo run --example station -- left   # then see the top of examples/station.rs
```

triq depends on a pinned commit of the iceoryx2 fork that carries the macOS
file-descriptor and kqueue fixes; see the workspace `Cargo.toml`.
