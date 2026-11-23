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

## In a project

```toml
[dependencies]
triq = "0.1"
# Only for a program that draws the project's own views of its messages:
triq-views = "0.1"
```

On macOS, add this once per machine to Cargo's own settings,
`~/.cargo/config.toml`. ["Platforms"](#platforms) says why.

```toml
[patch.crates-io]
iceoryx2 = { git = "https://github.com/Derekbenj/iceoryx2.git", rev = "638d936394bc5ceff3462c23dbcb4b7c220a040e" }
```

If a project has only nodes, it depends on `triq` alone and builds no window
code. `#[derive(ZeroCopySend)]` comes with `triq::prelude`, so a project never
needs iceoryx2 as a dependency of its own.

| Crate | What it is |
| --- | --- |
| `triq` | the library, and the `triq` command-line tool |
| `triq-macros` | `#[derive(Ports)]` and `#[derive(Fields)]`; comes with `triq` |
| `triq-views` | a window for a project's own views of its messages; see [Views of your own](#views-of-your-own) |
| `triq-tap` | reads any iceoryx2 topic as bytes; the workbench uses it, and it holds the only `unsafe` code |
| `triq-workbench` | the window: a program to run, and a library to draw it in a window of your own; see [The workbench](#the-workbench) |
| `triq-snarl` | the graph the workbench draws: [egui-snarl](https://github.com/zakarumych/egui-snarl), ported to egui 0.37, with the changes the workbench needs; see [its README](crates/triq-snarl/README.md) |

### Platforms

| System | Where triq stands |
| --- | --- |
| macOS | Written and tested here. Needs the iceoryx2 fork, set once per machine as above. |
| Linux | Compiles (docs.rs builds it on Linux), but has not been run. triq has no platform-specific code and iceoryx2 supports Linux, so it is expected to work. |
| Windows | Not run yet. |

**Why macOS needs the fork.** A crate on crates.io may only depend on
crates that are also on crates.io, so the published triq builds on the
released iceoryx2 0.10 (0.10.1 at the time of writing). On macOS, that keeps several files open for
every port and waits on them with `select`, which cannot handle a file
numbered 1024 or higher. This is how many ports a chain of nodes, each with
one input and one output, could have:

| | iceoryx2 0.10 as released | with the fork |
| --- | --- | --- |
| the usual limit of 256 open files | 10 ports; more fail with `PublishSubscribeOpenError::InternalFailure` | 200 ports; the open-file limit itself is reached before 300 |
| the limit raised (`ulimit -n`) | 60 ports; with more, the program aborts: `fd … out of range: valid fds are 0..FD_SETSIZE` | 600 ports; no upper limit was looked for |

The setting replaces that release with [a fork](https://github.com/Derekbenj/iceoryx2):
the same release plus four fixes. Two of them stop it holding files open;
iceoryx2 has since merged those for its next release. The other two are the
fork's own: file numbers are checked before `select` is used, and macOS waits
with kqueue instead.

Set once in Cargo's own settings, it reaches every project on the machine and
every program installed with `cargo install`, the workbench among them, with
nothing in a project's own `Cargo.toml`. Cargo ignores such a section in the
manifest of a dependency, which is why triq cannot supply it for you. (The
same section in a project's own `Cargo.toml` does the same for that project
alone.)

The patch is to iceoryx2 0.10. A project that comes to depend on a later
iceoryx2 is not patched: Cargo says the patch was not used, and builds on the
release.

A project that depends on triq by its git address does not need the setting,
because in the repository triq's own manifest names the fork:

```toml
triq = { git = "https://github.com/Derekbenj/triq" }
```

## A node

```rust
use triq::prelude::*;

#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
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
and a factory advertises them without creating a node. `Fields` makes the
message say which numbers it holds, so that tools can show them ([below](#what-a-message-holds)).
Optional parts of `Node`:

| Method | Purpose |
| --- | --- |
| `trigger()` | `Trigger::Input` (default), `Every(period)`, or `InputOrEvery(period)`; `.and_every(period)` after any of them adds a period |
| `tick()` | called when a period elapsed, with the number of that period; does what `run` does unless overridden |
| `set_params()` | accept new settings while running |

`run` is called because data arrived, not once for every message: several may
be waiting. Take them all with `while let Some(sample) = ports.raw.recv()?`,
or only the newest with `ports.raw.latest()?`. A node that takes one message
and returns is run again at once while more waits for it, so it does not fall
behind without anyone noticing; the other nodes on its thread have their turn
in between.

A node can have several periods, each timed on its own, whether or not data
wakes it as well. `tick` is told which elapsed by its number: 0 for the first
the trigger was given, 1 for the next. This one takes what arrives as it
arrives, sends its state a hundred times a second, and its health once a second:

```rust
fn trigger(&self) -> Trigger {
    Trigger::InputOrEvery(Duration::from_millis(10)).and_every(Duration::from_secs(1))
}

fn run(&mut self, ports: &mut EstimatorPorts) -> triq::Result {
    while let Some(imu) = ports.imu.recv()? {
        self.take(&imu);
    }
    Ok(())
}

fn tick(&mut self, ports: &mut EstimatorPorts, period: usize) -> triq::Result {
    match period {
        0 => ports.state.send(self.state)?,
        _ => ports.health.send(self.health())?,
    };
    Ok(())
}
```

Periods that elapse together are one wake-up: `run` first if data arrived,
then `tick` for each of them in order. A period that elapses again before the
node was called for it is one call, not two. An error from one call does not
cost the others theirs. The periods may come from the node's settings, and
change when those do. A node has room for 63.

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

A node that needs more than its settings when it is created is made by a
closure. The closure is given the name and the settings of each node that is
asked for, and holds whatever else the node needs:

```rust
rt.register_with("recorder", move |name, params| Ok(Recorder::new(name, params, shelf.clone())))?;
```

## Wiring

An output is named `runtime/node/port`, for example `robot/imu/pose`. That is
also the name of its iceoryx2 service, so iceoryx2's own tools list it.

- **Fan-out:** connect one output to many inputs.
- **Fan-in:** connect many outputs to one input; `sample.source()` says which.
- A connection is between two ports that are there, and is kept by the
  receiving runtime. Connecting to an output that no node sends on is
  refused, with `Error::Unknown` naming the output, as a misspelt name is:
  nothing waits for an output to appear. So a program that wires itself to
  another in code does so once the other runs, or tries again.
- A connection ends with the node it is to. When the node behind an output is
  removed, or its program stops or crashes, every input that heard it lets go
  of that output within a quarter of a second, with whatever of it the input
  had not yet taken. A node that takes the name later is another node: it is
  heard once it is connected, and not before, not even for that quarter second.
- iceoryx2 refuses a connection whose message types differ.

## Groups of nodes

A node has room for the name of one group. Its runtime keeps the name and
reports it, and does nothing else with it. The name is for a tool that draws
the machine: the workbench draws the nodes of a group as one card.

```rust
rt.set_group("imu", "sensing")?;          // another program asks the same of a `Remote`
rt.set_group("filter", "sensing/fine")?;  // a group inside that one: the outer one first
rt.set_group("imu", "")?;                 // in no group again
```

A group is there for as long as a node says it is in it. A node that is
removed takes its group with it, and so does a program that stops: when it
runs again, its nodes are in the groups it puts them in, and in no others.

A runtime can also offer a whole group as one kind: several nodes of kinds it
has registered, wired to each other. It is written as the calls that would
make it by hand are:

```rust
rt.register::<Estimator>("estimator")?;
rt.register::<Controller>("controller")?;
let stack = Group::new()
    .spawn("estimator", "estimator", Placement::Dedicated, json!(null))
    .spawn("controller", "controller", Placement::pool("control"), json!({ "gain": 2.0 }))
    .connect("estimator/state", "controller/state")
    .connect("robot/imu/pose", "estimator/pose");   // an output that is not of the group
rt.register_group("flight_stack", stack)?;
```

Any program may then ask for one, as it asks for a node. Asked for under the
name `drone`, it is the nodes `drone.estimator` and `drone.controller`, in
the group `drone`, with those settings, on those threads, and wired:

```rust
robot.spawn("flight_stack", "drone", Placement::Dedicated, json!(null))?;
```

- The placement and the settings in the request are not used: each node has
  the ones its group gives it.
- A group is made whole or not at all. If one of its nodes cannot be created
  or one of its wires cannot be drawn, the nodes that were made are removed again.
- A mistake in a group is refused when the group is registered, not when one
  is first asked for: a kind that is not registered, two nodes of one name, a
  wire to a port that is not there, or a wire between ports of different messages.
- The nodes of a group may be of kinds the application runs itself (see
  [Programs with their own main loop](#programs-with-their-own-main-loop)).
  A kind that is itself a group is not one of them: a group inside a group
  is written out, as follows.

Some of the nodes may be in a group inside the group. That one is written as
a group is, and given a name:

```rust
let perception = Group::new()
    .spawn("camera", "camera", Placement::Dedicated, json!(null))
    .spawn("estimator", "estimator", Placement::Dedicated, json!(null))
    .connect("camera/image", "estimator/image");                 // by the names its nodes have in it
let stack = Group::new()
    .spawn("controller", "controller", Placement::pool("control"), json!({ "gain": 2.0 }))
    .group("perception", perception)
    .connect("perception.estimator/state", "controller/state");  // outside it, they are called after it
rt.register_group("flight_stack", stack)?;
```

Asked for under the name `drone`, that is `drone.controller` in the group
`drone`, and `drone.perception.camera` and `drone.perception.estimator` in
the group `drone/perception`.

- A wire is written in the group that holds both its ends. A wire of
  `perception` is between its own nodes, or from an output that is not of
  the group at all; one that reaches `controller` is a wire of `stack`.
- A group inside may have groups inside it.
- The names add up, and a node's whole name, `drone.perception.estimator`,
  is at most 32 characters.

## From another program

```rust
for inventory in triq::discover()? { /* runtimes, pools, factories, nodes, ports, connections, counters */ }

let robot = triq::Remote::open("robot")?;
robot.spawn("filter", "filter_2", Placement::pool("dsp"), json!({ "alpha": 0.5 }))?;
triq::connect("robot/imu/pose", "robot/filter_2/raw")?;

let watch = triq::Watch::new()?;       // hears any runtime start, stop or change
watch.wait(Duration::from_secs(1))?;   // then call discover() again
```

The program that asks owns nothing. Each request is carried out by the runtime
that owns the node, and a refusal comes back as the same error it would raise
locally. Opening a runtime costs many times as much as asking it something, so
a thread keeps the runtimes it has opened: `Remote::open` and `discover` are
cheap from the second time on, and a runtime that stopped or started again is
noticed. A thread lets go of what it kept when it ends. The main thread of a
program does not end that way, so a program that looked at other runtimes
calls `triq::release()` before `main` returns. Then it leaves nothing of
itself on the machine for the next program to clear away.

The `triq` command-line tool does the same from a terminal. Install it with
`cargo install triq`; on macOS, add `--locked --git https://github.com/Derekbenj/triq`,
which builds it on the fork. Its commands:

```text
triq list                 # or `triq watch` to keep it on screen
triq spawn robot filter filter_2 --pool dsp --params '{"alpha":0.5}'
triq connect robot/imu/pose robot/filter_2/raw
triq set robot/filter_2 '{"alpha":0.1}'
triq group robot/filter_2 sensing   # put it in a group; without a name, in none
triq fields robot/imu/pose          # the numbers inside what an output sends
triq pause | resume | remove robot/filter_2
```

## The workbench

```sh
cargo install triq-workbench      # on macOS, with the iceoryx2 fork set once per machine: see Platforms
triq-workbench
```

From version 0.2.0 the workbench is on crates.io. Before that, and for what
is in this repository now, install it from here, which builds on the iceoryx2
fork with no patch, as triq's own manifest names it:

```sh
cargo install --locked --git https://github.com/Derekbenj/triq triq-workbench
```

In a checkout of this repository, `cargo run -p triq-workbench` does the same.
`--locked` builds it with the versions it was tested with. To draw the
workbench in a window of your own, depend on `triq-workbench`. Its graph is
drawn with `triq-snarl`, which comes with it.

When it runs, it shows every runtime on the machine: its pools, the kinds of
node it offers, its nodes as cards, and its connections as wires. It owns no
nodes and keeps no copy of the graph; the drawing follows what the runtimes
report and updates when they announce a change.

It is four windows in one: `Runtimes`, `Graph`, `Details` and `Charts`. To
begin with, the first three are side by side and the charts are under them.

- Each window sits under a bar with its name on it. Drag the name to move the window somewhere
  else: see "Arranging panes" below. Drag the line between two windows to give one of them more room.
- The cross beside a name closes the window. The row along the top lists every window; click a
  name there to open or close it. A window that is opened again returns to where it was.
- The workbench opens as it was when it last closed: the size and place of its window, how the
  four windows and the charts are arranged, which of them are open, what is on each chart, where
  the cards were put in the graph, which nodes are grouped, and the colours that were chosen for
  runtimes. It saves this from time to time, and when it closes.

### Arranging panes

The four windows are panes, and so are the charts within `Charts` and the views in a project's
own views program. All of them are arranged the same way, and any pane can go into any other. Drag
a pane by its name and drop it on one half of another pane, or on its middle. The highlight shows
where it will land, and it matters how high on the other pane you drop it:

| Dropped | High: the highlight covers the other pane's bar | Low: the highlight is entirely below that bar |
| --- | --- | --- |
| on a half | the two become neighbours | the pane joins the other one: the two become a **group** |
| on the middle | the two share one place, with both names on one bar | the same, unless the other pane already shares its place: then the pane joins that pane alone, and the two get a bar of their own under the first |

- A group has a bar of its own that names what is in it, such as `Graph, Charts` or `Charts 2, 3`.
  It keeps that bar even when nothing is beside it. The panes in a group keep their own bars, so
  groups can go into groups, and one bar can sit under another.
- Drag a group by its name, as you would a pane; it is still a group where you drop it. Dragging
  a bar by the space beside its names moves everything on that bar. The cross beside a group's
  name closes everything in the group.
- A pane dropped on a bar, among its names, becomes one more name on that bar.
- Clicking a name on a bar brings its pane to the front.
- A group that is left with only one pane stops being a group, and its bar disappears.
- `Runtimes`, `Details` and the list of numbers in `Charts` are laid out in the width they are
  given: what is written wraps, and what is filled in has less room. Each has a least width, about
  200 points. Made narrower than that, a pane squeezes nothing further, and cuts off what does
  not fit. The list of numbers gives way to the charts beside it down to its least width.

### The graph

In `Runtimes`, `Graph` and `Details`:

- Every runtime has a colour, shown beside its name in `Runtimes`. A node wears the colour of the
  runtime it runs in: its card, its pins, and the wires that leave it. Click the colour to choose
  another with a picker; `auto` beside it gives the runtime a colour of the workbench's choosing
  again. A chosen colour belongs to the runtime's name, and is kept.
- Click a node, and it keeps its colour together with everything that feeds it and everything
  that it feeds, directly or by way of other nodes: where its data comes from, and where it goes.
  Every other card goes grey, with its wires. A node that merely hears the same output is one of
  those: with two rigs on one controller, a click on one rig greys the other and what hangs on
  it. A click beside the cards brings the colours back.
- Click a kind in `Runtimes` to create a node: name, thread or pool, settings. The form stays after
  `Create`, under the next name that is free, for another node of the kind. A kind that is a group
  of nodes asks only for a name, and says which nodes it creates.
- Drag from an output pin to an input pin to connect them. Only pins of the same message type snap together.
- Right-click a wire or a pin to disconnect it.
- Click a node to see where it runs (its runtime, the process, the thread or pool), its counters
  and its settings, and to disconnect single sources. `Details` opens for it if it was closed.
- Pause a node or start it again with the button on its card. `Details` has the same button, and
  one to remove the node.
- Drag a card by any part of it to move it, scroll to move the view, and pinch to zoom. `Fit`
  brings every card back into view and `Tidy` lays the cards out again. When the graph's window is
  moved or resized, whatever was in its middle stays in the middle.
- A card stays where it is put. It is known by the names of its runtime and node: when its program
  stops and runs again, also in a later run of the workbench, the card is back in its place. `Tidy`
  forgets where cards were put. The places of the last 500 cards are kept.
- A wire is between two nodes that are there, and ends with the node at either end of it. Remove
  a node, or stop its program, and the wires to what heard it are gone: nothing waits for a node,
  and a node that is later given its name starts with no wires.
- Beside every pin is the rate of what passes through it, such as `120/s`, or `0/s` when nothing
  does: what a node takes in is on its left, and what it sends is on its right. `Details` adds how
  much was received and lost, the latency of the newest sample, the longest recent gap between two
  samples, and a node's last error.

### Drafting

`Draft`, above the graph, begins a draft. While there is one, nothing you do in the graph or in
`Details` is asked of a runtime: it is held back, and the windows show the machine as it would be.

- Everything can be drafted: wires drawn and cut, settings, pausing and resuming, removing a node,
  creating one, and which group a node is in. A node that is to be created is drawn with the ports
  its factory says it will have, and can be wired like any other. So are the nodes of a group that
  is to be created.
- What is only drafted is grey, with a glow around it: a new node, and a new wire with the pins at
  its ends. A wire that is to be cut is faint, and has the same glow along it. A card that is to
  be changed or removed keeps its colour, has the glow, and says what is to happen to it.
- A draft holds what it takes to get from the machine to the drawing and no more: one change for
  each thing that differs. Draw a wire and take it away again twenty times, and there is nothing
  to do. With nothing selected, `Details` lists the changes in the order they were made; the
  cross beside one takes it back.
- `Apply draft` asks the runtimes for the changes, one after another in that order, and ends the
  draft. If a runtime refuses one, that change and those after it are not made: they are a draft
  again, and the row along the top says what went wrong. `Discard draft` forgets the changes.
- To replace a node that is running, start the new one beside it, wire its inputs, and compare the
  two. Then draft the wires that move from the old one to the new one, and apply.

The changes are made one request after another, not at one instant. If a wire is cut before its
replacement is drawn, the input hears nothing for the time of one request; in the other order it
hears both for that long. The order of the draft is the order you made the changes in.

### Groups

Nodes can be put together in a group, which is drawn as one card. The keys are Blender's, with the
command key of a Mac where Blender has Ctrl, and work while the pointer is over the graph. On Linux
and Windows, Ctrl takes the place of `Cmd`; on a Mac it works as well.

| To | Do this |
| --- | --- |
| select several cards | hold Shift and click them, or hold Shift and drag across them |
| let go of all that is selected | click beside the cards |
| let go of one card | hold `Cmd` and click it |
| make a group of the selected cards | `Cmd+G`, or `Make Group` in `Details` |
| put the selected cards into a group that is there | select that group last and press `Cmd+Shift+G`, or `Move into` in `Details` |
| see what is inside a group | select its card and press `Tab`, double-click the card, or press `Edit` on it |
| go one group out | `Tab` again, `Ctrl+Tab`, or `Exit Group` above the graph |
| move the selected cards one group out, from inside | `P`, or `Move out` in `Details` |
| take a group apart | select its card and press `Cmd+Option+G`, or `Ungroup` in `Details` |

- The card of a group is outlined in white, or in black on a light background. Its header fades
  through the colours of the runtimes its nodes run in, and has a dot for each of them. It is never
  grey: grey is what a card is that neither feeds the selected one nor is fed by it.
- The card of a group shows the ports of its nodes that are connected to nothing, or to something
  outside the group, each as `node.port`. Wires between the nodes of a group are not shown from
  outside. A pin of a group is the port of its node: wire it as you would the node's own pin.
- Inside a group, two more cards stand for the outside, as in Blender, and show the ports out
  there in the same way. `Group Input` has the outputs that the group hears, and `Group Output`
  the inputs that hear it. With them are the ports that nothing is connected to yet, of the nodes
  around the group: those of the group that this one is inside of, or of the whole machine if it
  is inside none. So a group is wired to its surroundings without leaving it: every wire in there
  is one connection, drawn and cut like any other.
- A pin on one of those two cards is the port of a node outside the group, written `node.port`.
  Hover over it to see it in full, with its runtime. A wire from `Group Input` straight to
  `Group Output` is refused: it would connect nothing of the group.
- In a draft, a port whose last wire is cut counts as connected to nothing at once: it is offered
  on the card of its group, and on the two cards for the outside, with the glow of what is
  drafted. Cut a wire inside a group, step out, and both its ends are pins of the group's card,
  to be wired there in the same draft.
- What is created while the graph shows the inside of a group is in that group: a node, or a
  group of a kind that its runtime offers, which is then a group inside the one that is shown.
- The card of a group says how many nodes it has, and how many of them are paused or stopped.
  Click it to see the group in `Details`: there it can be renamed, and its nodes are listed.
- A group can be inside a group. The card of a group is selected and grouped like a node's, and
  `Cmd+G` inside a group makes the new group there. Above the graph it says where you are, such
  as `inside sensing/filters`.
- Everything goes one group at a time. `Tab` and `Ctrl+Tab` leave to the group around the one that
  is shown. `P` moves the selected cards into that group, and out of every group only if there is
  none around. `Cmd+Option+G` takes one group apart: what was in it is where the group was.
- A node is directly in one group at most. A group is there for as long as a node is in it.
- Which group a node is in is kept by the node's runtime, not by the workbench (see
  [Groups of nodes](#groups-of-nodes)). The workbench asks the runtime to put the node in the
  group, and draws what the runtime reports. So a group is the same in every workbench that is
  open, and is still there when the workbench is closed and opened again. A node that is removed
  takes its place in the group with it, and a program that is started again puts its nodes in
  the groups it chooses. A node in a group runs exactly as before.
- A program that was built with a triq from before there were groups cannot be asked: its nodes
  stay where they are, and the row along the top says so. Build it with this triq, and they can.
- A group's name is plain, like a node's: letters, digits, `-`, `_` and `.`. Anything else in a
  name that is typed becomes `_`.
- While drafting, grouping is held back with the rest: the graph shows the groups as drafted, and
  the runtimes are asked when the draft is applied.

### Charts

`Charts` shows what connections and outputs do over time:

- On the left is every connection with its four current numbers: its rate, how many samples a
  second it is losing, the longest recent gap between two samples, and the latency of the newest
  sample. Every output is there too, with the rate it sends at. They are sorted by name, so what a
  node takes in and what it sends appear together, and so does everything of one runtime. Each
  has a dot in the colour of its runtime before its name. The box at the top of the list names
  the runtimes, each beside its dot: choose one to list its numbers alone, or `every runtime`.
- Open an output to see the `numbers inside` its messages, each with its value in the newest
  message: `translation[0]`, `header.width`, the `length` of a slice. They can be dragged onto
  charts like the rest, and are drawn for every message, at the time it was sent.
- A topic that no triq runtime answers for is listed as well: one that another iceoryx2 program
  publishes, or an output whose program has stopped while something still listens to it. Nothing
  describes what is inside its messages, so only the length of a slice can be drawn.
- Drag a number onto a chart and it is drawn there as a line. Several numbers can share a chart.
  Their names are written over the top of the chart, and the lines pass behind them: a chart is as
  tall with ten lines as with one.
- Each line has a colour of its own. The button beside its name shows that colour and opens a picker.
- The `…` button beside a line opens how the line is drawn. With `Fade into the past` ticked, the
  line is more see-through the older it is, and two numbers under it say by when it is gone.
  The first is a share of the time the chart shows: at 50 %, on a chart that shows the last 30
  seconds, the line is gone after 15, and at 10 % after 3. The second is the same in seconds.
  Type into either. A number of seconds holds however much the chart shows, and may be more
  than that: then the line never quite goes. A share goes with the chart. Where a line is
  newest it always has its full colour.
- The eye beside a name hides that line and shows it again. A hidden line keeps its name on the
  chart, written faintly, and the chart no longer reaches for its values.
- `Legend`, on each chart, hides all the names at once to leave the lines the room, and shows them
  again.
- Drag a line by its name onto another name of the same chart, and it takes that place among the
  names. Lines are drawn in the order of their names, the last over the others.
- Drag a line by its name onto another chart to move it there. It keeps a colour that was chosen
  for it, and otherwise takes the first that is free on that chart.
- Click a number in the list to choose it, and hold Ctrl (Cmd on a Mac) while clicking to choose
  several. Hold Shift while clicking to choose every number from the one chosen last to this one.
  Dragging any chosen number takes the others with it, in the order they were chosen.
- `New chart` adds an empty chart beside the others. Charts are panes, arranged by hand like the
  windows, to any depth: neighbours, groups, and several in one place. Drag the line between two
  charts to give one more room and the other less.
- Hold a number you are dragging over the name of a chart that is behind another, and that chart
  comes to the front so that you can drop the number on it. The cross beside a name closes the
  chart, or all the charts of a group; the last chart is closed
  like any other, and `New chart` makes one again.
- `Numbers` hides the list, which gives its room to the charts, and shows it again. While the list
  is hidden, only the numbers that are on charts are read out of messages.
- A number on a chart is identified by name: the names of its runtime, node and port. It is
  remembered from one run to the next: it says `nothing yet` while nothing with those names is
  running, and is drawn as soon as something is. Anything new that is running appears in the list
  as usual, and on no chart until it is dragged onto one. The `×` beside a name removes that line.
- `Show the last`, written on every chart, sets how many seconds of the past that chart shows, up
  to the one hour that is kept. A new chart shows thirty.
- The top and bottom of a chart are always the largest and the smallest value in view, written at
  its right edge. Hover over a chart to read the value of every line at that moment. `Reverse`
  `y` turns the chart upside down: the smallest value is at the top.
- A chart ends at the present and moves on with every frame the window draws. A number from inside
  a message is drawn right up to the edge, as its message arrives. The runtimes' reports arrive a
  tenth of a second apart, so those lines end 0.15 s short of the edge and slide along evenly
  between one report and the next.

`New 2D chart` and `New 3D chart` add charts of numbers against each other: two numbers of one
message as a point on a plane, or three as a point in space. A chart is of one kind from when it
is made:

- Drag the numbers of one message onto it one after another, the one along x first, then y, and
  on a 3D chart z; or choose them in that order and drag them together. They are one curve,
  called by its topic. Several curves can share a chart, each with its own numbers.
- A slot for each axis says which number is along it. Drag a number of the same topic out of
  the list onto a slot to put it there; if several were chosen, the others go into the slots
  after it. Drag a slot onto another slot of the curve to swap the two. A number that is put in
  one slot and was in another trades places with the one that was there, so both keep their past.
- The newest point of a curve is a dot. In `…`, `Draw its trail` also draws the path the point
  took over the time the chart shows, and `Fade into the past` makes the older part of that path
  more see-through.
- A curve has a past from when it was put on a chart. A number that is put in a slot and was on
  no chart has none, so the trail begins again.
- A chart shows everything there is, edge to edge, when it first has something to show and when
  you press `Fit`. Otherwise it stays where it is, and the point moves over it. The wheel, or a
  pinch, zooms about the middle of the chart, and dragging moves it.
- A chart does not lose sight of where a curve is now. By hand it cannot be zoomed or moved past
  the point where the newest point of a curve would leave it: the wheel and the drag stop there.
  When the curve itself goes where the chart does not reach, the chart is seen from just as much
  further away as it takes, about the same middle. To zoom in on the point itself, follow it.
- Under the curves is a grid. Its lines are at round values and stay where those values are, so
  when the chart follows a curve it moves over the grid, and the squares that pass show how far.
  The line where an axis is 0 is stronger. On a 2D chart the grid covers the chart, and each line
  says its value where it leaves it: x at the bottom, y at the right.
- `1:1` gives every axis one scale, so that shapes are as they are. Click it again to have each
  axis fill the chart. A 3D chart starts with it on, and a 2D chart with it off.
- `Reverse` has a button for each axis. While it is down, that axis runs the other way: x to the
  left, y down, and on a 3D chart whichever axis it is. A stick of a game controller is a case
  for it: SDL gives -32768 for all the way up and 32767 for all the way down, so pushing it up
  draws downwards until y is reversed.
- On a 3D chart the grid is the ground: the plane where the axis that is up is 0. `Ground` says
  which two axes it is along, `xy`, `xz` or `yz`, and the third is up. The ground is a part of
  the world, not of the chart. It lies about the point where every axis is 0, and reaches twice
  as far from there along each of its axes as the furthest that anything on the chart is along
  that axis, as a round number: with a point 7 away along x, to 20 either way. With `1:1` on it
  is a square, twice as far across as the furthest that anything is along any axis. It stays
  where it is while a point moves over it, and it is as large from near by as from far away.
- The values of the ground are written along its axes: those of x along the line where y is 0,
  and those of y along the line where x is 0, each with its letter after the last value. When
  the chart does not show that line, they are written along the nearest line it does show.
  Where all three axes are 0, each is drawn one square long: x in red, y in green and z in blue.
- A curve on a 3D chart stands on the ground. A thin line goes from its newest point straight to
  the ground, to a ring there, and says how far that is: `z 16.5`. The ring is where the point is
  over the ground, and the line is how high: together they tell a point that is near and low from
  one that is far and high. A 3D chart that shows everything shows the rings as well, and the
  point where every axis is 0.
- Dragging a 3D chart turns it about its middle. To move it instead, hold Shift while dragging,
  or drag with the other button of the mouse. Nothing is drawn smaller for being further away, so
  a unit is as long at the back as at the front.
- In `…`, `Keep its newest point in the middle` has the chart follow one curve: zooming is then
  about that point. Dragging the chart lets go of it.
- The row under a chart says which numbers are along x, y and z, in the colours of their curves.
  The box beside the colour of a curve says whether its numbers are named there: untick it for all
  but one curve when they are in the same units.
- A curve is dragged by the name of its topic onto another chart of the same kind to move it
  there. What a runtime counts for a connection or an output is not inside a message, and has no
  place on a 2D or 3D chart.

The workbench measures none of this itself. A runtime reports counters for each of its connections,
and the workbench reads them ten times a second and keeps what they say. So a line has ten points a
second, whatever the rate of the data. Asking the runtimes it knows takes a millisecond or two each.
Finding out which runtimes there are takes far longer, about a millisecond for every output on the
machine, so the workbench does that only when a runtime announces that it came, went or changed,
and every two seconds besides. A rate is taken over the half second before each point, and
is timed by when the samples were sent: both ends of a connection report the send time of their
newest sample, which the sender stamps on it. If samples were counted against the clock of the
workbench's readings instead, sixty samples a second would read as 58, 60 or 62. The longest gap is
the one thing a count cannot tell, so every input keeps it: the longest time from one sample to the
next, by when they were sent, over the last half second to one second. A hiccup in the sender shows
up there, and so does the hole that lost samples leave.

The numbers inside messages are the one thing the workbench reads for itself. It reads them as
bytes (`triq-tap`), using the list of numbers that the output's runtime gives for its message type.
While an output is open in the list or has a number on a chart, the workbench is one more of its
readers: it takes one of the places the output has for inputs, and it never makes the sender wait.
An output that does wait for its readers (`reliable_ms`) is not read at all. The history of a
number is kept from the moment it is put on a chart.

`graph.rs` (the drawing as plain data: cards, pins, wires and groups),
`draft.rs` (the changes a draft holds, as plain data), `colors.rs` (a colour
per runtime), `history.rs` (what connections and outputs did, as plain data),
`signals.rs` (the numbers inside messages), `charts.rs` (the charts, with the
list of numbers, the lines over time, the curves of numbers against each other,
how those are looked at and what a line is drawn with in `charts/`),
`feed.rs` (the background threads), `app.rs` (the windows), `app/cards.rs`
(how the graph is drawn and handled) and `app/details.rs` (the details
window) are about 5,920 lines together, and `triq-tap` another 170. The graph is drawn with `egui-snarl`, and the
windows and the charts are arranged with `egui_tiles`, by the rules that
`triq-views` has for panes (`panes.rs` there). The tests drive the
real window code without opening a window, with a simulated pointer where it
matters, and write pictures to `target/tmp`: `windows.png`, `workbench.png`,
`graph.png`, `focus.png`, `draft.png`, `group.png`, `group-inside.png`,
`charts.png`, `chart-colours.png`, `curves.png` and `space.png`.

## Programs with their own main loop

A renderer or a device driver keeps its main thread. It can hold ports itself:

```rust
let ports: CameraPorts = rt.attach("camera")?;   // the node lives as long as the ports
ports.image.send_slice(&pixels)?;                // whenever the application likes
```

Or it can offer a factory whose nodes it creates itself, picking up requests in its own loop:

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

Most applications only want the current nodes, by name. `local_nodes` runs
the loop above and keeps the nodes, each with whatever the application adds:

```rust
let mut cameras: LocalNodes<CameraPorts, CameraParams, Shutter> = rt.local_nodes("camera")?;
// in the application's loop:
cameras.serve();   // what other programs asked for since last time
for (name, camera) in &mut cameras.nodes { /* camera.ports, .params, .paused, .state: Shutter */ }
```

A spawn request returns at once with the ports already bound, so nobody waits
for the application's loop. The application reports what only it knows with
`rt.report(node, problem)`, which shows as the node's error, and
`rt.set_factory_info(kind, json)`, which shows beside the factory (the
controllers that are plugged in, for example). See
`crates/triq/examples/app_loop.rs`.

## Views of your own

The workbench shows what is true of any node: the graph, the timing of every
connection, and the numbers inside any message that lists them. It cannot
draw a controller's sticks or a camera's picture, and it does not try.
Drawing those is the job of a **view**, and a view is two ordinary things in
one of the project's own programs:

- a kind of node with an input;
- something that draws what a node of that kind last received.

`triq-views` is the program around them: a runtime, and a window with a list
of the current views and a pane for each. A project writes its kinds of view,
and this:

```rust
fn main() -> triq_views::eframe::Result {
    triq_views::run(Runtime::builder("views"), |views| {
        views.add(ControllerViews::new(views.runtime())?);             // a kind that a crate wrote
        views.add(FrameViews::new(views.runtime(), views.window())?);  // and another
        views.add(Latest::new(views.runtime(), "gauge", degrees)?);    // one that is written right here
        Ok(())
    })
}

/// Everything a view of a small message needs: the message it listens to, and how the newest one is drawn.
fn degrees(ui: &mut egui::Ui, reading: &Reading) {
    ui.label(format!("{:.1} degrees", reading.celsius));
}
```

A complete program like it, with something to look at as soon as it runs, is
`crates/triq-views/examples/template.rs`: a thermometer, a kind of view that
shows what it reads, and one such view already wired. Copy it to start a
views program of your own.

```sh
cargo run -p triq-views --example template
```

`run` starts the runtime it is given, opens the window, and hands `views` to
your function, which tells it the kinds. Each kind offers its nodes on
`views.runtime()`. The runtime is the project's to configure, as in any
program: a project whose views do real work gives the runtime a pool for them,
`Runtime::builder("views").pool("views", 2)`, and each such view is placed in
that pool or on a thread of its own when it is created.

Who owns what:

| | Owner |
| --- | --- |
| What a kind of view listens to, and how it draws | the crate that writes the kind |
| The views (they are nodes), their inputs, the window and how it is arranged | the views program, in its own runtime |
| Saying which kinds of view can be created | that runtime, as every runtime says what it offers |
| Finding the kinds, creating a view, wiring it, pausing and removing it | the workbench, the `triq` tool, or any other program |
| Knowing that a node is a view | nobody: to triq and to the workbench it is a node with an input |

So a new kind of view is written in a crate of the project and handed to its
views program. triq is not touched, and neither is the workbench, which runs
unchanged and lists the new kind as soon as that program runs. Each of
the two windows draws at its own pace, and neither stops when the other does.

What the views program does for every kind:

- Every view is listed as soon as its node exists, and has a pane, which the user arranges like
  the workbench's windows: see [Arranging panes](#arranging-panes). The cross on a pane closes the
  view, not its node. Click the view in the list to open it again, or drag it from the list to
  where it should go.
- The program remembers the size and place of its window and how the views were arranged. Nodes
  are not kept from one run to the next, so a view returns to its old place when a node with its
  name exists again.

A kind of view is anything that says which views there are and draws one:

```rust
pub trait Kind {
    fn names(&mut self) -> Vec<String>;                 // the views of this kind there are now
    fn show(&mut self, ui: &mut egui::Ui, name: &str);  // draw one, in the room it is given
}
```

Who runs the nodes behind the views depends on how much work a message takes:

| | The window runs them | The runtime runs them |
| --- | --- | --- |
| Written as | `Latest::new(runtime, kind, draw)` | a `Kind` of the project's own |
| Offered with | `rt.local_nodes(kind)`, by `Latest` | `rt.register_with(kind, ...)` |
| A message is read | by the window, as it draws | on the thread or pool the node was placed on |
| The window | reads each node's input, then draws | draws what the nodes left for it |
| Right for | small messages: a controller's state | large ones: a camera's frame, scaled down first |
| Example | `triq-xbox/src/viewer.rs` | `triq-bevy/src/viewer.rs` |

A project that already has a window makes its own `Views::new(runtime,
window)` and draws it with `Views::show`. Views are drawn with the egui that
`triq-views` was built with, re-exported as `triq_views::egui`. A crate that
only defines a kind of view should depend on `triq-views` without its default
features; it then builds no window. The crate is about 530 lines.

The workbench is also a widget, `Workbench::show`, for a project that does
want everything in one window. That window is then drawn with the egui the
workbench was built with, re-exported as `triq_workbench::egui`, and all of it
is drawn again whenever any part of it has something new.

## Programs built on it

Three programs use the library the way any project does, each in a repository
of its own. Those repositories are not public yet.

| Repository | What to read it for |
| --- | --- |
| `triq-xbox` | a device as a node; which device is a setting, not a wire; a view of a small message, in one call |
| `triq-bevy` | nodes inside an engine's own loop; large frames with a header; a view whose nodes the runtime runs |
| `triq-demo` | a views program with real things to show: those two kinds of view and one of its own, in 46 lines |

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
A loan holds default values when it is handed over, which for a slice is one
quick pass over the memory. Data that already sits somewhere else is sent
with `send_slice`, which copies it in once without setting anything first.

## Headers

A slice says nothing about itself except its length. Whatever else the receiver
needs goes in a header: a second, small `#[repr(C)]` type that travels in the same
sample, beside the data.

```rust
#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Frame")]
struct Frame { width: u32, height: u32, number: u64 }

#[derive(Ports)]
struct CameraPorts {
    image: Output<[u8], Frame>,          // pixels, and what they are
}

let mut frame = ports.image.loan(width * height * 4)?;
render_into(&mut frame);                 // the pixels, written in place
*frame.header_mut() = Frame { width, height, number };
frame.send()?;

let frame = viewer.image.recv()?.unwrap();   // viewer.image: Input<[u8], Frame>
show(&frame, frame.header().width, frame.header().height);
```

The header is part of the message type, shown as `u8[] + Frame`, so an input
that expects another header, or none, is refused when it is connected. Without
a header, the size would have to be packed into the first bytes of the pixels, by a
convention each side has to know and nothing checks.

## What a message holds

iceoryx2 knows a message type by its name, size and alignment, and nothing of
what is inside. A tool that was not built with the type could show only bytes.
So every message type derives `Fields`, and lists its numbers: what each is
called, where it begins, how it is stored.

```rust
#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
struct Reading { valid: bool, pose: Pose, at: f64 }
// valid, pose.translation[0] ... pose.rotation[3], at
```

A member that is a struct lists its numbers under the member's name. An array
lists its numbers by index, up to the first 64 of a long array. A type whose
numbers cannot be listed, such as an enum, says so in one line:
`impl Fields for Mode {}`.

None of this travels with the messages. The runtime that owns an output
answers for it: `Remote::layout("imu", "pose")` returns the list, for the
message and for its header, and `Field::read` takes a number out of the bytes
of a message.

## Where the code is

| File | Lines | What it holds |
| --- | ---: | --- |
| `message.rs` | 220 | message types, headers, the stamp on every sample |
| `fields.rs` | 132 | the numbers a message lists, and reading one from bytes |
| `port.rs`, `port/output.rs`, `port/input.rs` | 754 | `Output`, `Input`, `Sample`, `Loan`, port declarations |
| `node.rs` | 328 | the `Node` trait, `Trigger`, `Placement` |
| `executor.rs` | 221 | the thread that waits and runs nodes |
| `executor/workers.rs` | 57 | the optional dispatcher behind shared pools |
| `runtime.rs` | 692 | `Runtime`: spawn, factories, connect, inventory |
| `runtime/local.rs` | 176 | factories whose nodes the application runs |
| `runtime/group.rs` | 133 | which group a node is in, and kinds that create a group of nodes |
| `control.rs`, `remote.rs` | 532 | requests between programs, discovery, change announcements |
| `inventory.rs`, `error.rs`, `raw.rs`, `lib.rs` | 425 | report types, the error type, iceoryx2 type names |

About 3,670 lines including comments, plus the derive macros (108) and the command-line tool (185).
Tests are in `crates/triq/tests`, one file per subject.

## Measurements

These numbers come from the benchmark program in this repository,
`crates/triq-bench`, which is not a crate to depend on. It has two workloads.
`hop` is one node sending to one node in another process. `mesh` is pairs of
nodes sending to each other, in pools of threads that are either all in one
process or each in a process of its own; a message that takes longer than
200 µs counts as late. Each row is a single run on a desktop that was not idle
(Apple M5, macOS 27, release build, AC power). Latency is measured from just
before a node hands a message to its output until the receiving node has it.

| Workload | Result |
| --- | --- |
| Two processes, 128 bytes at 10 kHz, 30 s | p50 5, p99 8, max 105 µs; 0 lost |
| 30 nodes at 5 kHz, one process, 3 pools of 10, 60 s | p50 7, p99 26, max 439 µs; 0 lost; 9 late |
| Same, 5 pools of 6 | p50 7, p99 24, max 126 µs; 0 lost; 0 late |
| Same, 3 processes of 10 threads | 0 lost in two runs. One: p50 7, p99 22, max 1,958 µs, 317 late. The other, which generated only 98.7% of its nominal rate: p50 7, p99 47, max 33,798 µs, 21,041 late |
| Same, 5 processes of 6 threads | p50 7, p99 19, max 212 µs; 0 lost; 2 late |

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
- Linux and Windows: see [Platforms](#platforms).
- Requests are served one at a time, so a slow `Node::new` delays other requests to that runtime.

## Build and test

```sh
cargo test -p triq                # the library alone
cargo test --workspace            # with the workbench and the tap
cargo clippy --workspace --all-targets -- -D warnings
cargo run --example pipeline      # one process
cargo run --example station -- left   # then see the top of examples/station.rs
```

In this repository triq builds on the iceoryx2 fork, at the commit that the
workspace `Cargo.toml` names; see [Platforms](#platforms).

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
