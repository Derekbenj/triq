//! Data moving between ports that an application holds itself.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use std::time::{Duration, Instant};
use triq::prelude::*;

#[derive(Ports)]
struct Source {
    out: Output<Pose>,
}

#[derive(Ports)]
struct Sink {
    input: Input<Pose>,
}

#[test]
fn a_sample_sent_on_an_output_arrives_on_a_connected_input() -> triq::Result {
    let rt = Runtime::new(&unique("basic"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    assert_eq!(source.out.send(pose(1.0))?, 0, "nobody is connected yet");
    assert!(sink.input.recv()?.is_none());

    rt.connect("source/out", "sink/input")?;
    assert_eq!(source.out.subscribers(), 1);
    assert_eq!(source.out.send(pose(2.0))?, 1);
    let sample = sink.input.recv()?.expect("a sample");
    assert_eq!(*sample, pose(2.0));
    assert_eq!(sample.stamp().seq, 2);
    assert_eq!(sample.source(), format!("{}/source/out", rt.name()));
    assert!(sink.input.recv()?.is_none());
    Ok(())
}

#[test]
fn fan_out_delivers_one_sample_to_every_input() -> triq::Result {
    let rt = Runtime::new(&unique("fanout"))?;
    let source: Source = rt.attach("source")?;
    let sinks: Vec<Sink> = (0..3).map(|i| rt.attach(&format!("sink{i}"))).collect::<triq::Result<_>>()?;
    for i in 0..3 {
        rt.connect("source/out", &format!("sink{i}/input"))?;
    }
    assert_eq!(source.out.send(pose(7.0))?, 3);
    for sink in &sinks {
        assert_eq!(*sink.input.recv()?.expect("a sample"), pose(7.0));
    }
    Ok(())
}

#[test]
fn fan_in_merges_several_outputs_into_one_input() -> triq::Result {
    let rt = Runtime::new(&unique("fanin"))?;
    let left: Source = rt.attach("left")?;
    let right: Source = rt.attach("right")?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("left/out", "sink/input")?;
    rt.connect("right/out", "sink/input")?;
    left.out.send(pose(1.0))?;
    right.out.send(pose(2.0))?;
    left.out.send(pose(3.0))?;

    let mut seen = Vec::new();
    while let Some(sample) = sink.input.recv()? {
        seen.push((sample.source().rsplit('/').nth(1).unwrap().to_owned(), sample.translation[0]));
    }
    seen.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(seen, [("left".into(), 1.0), ("right".into(), 2.0), ("left".into(), 3.0)]);
    Ok(())
}

#[test]
fn ports_connect_across_runtimes() -> triq::Result {
    let producer = Runtime::new(&unique("producer"))?;
    let consumer = Runtime::new(&unique("consumer"))?;
    let source: Source = producer.attach("source")?;
    let sink: Sink = consumer.attach("sink")?;
    consumer.connect(&format!("{}/source/out", producer.name()), "sink/input")?;
    source.out.send(pose(4.0))?;
    assert_eq!(*sink.input.recv()?.expect("a sample"), pose(4.0));
    Ok(())
}

#[derive(Ports)]
struct SmallQueue {
    #[port(queue = 2)]
    out: Output<u64>,
}

#[derive(Ports)]
struct Numbers {
    input: Input<u64>,
}

#[test]
fn a_full_queue_drops_its_oldest_sample_and_the_input_counts_the_loss() -> triq::Result {
    let rt = Runtime::new(&unique("latest"))?;
    let source: SmallQueue = rt.attach("source")?;
    let sink: Numbers = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    source.out.send(1)?;
    assert_eq!(*sink.input.recv()?.unwrap(), 1);
    for value in 2..=6 {
        source.out.send(value)?; // never waits
    }
    assert_eq!(*sink.input.recv()?.unwrap(), 5);
    assert_eq!(*sink.input.recv()?.unwrap(), 6);
    assert!(sink.input.recv()?.is_none());
    assert_eq!(sink.input.lost(), 3);
    Ok(())
}

#[test]
fn a_connection_reports_the_longest_gap_between_its_samples_of_late() -> triq::Result {
    let rt = Runtime::new(&unique("gap"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    let gap = || {
        let inventory = rt.inventory();
        let sink = inventory.nodes.iter().find(|node| node.name == "sink").expect("the sink");
        Duration::from_micros(sink.inputs[0].sources[0].gap_us)
    };
    // Send one sample after a pause, and take it.
    let send_after = |pause: Duration| -> triq::Result {
        std::thread::sleep(pause);
        source.out.send(pose(0.0))?;
        assert!(sink.input.recv()?.is_some());
        Ok(())
    };

    send_after(Duration::ZERO)?;
    assert_eq!(gap(), Duration::ZERO, "one sample has nothing before it");

    // Samples a few milliseconds apart, then a hole.
    let (step, hole) = (Duration::from_millis(5), Duration::from_millis(200));
    for _ in 0..5 {
        send_after(step)?;
    }
    assert!(gap() >= step && gap() < hole, "{:?}", gap());
    send_after(hole)?;
    let seen = Instant::now();
    assert!(gap() >= hole, "the hole is the longest gap: {:?}", gap());

    // It is reported for at least half a second, so that a tool looking a few
    // times a second sees it however many tools are looking, and for no more
    // than a second. After that it is history.
    while seen.elapsed() < Duration::from_millis(400) {
        send_after(step)?;
    }
    assert!(gap() >= hole, "still reported: {:?}", gap());
    while seen.elapsed() < Duration::from_millis(1100) {
        send_after(step)?;
    }
    assert!(gap() >= step && gap() < hole, "{:?}", gap());
    Ok(())
}

#[test]
fn latest_skips_to_the_newest_sample() -> triq::Result {
    let rt = Runtime::new(&unique("skip"))?;
    let source: SmallQueue = rt.attach("source")?;
    let sink: Numbers = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    assert!(sink.input.latest()?.is_none());
    source.out.send(1)?;
    source.out.send(2)?;
    assert_eq!(*sink.input.latest()?.unwrap(), 2);
    assert!(sink.input.latest()?.is_none());
    Ok(())
}

#[derive(Ports)]
struct Reliable {
    #[port(queue = 1, reliable_ms = 40)]
    out: Output<u64>,
}

#[test]
fn a_reliable_output_waits_for_a_full_input_then_reports_the_failure() -> triq::Result {
    let rt = Runtime::new(&unique("reliable"))?;
    let source: Reliable = rt.attach("source")?;
    let sink: Numbers = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    assert_eq!(source.out.send(1)?, 1);

    let started = Instant::now();
    assert_eq!(source.out.send(2), Err(Error::Undelivered));
    assert!(started.elapsed() >= Duration::from_millis(35), "it waited: {:?}", started.elapsed());

    assert_eq!(*sink.input.recv()?.unwrap(), 1, "the queued sample was not overwritten");
    assert!(sink.input.recv()?.is_none());
    assert_eq!(source.out.send(3)?, 1);
    assert_eq!(*sink.input.recv()?.unwrap(), 3);
    Ok(())
}

#[derive(Ports)]
struct Camera {
    image: Output<[u8]>,
}

#[derive(Ports)]
struct Viewer {
    image: Input<[u8]>,
}

#[test]
fn slices_need_no_declared_size() -> triq::Result {
    let rt = Runtime::new(&unique("slices"))?;
    let camera: Camera = rt.attach("camera")?;
    let viewer: Viewer = rt.attach("viewer")?;
    rt.connect("camera/image", "viewer/image")?;
    for len in [16usize, 4 * 1024 * 1024, 3] {
        let frame: Vec<u8> = (0..len).map(|i| i as u8).collect();
        camera.image.send_slice(&frame)?;
        let sample = viewer.image.recv()?.expect("a frame");
        assert_eq!(sample.len(), len);
        assert!(*sample == frame[..]);
    }
    Ok(())
}

#[test]
fn a_loan_is_filled_in_place() -> triq::Result {
    let rt = Runtime::new(&unique("loan"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    let camera: Camera = rt.attach("camera")?;
    let viewer: Viewer = rt.attach("viewer")?;
    rt.connect("source/out", "sink/input")?;
    rt.connect("camera/image", "viewer/image")?;

    let mut loan = source.out.loan()?;
    loan.translation = [1.0, 2.0, 3.0];
    assert_eq!(loan.send()?, 1);
    assert_eq!(sink.input.recv()?.unwrap().translation, [1.0, 2.0, 3.0]);

    let mut frame = camera.image.loan(4)?;
    frame.copy_from_slice(&[9, 8, 7, 6]);
    frame.send()?;
    assert_eq!(*viewer.image.recv()?.unwrap(), [9, 8, 7, 6]);
    Ok(())
}

#[test]
fn an_input_connected_too_early_links_once_the_output_appears() -> triq::Result {
    let rt = Runtime::new(&unique("early"))?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("later/out", "sink/input")?;
    let waiting = &rt.inventory().nodes[0].inputs[0].sources[0];
    assert_eq!(waiting.output, format!("{}/later/out", rt.name()));
    assert!(!waiting.connected);

    let source: Source = rt.attach("later")?;
    assert!(wait_for(|| source.out.subscribers() == 1));
    source.out.send(pose(5.0))?;
    assert_eq!(*sink.input.recv()?.unwrap(), pose(5.0));
    Ok(())
}

#[test]
fn ports_of_different_types_cannot_be_connected() -> triq::Result {
    let rt = Runtime::new(&unique("types"))?;
    let _source: Source = rt.attach("source")?;
    let _sink: Numbers = rt.attach("sink")?;
    assert!(matches!(rt.connect("source/out", "sink/input"), Err(Error::Mismatch(_))));
    let inventory = rt.inventory();
    let sink = inventory.nodes.iter().find(|node| node.name == "sink").unwrap();
    assert!(sink.inputs[0].sources.is_empty(), "nothing was recorded");
    Ok(())
}

#[test]
fn disconnect_stops_delivery() -> triq::Result {
    let rt = Runtime::new(&unique("disconnect"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    rt.connect("source/out", "sink/input")?; // connecting twice changes nothing
    assert_eq!(source.out.send(pose(1.0))?, 1);
    assert!(sink.input.recv()?.is_some());

    rt.disconnect("source/out", "sink/input")?;
    assert_eq!(source.out.send(pose(2.0))?, 0);
    assert!(sink.input.recv()?.is_none());
    rt.disconnect("source/out", "sink/input")?; // and so does disconnecting twice
    Ok(())
}

#[test]
fn names_are_checked() -> triq::Result {
    assert!(matches!(Runtime::new("has/slash"), Err(Error::Name(_))));
    assert!(matches!(Runtime::new("_reserved"), Err(Error::Name(_))));
    assert!(matches!(Runtime::new(""), Err(Error::Name(_))));

    let name = unique("names");
    let rt = Runtime::new(&name)?;
    assert!(matches!(Runtime::new(&name), Err(Error::Name(_))), "a runtime name is unique on the machine");

    let _source: Source = rt.attach("source")?;
    assert!(matches!(rt.attach::<Source>("source"), Err(Error::Name(_))));
    assert!(matches!(rt.attach::<Source>("bad name"), Err(Error::Name(_))));
    assert!(matches!(rt.connect("source/out", "nobody/input"), Err(Error::Unknown(_))));
    assert!(matches!(rt.connect("source/out", "source/out"), Err(Error::Unknown(_))));
    Ok(())
}

#[derive(Ports)]
#[allow(dead_code)]
struct WrongOption {
    #[port(depth = 1)]
    out: Output<u64>,
}

#[test]
fn an_option_that_does_not_fit_the_port_is_refused() -> triq::Result {
    let rt = Runtime::new(&unique("option"))?;
    assert!(matches!(rt.attach::<WrongOption>("node"), Err(Error::Mismatch(_))));
    assert!(rt.inventory().nodes.is_empty());
    Ok(())
}

#[test]
fn a_node_lives_as_long_as_its_ports() -> triq::Result {
    let rt = Runtime::new(&unique("lifetime"))?;
    let source: Source = rt.attach("source")?;
    assert_eq!(rt.inventory().nodes.len(), 1);
    drop(source);
    assert!(rt.inventory().nodes.is_empty());
    let _again: Source = rt.attach("source")?;
    Ok(())
}

#[test]
fn the_inventory_reports_ports_connections_and_counters() -> triq::Result {
    let rt = Runtime::new(&unique("inventory"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    source.out.send(pose(1.0))?;
    sink.input.recv()?;

    let inventory = rt.inventory();
    assert_eq!(inventory.runtime, rt.name());
    assert_eq!(inventory.pid, std::process::id());
    let [sink_info, source_info] = &inventory.nodes[..] else { panic!("two nodes, sorted by name") };
    assert_eq!((sink_info.name.as_str(), source_info.name.as_str()), ("sink", "source"));

    let output = &source_info.outputs[0];
    assert_eq!(output.port, Source::spec()[0]);
    assert_eq!(output.service, format!("{}/source/out", rt.name()));
    assert_eq!((output.subscribers, output.sent), (1, 1));

    let input = &sink_info.inputs[0];
    assert_eq!(input.port, Sink::spec()[0]);
    let link = &input.sources[0];
    assert_eq!(link.output, output.service);
    assert!(link.connected);
    assert_eq!((link.received, link.lost), (1, 0));
    Ok(())
}

#[test]
fn asking_about_an_input_does_not_hold_up_its_reader() -> triq::Result {
    let rt = Runtime::new(&unique("aside"))?;
    let source: Source = rt.attach("source")?;
    let sink: Sink = rt.attach("sink")?;
    rt.connect("source/out", "sink/input")?;
    rt.connect("missing/out", "sink/input")?; // keeps the runtime retrying in the background

    let stop = std::sync::atomic::AtomicBool::new(false);
    let slow = std::thread::scope(|scope| -> triq::Result<usize> {
        // Another program asking for the inventory over and over.
        scope.spawn(|| {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                rt.inventory();
            }
        });
        let mut slow = 0;
        for _ in 0..20_000 {
            source.out.send(pose(1.0))?;
            let asked = Instant::now();
            sink.input.recv()?;
            slow += usize::from(asked.elapsed() > Duration::from_micros(50));
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(slow)
    })?;
    assert!(slow < 400, "{slow} of 20000 reads were held up");
    Ok(())
}

/// What a frame is, beside its pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend)]
#[repr(C)]
#[type_name("FrameInfo")]
struct FrameInfo {
    width: u32,
    height: u32,
    frame: u64,
}

#[derive(Ports)]
struct HeadedCamera {
    image: Output<[u8], FrameInfo>,
    exposure: Output<f32, FrameInfo>,
}

#[derive(Ports)]
struct HeadedViewer {
    image: Input<[u8], FrameInfo>,
    exposure: Input<f32, FrameInfo>,
}

#[test]
fn a_message_can_carry_a_typed_header_beside_its_data() -> triq::Result {
    let rt = Runtime::new(&unique("header"))?;
    let camera: HeadedCamera = rt.attach("camera")?;
    let viewer: HeadedViewer = rt.attach("viewer")?;
    rt.connect("camera/image", "viewer/image")?;
    rt.connect("camera/exposure", "viewer/exposure")?;

    // Filled in place: the pixels and what they are travel together.
    let mut frame = camera.image.loan(6)?;
    *frame.header_mut() = FrameInfo { width: 3, height: 2, frame: 7 };
    frame.copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    frame.send()?;
    let sample = viewer.image.recv()?.expect("a frame");
    assert_eq!(*sample.header(), FrameInfo { width: 3, height: 2, frame: 7 });
    assert_eq!(*sample, [1, 2, 3, 4, 5, 6]);
    assert_eq!(sample.stamp().seq, 1, "the stamp is still there");

    // Copied in, with and without saying what the header is.
    camera.image.send_slice_with(&[9, 9], FrameInfo { width: 2, height: 1, frame: 8 })?;
    assert_eq!(viewer.image.recv()?.unwrap().header().frame, 8);
    camera.image.send_slice(&[0])?;
    assert_eq!(*viewer.image.recv()?.unwrap().header(), FrameInfo::default());
    camera.exposure.send_with(0.5, FrameInfo { frame: 9, ..Default::default() })?;
    let exposure = viewer.exposure.recv()?.unwrap();
    assert_eq!((*exposure, exposure.header().frame), (0.5, 9));
    Ok(())
}

#[test]
fn a_header_is_part_of_the_message_type() -> triq::Result {
    assert_eq!(HeadedCamera::spec()[0].message.header, "FrameInfo");
    assert_eq!(Camera::spec()[0].message.header, "", "no header unless one is named");
    assert_ne!(HeadedCamera::spec()[0].message, Camera::spec()[0].message);
    assert_eq!(HeadedCamera::spec()[0].message.to_string(), "u8[] + FrameInfo");
    assert_eq!(HeadedCamera::spec()[1].message.to_string(), "f32 + FrameInfo");

    let rt = Runtime::new(&unique("headers"))?;
    let _camera: HeadedCamera = rt.attach("camera")?;
    let _plain: Viewer = rt.attach("plain")?;
    assert!(matches!(rt.connect("camera/image", "plain/image"), Err(Error::Mismatch(_))));
    Ok(())
}
