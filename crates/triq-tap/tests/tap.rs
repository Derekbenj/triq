//! Any topic on the machine can be listed and read as bytes, by a program
//! that was built without its message type.
use iceoryx2::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use triq::prelude::*;
use triq::{Remote, Stamp};
use triq_tap::Taps;

#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Pose")]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Frame")]
struct Frame {
    width: u32,
    height: u32,
    number: u64,
}

#[derive(Ports)]
struct RobotPorts {
    pose: Output<Pose>,
    image: Output<[u8], Frame>,
    counts: Output<[u32]>,
    #[port(reliable_ms = 20)]
    sure: Output<u64>,
}

/// A runtime of its own for every test, with one node that has each kind of output.
fn robot() -> triq::Result<(Runtime, RobotPorts)> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!("tap{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let runtime = Runtime::new(&name)?;
    let ports = runtime.attach("robot")?;
    Ok((runtime, ports))
}

/// A test fails on an error of triq, of the tap or of iceoryx2.
type Checked = std::result::Result<(), Box<dyn std::error::Error>>;

/// The bytes beside a message and the bytes of the message, as a tap shows them.
type Bytes = (Vec<u8>, Vec<u8>);

fn copied(beside: &[u8], message: &[u8]) -> Bytes {
    (beside.to_vec(), message.to_vec())
}

#[test]
fn every_topic_is_listed_with_what_iceoryx2_knows_of_it() -> Checked {
    let (runtime, _ports) = robot()?;
    let taps = Taps::new()?;
    let topics = taps.topics()?;
    let topic = |port: &str| {
        let name = format!("{}/robot/{port}", runtime.name());
        topics.iter().find(|topic| topic.name == name).unwrap_or_else(|| panic!("{name} is listed"))
    };

    let pose = topic("pose");
    assert_eq!((pose.message.as_str(), pose.size, pose.slice, pose.waits), ("Pose", 28, false, false));
    let image = topic("image");
    assert_eq!((image.message.as_str(), image.size, image.slice), ("u8", 1, true));
    // What travels beside a message: for a triq output its stamp, and then the port's header.
    assert_eq!((pose.beside, image.beside), (16, 32));
    assert!(topic("sure").waits, "its sender waits for a reader that falls behind");

    assert!(topics.windows(2).all(|pair| pair[0].name < pair[1].name), "sorted by name");
    // Only what messages are published on: not the services runtimes are asked things through.
    assert!(!topics.iter().any(|topic| topic.name.ends_with("/_control")));
    Ok(())
}

#[test]
fn a_tap_shows_each_message_as_bytes() -> Checked {
    let (runtime, ports) = robot()?;
    let taps = Taps::new()?;
    let name = |port: &str| format!("{}/robot/{port}", runtime.name());

    // A message of fixed size: its bytes are the value as it lies in memory.
    let pose = taps.open(&name("pose"))?;
    assert_eq!(ports.pose.subscribers(), 1, "a tap is one more reader");
    assert_eq!(pose.take(copied)?, None, "nothing was sent yet");
    ports.pose.send(Pose { translation: [1.5, -2.0, 3.25], rotation: [0.0, 0.0, 0.0, 1.0] })?;
    let (beside, message) = pose.take(copied)?.expect("a message");
    let numbers: Vec<u8> = [1.5f32, -2.0, 3.25, 0.0, 0.0, 0.0, 1.0].iter().flat_map(|x| x.to_ne_bytes()).collect();
    assert_eq!(message, numbers);
    // Beside it is the stamp triq put there.
    let stamp = Stamp::read(&beside).expect("a stamp");
    assert!(stamp.seq == 1 && stamp.sent_ns > 0);
    assert_eq!(pose.take(copied)?, None);

    // The numbers its type lists are read out of those bytes.
    let layout = Remote::open(runtime.name())?.layout("robot", "pose")?;
    let read: Vec<f64> = layout.payload.iter().filter_map(|field| field.read(&message)).collect();
    assert_eq!(read, [1.5, -2.0, 3.25, 0.0, 0.0, 0.0, 1.0]);

    // A slice: as many bytes as it has elements, times the size of one. The header is beside it.
    let image = taps.open(&name("image"))?;
    ports.image.send_slice_with(&[1, 2, 3, 4, 5], Frame { width: 5, height: 1, number: 9 })?;
    let (beside, message) = image.take(copied)?.expect("a frame");
    assert_eq!(message, [1, 2, 3, 4, 5]);
    let layout = Remote::open(runtime.name())?.layout("robot", "image")?;
    let header: Vec<f64> = layout.header.iter().filter_map(|field| field.read(&beside)).collect();
    assert_eq!(header, [5.0, 1.0, 9.0]);
    let counts = taps.open(&name("counts"))?;
    ports.counts.send_slice(&[7, 8, 9])?;
    let (_, message) = counts.take(copied)?.expect("counts");
    assert_eq!(message, [7u32, 8, 9].iter().flat_map(|count| count.to_ne_bytes()).collect::<Vec<u8>>());

    // A tap that is closed is no longer a reader.
    drop(pose);
    assert_eq!(ports.pose.subscribers(), 0);
    Ok(())
}

#[test]
fn a_tap_never_holds_up_a_sender() -> Checked {
    let (runtime, ports) = robot()?;
    let taps = Taps::new()?;
    let pose = taps.open(&format!("{}/robot/pose", runtime.name()))?;

    // A hundred messages while the tap reads none: each is sent at once, and the tap finds the newest.
    for number in 1..=100 {
        assert_eq!(ports.pose.send(Pose { translation: [number as f32, 0.0, 0.0], ..Pose::default() })?, 1);
    }
    let mut seen = Vec::new();
    while let Some(seq) = pose.take(|beside, _| Stamp::read(beside).map(|stamp| stamp.seq))? {
        seen.extend(seq);
    }
    assert_eq!(seen, (69..=100).collect::<Vec<u64>>(), "the 32 that fit in its queue");

    // An output that waits for its readers is not tapped: a tap that fell behind would stall it.
    let refused = taps.open(&format!("{}/robot/sure", runtime.name())).err().expect("refused");
    assert!(refused.to_string().contains("waits for its readers"), "{refused}");
    assert_eq!(ports.sure.subscribers(), 0);
    // And a topic nobody offers cannot be opened; a tap never creates one.
    assert!(taps.open(&format!("{}/robot/nothing", runtime.name())).is_err());
    assert!(!taps.topics()?.iter().any(|topic| topic.name.ends_with("/robot/nothing")));
    Ok(())
}

#[test]
fn a_topic_that_is_not_triqs_is_read_the_same_way() -> Checked {
    // Plain iceoryx2: a service, a publisher, a number.
    let name = format!("tap{}/plain", std::process::id());
    let node = NodeBuilder::new().create::<ipc::Service>()?;
    let service = node.service_builder(&name.as_str().try_into()?).publish_subscribe::<u64>().open_or_create()?;
    let publisher = service.publisher_builder().create()?;

    let taps = Taps::new()?;
    let listed = taps.topics()?.into_iter().find(|topic| topic.name == name).expect("it is listed");
    assert_eq!((listed.message.as_str(), listed.size, listed.slice, listed.beside), ("u64", 8, false, 0));
    let tap = taps.open(&name)?;
    publisher.send_copy(42)?;
    assert_eq!(tap.take(copied)?, Some((Vec::new(), 42u64.to_ne_bytes().to_vec())));
    Ok(())
}
