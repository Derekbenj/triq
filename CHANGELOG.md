# Changes

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
