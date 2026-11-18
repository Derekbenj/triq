//! The numbers inside messages over time: a topic that is watched is read as
//! bytes, and the numbers its type lists are taken out of every message.
#[path = "support/scene.rs"]
mod scene;
use scene::*;
use std::collections::{BTreeMap, BTreeSet};
use triq::prelude::*;
use triq_workbench::signals::{Signals, Wanted};

#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Frame")]
struct Frame {
    width: u32,
    height: u32,
}

#[derive(Ports)]
struct CameraPorts {
    image: Output<[u8], Frame>,
    #[port(reliable_ms = 20)]
    sure: Output<u64>,
}

/// Watch these topics, and keep the past of these numbers of each.
fn wanted<const N: usize>(topics: [(&str, &[&str]); N]) -> Wanted {
    let numbers = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<BTreeSet<_>>();
    topics.into_iter().map(|(topic, names)| (topic.to_owned(), numbers(names))).collect::<BTreeMap<_, _>>()
}

fn pose(x: f32) -> Pose {
    Pose { translation: [x, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0] }
}

#[test]
fn the_numbers_inside_a_topic_are_kept_while_it_is_watched() -> triq::Result {
    let scene = Scene::new()?;
    let sensor = format!("{}/imu/pose", scene.robot.name());
    let signals = Signals::start()?;

    // Every topic on the machine is listed, with what iceoryx2 knows of it.
    assert!(wait_for(|| signals.topics().iter().any(|topic| topic.name == sensor)), "the topic is listed");
    let listed = signals.topics().into_iter().find(|topic| topic.name == sensor).unwrap();
    assert_eq!((listed.message.as_str(), listed.slice), ("Pose", false));
    assert!(signals.watched(&sensor).is_none(), "nothing is read that nobody looks at");
    assert_eq!(scene.imu.pose.subscribers(), 1, "only the filter");

    // Watched, it says which numbers it holds; they have no value until a message comes.
    signals.watch(wanted([(&sensor, &[])]));
    assert!(wait_for(|| signals.watched(&sensor).is_some()));
    let watched = signals.watched(&sensor).unwrap();
    let names: Vec<&str> = watched.numbers.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names[..2], ["translation[0]", "translation[1]"]);
    assert_eq!((names.len(), watched.problem, watched.numbers[0].1), (7, None, None));
    assert_eq!(scene.imu.pose.subscribers(), 2, "the workbench reads it as well");

    scene.imu.pose.send(pose(1.5))?;
    assert!(wait_for(|| signals.latest(&sensor, "translation[0]") == Some(1.5)), "the newest value is at hand");
    assert_eq!(signals.latest(&sensor, "rotation[3]"), Some(1.0));
    assert!(signals.line(&sensor, "translation[0]", 0.0, f64::MAX).is_empty(), "its past is kept only for a chart");

    // With a number on a chart, every message leaves a point: when it was sent, and the value.
    signals.watch(wanted([(&sensor, &["translation[0]"])]));
    assert!(wait_for(|| {
        // The tap may have been asked for the past before it heard of the chart: send until it keeps something.
        scene.imu.pose.send(pose(0.0)).expect("send");
        !signals.line(&sensor, "translation[0]", 0.0, f64::MAX).is_empty()
    }));
    // Wait for the last of those to have come through, so that what follows is all that follows.
    scene.imu.pose.send(pose(99.0))?;
    let newest = || signals.line(&sensor, "translation[0]", 0.0, f64::MAX).last().map(|(_, value)| *value);
    assert!(wait_for(|| newest() == Some(99.0)));
    let before = signals.line(&sensor, "translation[0]", 0.0, f64::MAX).len();
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    for x in [1.0, 2.0, 3.0] {
        scene.imu.pose.send(pose(x))?;
    }
    assert!(wait_for(|| signals.line(&sensor, "translation[0]", 0.0, f64::MAX).len() == before + 3));
    let line = signals.line(&sensor, "translation[0]", 0.0, f64::MAX);
    let newest = &line[before..];
    assert_eq!(newest.iter().map(|(_, value)| *value).collect::<Vec<_>>(), [1.0, 2.0, 3.0], "none is skipped");
    assert!(newest.windows(2).all(|pair| pair[0].0 < pair[1].0) && (newest[0].0 - started).abs() < 1.0);
    // Only what is asked for is given: the points from a time on.
    assert_eq!(signals.line(&sensor, "translation[0]", newest[1].0, f64::MAX), newest[1..]);
    assert!(signals.line(&sensor, "translation[1]", 0.0, f64::MAX).is_empty(), "the others are not on a chart");

    // No longer watched, it is no longer read.
    signals.watch(Wanted::new());
    assert!(wait_for(|| signals.watched(&sensor).is_none() && scene.imu.pose.subscribers() == 1));
    Ok(())
}

#[test]
fn a_topic_that_is_not_there_yet_is_read_once_it_is() -> triq::Result {
    // A chart may remember a number of a program that is not running: the name of its topic is all there is.
    let robot = unique("late");
    let sensor = format!("{robot}/imu/pose");
    let signals = Signals::start()?;
    signals.watch(wanted([(&sensor, &["translation[0]"])]));
    assert!(wait_for(|| signals.watched(&sensor).is_some_and(|watched| watched.problem.is_some())));
    assert!(
        signals.watched(&sensor).unwrap().numbers.is_empty() && signals.latest(&sensor, "translation[0]").is_none()
    );

    // The program starts. Nobody asks again: the topic is found, and read from then on.
    let runtime = Runtime::new(&robot)?;
    let imu: ImuPorts = runtime.attach("imu")?;
    let read = wait_for(|| {
        imu.pose.send(pose(4.5)).expect("send");
        signals.latest(&sensor, "translation[0]") == Some(4.5)
    });
    assert!(read, "the number is read");
    assert!(signals.watched(&sensor).unwrap().problem.is_none() && imu.pose.subscribers() == 1);
    assert!(!signals.line(&sensor, "translation[0]", 0.0, f64::MAX).is_empty(), "and its past is kept for the chart");
    Ok(())
}

#[test]
fn a_slice_says_how_long_it_is_and_a_header_what_it_holds() -> triq::Result {
    let scene = Scene::new()?;
    let camera: CameraPorts = scene.robot.attach("camera")?;
    let (image, sure) = (format!("{}/camera/image", scene.robot.name()), format!("{}/camera/sure", scene.robot.name()));
    let signals = Signals::start()?;
    signals.watch(wanted([(&image, &[]), (&sure, &[])]));
    assert!(wait_for(|| signals.watched(&image).is_some() && signals.watched(&sure).is_some()));

    let names = |topic: &str| -> Vec<String> {
        signals.watched(topic).unwrap().numbers.into_iter().map(|(name, _)| name).collect()
    };
    // How many elements, the first of them, and what travels beside them.
    assert_eq!(names(&image), ["length", "[0]", "header.width", "header.height"]);
    camera.image.send_slice_with(&[9, 8, 7, 6, 5, 4], Frame { width: 3, height: 2 })?;
    assert!(wait_for(|| signals.latest(&image, "length") == Some(6.0)));
    let read = ["header.width", "header.height", "[0]"].map(|name| signals.latest(&image, name));
    assert_eq!(read, [Some(3.0), Some(2.0), Some(9.0)]);

    // An output that waits for its readers is left alone, and says why.
    let refused = signals.watched(&sure).unwrap();
    assert!(refused.problem.is_some_and(|problem| problem.contains("waits for its readers")));
    assert_eq!((refused.numbers.len(), camera.sure.subscribers()), (0, 0));
    Ok(())
}

#[test]
fn a_topic_of_another_program_is_read_for_what_can_be_known_of_it()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use iceoryx2::prelude::{NodeBuilder, ipc};
    // Plain iceoryx2: nobody to ask what is inside, and no stamp beside the messages.
    let (plain, sliced) = (unique("plain"), unique("sliced"));
    let node = NodeBuilder::new().create::<ipc::Service>()?;
    let numbers = node.service_builder(&plain.as_str().try_into()?).publish_subscribe::<u64>().open_or_create()?;
    let slices = node.service_builder(&sliced.as_str().try_into()?).publish_subscribe::<[u16]>().open_or_create()?;
    let _number = numbers.publisher_builder().create()?;
    let slice = slices.publisher_builder().initial_max_slice_len(8).create()?;

    let signals = Signals::start()?;
    assert!(wait_for(|| signals.topics().iter().any(|topic| topic.name == plain)), "it is listed like any other");
    signals.watch(wanted([(&plain, &[]), (&sliced, &["length"])]));
    assert!(wait_for(|| signals.watched(&plain).is_some() && signals.watched(&sliced).is_some()));
    assert_eq!(signals.watched(&plain).unwrap(), Default::default(), "no numbers, and nothing wrong");

    // How long a slice is can always be told. Its messages are placed in time when they are read.
    let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs_f64();
    slice.loan_slice_uninit(3)?.write_from_slice(&[1, 2, 3]).send()?;
    assert!(wait_for(|| signals.latest(&sliced, "length") == Some(3.0)));
    let line = signals.line(&sliced, "length", 0.0, f64::MAX);
    assert!(line.len() == 1 && line[0].0 >= before && line[0].0 < before + 3.0);
    Ok(())
}

#[test]
fn what_was_read_is_let_go_of_by_the_time_reading_stops() -> triq::Result {
    let scene = Scene::new()?;
    let sensor = format!("{}/imu/pose", scene.robot.name());
    let signals = Signals::start()?;
    signals.watch(wanted([(&sensor, &[])]));
    assert!(wait_for(|| scene.imu.pose.subscribers() == 2), "the filter, and the workbench");

    // When the workbench closes, its program ends at once. Whatever is still being read then is
    // left on the machine for another program to clear away, so nothing is.
    drop(signals);
    assert_eq!(scene.imu.pose.subscribers(), 1, "only the filter again, as soon as the reading is dropped");
    Ok(())
}
