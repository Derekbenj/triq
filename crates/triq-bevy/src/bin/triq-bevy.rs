//! `triq-bevy [runtime name] [--headless]`: a Bevy scene that offers `rig` and `camera` nodes.
//!
//! Create a rig and wire a controller's `state` to its `controller` input, and
//! the rig moves. Create cameras and wire the rig's `mounts` to each camera's
//! `mounts` input, and they ride on the rig and send what they see.
fn main() -> triq::Result {
    let (mut name, mut headless) = ("bevy".to_owned(), false);
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--headless" => headless = true,
            other => name = other.to_owned(),
        }
    }
    triq_bevy::app(&name, headless)?.run();
    Ok(())
}
