# triq-tap

Read any [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) topic as
bytes, without having its message type.

A program that subscribes to a topic normally names the Rust type of its
messages, and iceoryx2 checks it. A tool that shows whatever is on the machine
has no such type. `triq-tap` subscribes all the same, by giving the subscriber
the size and alignment that the topic itself declares.

```rust
use triq_tap::Taps;

fn main() -> triq_tap::Result<()> {
    let taps = Taps::new()?;
    for topic in taps.topics()? {
        println!("{} carries {} ({} bytes)", topic.name, topic.message, topic.size);
    }

    let tap = taps.open("robot/imu/pose")?;
    // The bytes that travel beside a message, and the message's own. Both lie
    // in shared memory and are not copied.
    while let Some(length) = tap.take(|_beside, message| message.len())? {
        println!("a message of {length} bytes");
    }
    Ok(())
}
```

- A tap is an ordinary reader with a queue of its own, which loses its oldest
  message when it is full. The sender never waits for a tap.
- A topic whose sender does wait for full readers is not tapped at all.
- A tap never creates a topic; it only reads one that exists.

The [triq](https://github.com/Derekbenj/triq) workbench uses this crate to
show the numbers inside messages, but it works with the topics of any iceoryx2
program. It is the one crate of triq that uses `unsafe`: in three places, each
with the reason it is sound beside it.

On macOS, set the iceoryx2 fork once per machine, as
["Platforms"](https://github.com/Derekbenj/triq#platforms) in the triq README
describes.

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
