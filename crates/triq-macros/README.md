# triq-macros

The derive macros of [triq](https://crates.io/crates/triq).

You do not need to depend on this crate. Depend on `triq`, which brings both
macros with it in `triq::prelude`.

- `#[derive(Ports)]` turns a struct of `Input<T>` and `Output<T>` fields into
  a port declaration. The field name is the port name, and `#[port(...)]` sets
  options.
- `#[derive(Fields)]` makes a message type list the numbers it holds, so that
  tools can show them.

The [triq README](https://github.com/Derekbenj/triq#readme) shows both in use.

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
