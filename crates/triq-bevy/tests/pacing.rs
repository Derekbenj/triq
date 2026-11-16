//! Which of the frames a renderer hands it a camera sends. No engine involved.
use std::time::{Duration, Instant};
use triq_bevy::{Pacing, next_frame};

/// A frame every `frame`, each early or late by a different amount of up to `wobble`.
fn frames(start: Instant, count: u32, frame: Duration, wobble: Duration) -> Vec<Instant> {
    let offset = |number: u32| wobble.mul_f64(f64::from(number.wrapping_mul(2_654_435_761) % 1000) / 500.0);
    (1..=count).map(|number| start + frame * number + offset(number) - wobble).collect()
}

/// The frames that are sent, of those given.
fn sent(pacing: &mut Pacing, frames: &[Instant], hz: f32, frame: Duration) -> Vec<Instant> {
    frames.iter().copied().filter(|ready| pacing.take(*ready, hz, frame)).collect()
}

#[test]
fn a_camera_asked_for_every_frame_sends_every_frame() {
    let start = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 120.0);
    // Frames are never ready exactly on time: here up to three milliseconds early or late.
    let ready = frames(start, 1200, frame, Duration::from_millis(3));
    let mut pacing = Pacing::new(start);
    assert_eq!(sent(&mut pacing, &ready, 120.0, frame).len(), 1200, "none is dropped for being a little early");

    // Now and then one comes a whole frame late, just before the next, as when
    // the GPU hands two back together; or two come late, and three together.
    // All are sent, and so are those after.
    let mut bunched = frames(start, 1200, frame, Duration::from_millis(2));
    for late in (40..1190).step_by(40) {
        bunched[late] = bunched[late + 1] - Duration::from_micros(800);
        if late % 80 == 0 {
            (bunched[late], bunched[late + 1]) = (bunched[late + 2] - frame / 5, bunched[late + 2] - frame / 10);
        }
    }
    let mut pacing = Pacing::new(start);
    assert_eq!(sent(&mut pacing, &bunched, 120.0, frame).len(), 1200);

    // Asked for more than there are frames, it sends what there is.
    let slow = Duration::from_secs_f64(1.0 / 60.0);
    let ready = frames(start, 600, slow, Duration::from_millis(3));
    let mut pacing = Pacing::new(start);
    assert_eq!(sent(&mut pacing, &ready, 120.0, slow).len(), 600);
}

#[test]
fn a_camera_asked_for_fewer_sends_them_evenly() {
    let start = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 120.0);
    let ready = frames(start, 1200, frame, Duration::from_millis(3));
    let mut pacing = Pacing::new(start);
    let pictures = sent(&mut pacing, &ready, 30.0, frame);
    // Ten seconds at thirty a second, chosen from frames 8.3 ms apart.
    assert!((299..=301).contains(&pictures.len()), "{} pictures", pictures.len());
    let period = Duration::from_secs_f64(1.0 / 30.0);
    let apart: Vec<Duration> = pictures.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let (least, most) = (apart.iter().min().unwrap(), apart.iter().max().unwrap());
    assert!(*least >= period - frame * 2 && *most <= period + frame * 2, "between {least:?} and {most:?} apart");
}

#[test]
fn a_camera_does_not_make_up_for_frames_it_never_had() {
    let start = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 120.0);
    let mut pacing = Pacing::new(start);
    let mut ready = frames(start, 120, frame, Duration::ZERO);
    // Nothing for two seconds, then frames again.
    let resumed = *ready.last().unwrap() + Duration::from_secs(2);
    ready.extend(frames(resumed, 120, frame, Duration::ZERO));
    let pictures = sent(&mut pacing, &ready, 30.0, frame);
    // Two seconds' worth at thirty a second, and one at each start.
    assert!((60..=62).contains(&pictures.len()), "{} pictures", pictures.len());
    let after: Vec<Duration> =
        pictures.windows(2).filter(|pair| pair[0] > resumed).map(|pair| pair[1] - pair[0]).collect();
    assert!(after.iter().all(|gap| *gap >= Duration::from_millis(24)), "no burst to catch up: {after:?}");
}

#[test]
fn a_program_without_a_window_keeps_to_its_frame_rate() {
    let start = Instant::now();
    let frame = Duration::from_millis(10);
    // However long a frame took, the next is due a frame after this one was due:
    // a hundred frames are due a second after the first, not a second and a hundred delays.
    let mut due = start;
    for took in (0..100).map(|number| Duration::from_micros(500 + 70 * number)) {
        due = next_frame(due, frame, due + took);
    }
    assert_eq!(due, start + Duration::from_secs(1));
    // A frame that overran is not made up for with a rush of frames: the count starts again.
    let late = due + Duration::from_millis(35);
    assert_eq!(next_frame(due, frame, late), late);
}
