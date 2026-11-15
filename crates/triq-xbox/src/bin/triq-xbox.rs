//! `triq-xbox [runtime name]`: offers the controllers plugged into this machine as nodes.
//!
//! Create a node of kind `xbox` with settings `{"device": id}`, using an id
//! the factory advertises, and wire its `state` output wherever it is needed.
use std::time::{Duration, Instant};
use triq::Runtime;
use triq_xbox::{Host, SdlPads};

/// How often controllers are read and published: 200 times a second.
const PERIOD: Duration = Duration::from_millis(5);

fn main() -> triq::Result {
    let name = std::env::args().nth(1).unwrap_or_else(|| "xbox".into());
    let mut host = Host::new(Runtime::new(&name)?, SdlPads::new()?)?;
    eprintln!("{name}: offering controllers as `xbox` nodes. Try `triq list`.");
    while host.pads_mut().pump() && !host.runtime().stopping() {
        let started = Instant::now();
        host.step()?;
        std::thread::sleep(PERIOD.saturating_sub(started.elapsed()));
    }
    Ok(())
}
