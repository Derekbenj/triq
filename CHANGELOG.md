# Changes

## Not yet released

- **A pane dropped on the name of another is side by side with it.** In the workbench, and in a
  views program, a window, chart or view that is dragged by its name and let go on the name of
  another, on that one's bar, becomes a group with it where the other was, the two side by
  side. On the name of a group it joins the group. Dropped at an end of a name, it is one more
  name on the bar, as before. `triq_views::panes::tidy` takes a `panes::Carried` now, which a
  `Behavior` tells of each name and asks for the highlight.
- **`triq serve`: triq's programs in a browser.** `triq-serve` serves a page to this machine
  alone that starts triq's programs as processes, more than one of each under names of their
  own, shows what they print, and stops them. What the page does a script or a language model
  can ask as well, with the token it prints.
- **A folder of programs is registered once**, with `triq register <folder>`, in a file, and
  its programs are found from then on with nothing on the `PATH`. `triq::programs()` and
  `triq::program(name)` find them for a program of your own.
- **`triq <program>` runs the program `triq-<program>`**: the one beside the tool, or on the
  `PATH`. `triq workbench` starts the workbench, and a program of a project built on triq is run
  the same way. Typed alone, `triq` lists the programs it has found.
- **The workbench's list of numbers is node by node, and is searched.** In `Charts`, what a node
  hears and sends is folded away under the node's name until that is clicked, and a box lists
  what has every word typed in it in its name: a runtime, a node, a port, or part of one. The
  box that chooses a runtime and the search stay at the top as the list scrolls.
- **Some of a node's settings can be changed, and the rest left.** `Runtime::change_params` and
  `Remote::change_params` take the settings to change, by name, and `triq change <runtime/node>
  <json>` does the same from a terminal. `set_params` and `triq set` go on replacing all of
  them, so that a setting left out is its default again. A runtime built with 0.2.0 does not
  know the new request, and says so.
- **The workbench changes the settings that were edited, and leaves the rest.** `Apply changes`
  in a node's details asks for what was typed that differs from what the node had, and says
  which settings those are; `Replace all` asks for all of them as written, as `Apply` did.
  Text that nobody is typing in now keeps up with the node. `triq::merged` is what a change
  makes of settings, for a tool that shows it before asking.
- **A number in a node's settings reads back as exactly the number it is**, for an `f64` and a
  whole number of up to 64 bits. Settings are shown as text; read back from it, a few numbers
  came back one step of rounding away, so that settings sent back as they were shown were not
  always the same settings. This turns on `serde_json`'s `float_roundtrip`, and Cargo turns a
  feature on for every crate of a project that has triq in it: their own JSON is read exactly
  too, a little more slowly.
- **A node's own refusal is no longer taken for an old runtime's.** A value a node did not know
  for a setting that is one of a few names came back from another program as "the runtime was
  built with an earlier triq". It comes back as the node's words.
- **The workbench pauses, starts again and removes a whole group.** The details of a group have
  `Pause all`, `Resume all` and `Remove all`, which ask every node in it, those of the groups
  inside it too, and its list of nodes says which of them do not run.
- **A pool whose threads never sleep.** `Runtime::builder("robot").polling_pool("fast", 1)` is a
  third kind of pool. A node placed in it keeps one of its threads, as in `pool`, and that thread
  looks for work again and again in place of sleeping until some comes: a message to the node
  costs no thread wake-up, 5 µs where a thread that had fallen asleep takes 20 to 50, and each
  thread of the pool keeps a processor core busy. On a machine whose cores are all busy it gains
  nothing. `triq list` and the workbench say which pools poll.
- **`PoolInfo` has a field more**, `polling`. A program that builds one itself has to name it;
  one that reads an inventory goes on as before, and a runtime of 0.2.0 is read as having no
  pool that polls.

## 0.2.0

What a project that used 0.1 has to change:

- **`Node::tick` is told which period elapsed:** `fn tick(&mut self, ports: &mut Self::Ports, period: usize)`.
  A node with one period takes `_: usize` and goes on as before.
- **`Trigger` is no longer `Copy`.** It has a fourth variant, `Trigger::Several`.
- **A connection ends with the node it is to.** In 0.1 an input went on waiting for an output
  by its name after the node behind it was removed, or its program stopped, and heard whatever
  node was given that name next. Now the connection is gone with the node, and is made again by
  whoever wants it.
- **A connection needs both its ports.** Connecting to an output that no node sends on is refused
  with `Error::Unknown`, where 0.1 waited for one to appear.
- **`Member`, one node of a `Group`, has a field more**, `group`. A group is best written with
  `Group::new().spawn(...)`, which fills it in.

What is new:

- **Several periods.** `Trigger::Every(fast).and_every(slow)` calls `tick` on each period,
  timed on its own, with its number. A node has room for 63.
- **Groups.** A node says which group it is in (`Runtime::set_group`, and `Remote::set_group`
  from another program); a group inside another is `outer/inner`. A runtime offers several
  nodes, wired together, as one kind (`Runtime::register_group`), and some of them may be in a
  group inside it (`Group::group`).
- **A node that takes one message a run** is run again at once while more waits for it, and the
  other nodes on its thread have their turn in between.
- **A program that looked at other runtimes** (`Remote`, `discover`) leaves nothing of itself
  behind when it ends.
- **The workbench is on crates.io:** `cargo install triq-workbench`. Since 0.1 it drafts changes
  and applies them together, groups nodes as Blender does, colours each runtime, and charts any
  number over time, two numbers against each other, or three in space.
- **`triq-snarl`**, the graph the workbench draws: egui-snarl, ported to egui 0.37, with the
  changes the workbench needs.
- **On macOS** the iceoryx2 fork is set once per machine, in `~/.cargo/config.toml`; see
  "Platforms" in the README.

`triq-macros`, `triq-views` and `triq-tap` have not changed, and are 0.2.0 so that every crate
of triq has the same version.

## 0.1.0 and 0.1.1

The first releases, on 2026-11-17.
