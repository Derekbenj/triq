//! `triq-bevy [runtime name] [--headless] [--fps N]`: a Bevy scene that offers `rig` and `camera` nodes.
//!
//! Create a rig and wire a controller's `state` to its `controller` input, and
//! the rig moves. Create cameras and wire the rig's `mounts` to each camera's
//! `mounts` input, and they ride on the rig and send what they see.
//!
//! `--fps` is how many frames a second the program runs when it keeps time
//! itself: without a window, or with its window not in front. Sixty unless
//! told otherwise. No camera sends more pictures than that.
fn main() -> triq::Result {
    let (mut name, mut headless, mut fps) = ("bevy".to_owned(), false, 60.0);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--headless" => headless = true,
            "--fps" => {
                let given = arguments.next().and_then(|number| number.parse().ok());
                fps = given.ok_or_else(|| triq::Error::Other("--fps needs a number, such as --fps 120".into()))?;
            }
            other => name = other.to_owned(),
        }
    }
    triq_bevy::app(&name, headless, fps)?.run();
    Ok(())
}
