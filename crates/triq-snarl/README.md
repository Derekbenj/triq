# triq-snarl

Node graphs for [egui](https://github.com/emilk/egui): what triq's workbench
draws its graph with.

It is [egui-snarl](https://github.com/zakarumych/egui-snarl) 0.12.0 by
Zakarum, with changes. It is not egui-snarl, and its author has not reviewed
them. For a node graph in a program of your own, egui-snarl is the place to
start.

## What is changed

| From | What |
| --- | --- |
| Rafael Amador | egui 0.37, where egui-snarl 0.12.0 is egui 0.36 |
| triq | a wire can have its own colour, and a pin snaps to where it is dropped |
| triq | the node in focus, and the menu of a node, behave as the workbench needs |
| triq | a click on empty canvas lets go of the selection, and a wire can glow |
| triq | the header of a node can fade through several colours |
| here | [egui-scale](https://github.com/zakarumych/egui-scale) by Zakarum, as Rafael Amador ported it to egui 0.37, is a module of this crate (`src/egui_scale.rs`): it has no release for egui 0.37, and a crate on crates.io may only depend on crates that are there |
| here | the optional `egui-probe` support is left out, for the same reason |

The changes were made on a fork of egui-snarl,
[Derekbenj/egui-snarl](https://github.com/Derekbenj/egui-snarl), branch
`triq-egui-0.37`, up to commit `b2d48da`. That branch is where the egui 0.37
port can be offered back to egui-snarl.

The code keeps egui-snarl's formatting, rustfmt's defaults (`rustfmt.toml`
here), so that it stays easy to compare with egui-snarl.

## Licence

MIT OR Apache-2.0, as egui-snarl and egui-scale are. `LICENSE-MIT` and
`LICENSE-APACHE` are egui-snarl's.
