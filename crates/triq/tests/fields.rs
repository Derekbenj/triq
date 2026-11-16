//! A message says which numbers it holds, so that a tool can read them from
//! its bytes without knowing the type.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use triq::prelude::*;
use triq::{Field, Remote, Scalar, Stamp};

#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Reading")]
struct Reading {
    valid: bool,
    count: u16,
    pose: Pose,
    at: f64,
}

#[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
#[repr(C)]
#[type_name("Frame")]
struct Frame {
    width: u32,
    height: u32,
    number: u64,
}

/// A message whose numbers cannot be listed says so in one line.
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[repr(C)]
struct Opaque {
    raw: [u8; 3],
}

impl Fields for Opaque {}

#[derive(Ports)]
#[allow(dead_code)]
struct CameraPorts {
    image: Output<[u8], Frame>,
    reading: Output<Reading>,
    opaque: Output<Opaque>,
}

/// The numbers of a message of type `T`.
fn listed<T: Fields>() -> Vec<Field> {
    let mut found = Vec::new();
    T::fields("", 0, &mut found);
    found
}

fn names(fields: &[Field]) -> Vec<&str> {
    fields.iter().map(|field| field.name.as_str()).collect()
}

#[test]
fn a_message_lists_its_numbers_by_name_place_and_kind() {
    let fields = listed::<Reading>();
    assert_eq!(
        names(&fields),
        [
            "valid",
            "count",
            "pose.translation[0]",
            "pose.translation[1]",
            "pose.translation[2]",
            "pose.rotation[0]",
            "pose.rotation[1]",
            "pose.rotation[2]",
            "pose.rotation[3]",
            "at"
        ]
    );
    // Where each begins is where `#[repr(C)]` puts it.
    assert_eq!(fields[0], Field { name: "valid".into(), at: 0, kind: Scalar::Bool });
    assert_eq!((fields[1].at, fields[1].kind), (2, Scalar::U16));
    assert_eq!((fields[2].at, fields[3].at, fields[2].kind), (4, 8, Scalar::F32));
    assert_eq!((fields[9].at, fields[9].kind), (32, Scalar::F64));

    // A bare number is called `value`, and the elements of a bare array by their place in it.
    assert_eq!(listed::<u64>(), [Field { name: "value".into(), at: 0, kind: Scalar::U64 }]);
    let pair = listed::<[i16; 2]>();
    assert_eq!((names(&pair), pair[1].at), (vec!["[0]", "[1]"], 2));
    assert!(listed::<Opaque>().is_empty() && listed::<()>().is_empty());
}

#[test]
fn a_long_array_lists_only_its_first_numbers() {
    // A picture inside a message is not a thousand numbers to choose from.
    let long = listed::<[u8; 1000]>();
    assert_eq!((long.len(), long[63].name.as_str(), long[63].at), (64, "[63]", 63));
}

#[test]
fn a_number_is_read_from_the_bytes_of_a_message() {
    // A pose as it lies in memory: seven `f32`, one after the other.
    let pose = [1.5f32, -2.0, 3.25, 0.0, 0.0, 0.0, 1.0];
    let bytes: Vec<u8> = pose.iter().flat_map(|number| number.to_ne_bytes()).collect();
    let read: Vec<Option<f64>> = listed::<Pose>().iter().map(|field| field.read(&bytes)).collect();
    assert_eq!(read, pose.map(|number| Some(f64::from(number))));
    // Bytes that end before the number does hold no number.
    assert_eq!(listed::<Pose>()[1].read(&bytes[..6]), None);

    // Every kind of number, two bytes into a message.
    let kinds = [
        (Scalar::Bool, vec![1], 1.0),
        (Scalar::U8, vec![200], 200.0),
        (Scalar::I8, (-3i8).to_ne_bytes().to_vec(), -3.0),
        (Scalar::U16, 60_000u16.to_ne_bytes().to_vec(), 60_000.0),
        (Scalar::I16, (-300i16).to_ne_bytes().to_vec(), -300.0),
        (Scalar::U32, 4_000_000_000u32.to_ne_bytes().to_vec(), 4e9),
        (Scalar::I32, (-70_000i32).to_ne_bytes().to_vec(), -70_000.0),
        (Scalar::U64, (1u64 << 40).to_ne_bytes().to_vec(), 1_099_511_627_776.0),
        (Scalar::I64, (-5i64).to_ne_bytes().to_vec(), -5.0),
        (Scalar::F32, 0.25f32.to_ne_bytes().to_vec(), 0.25),
        (Scalar::F64, (-1e-9f64).to_ne_bytes().to_vec(), -1e-9),
    ];
    for (kind, number, value) in kinds {
        let message = [vec![0xAA, 0xAA], number].concat();
        assert_eq!(Field { name: "x".into(), at: 2, kind }.read(&message), Some(value), "{kind:?}");
    }
}

#[test]
fn any_program_can_ask_what_an_output_carries() -> triq::Result {
    let runtime = Runtime::new(&unique("fields"))?;
    let _camera: CameraPorts = runtime.attach("camera")?;
    let remote = Remote::open(runtime.name())?;

    // For a slice, the numbers of one element. The header travels after triq's own stamp.
    let image = remote.layout("camera", "image")?;
    assert_eq!(image.payload, [Field { name: "value".into(), at: 0, kind: Scalar::U8 }]);
    assert_eq!(names(&image.header), ["width", "height", "number"]);
    assert_eq!(image.header.iter().map(|field| field.at).collect::<Vec<_>>(), [16, 20, 24]);

    let reading = remote.layout("camera", "reading")?;
    assert_eq!((reading.payload, reading.header), (listed::<Reading>(), Vec::new()));
    assert!(matches!(remote.layout("camera", "nothing"), Err(Error::Unknown(_))));
    assert!(matches!(remote.layout("nobody", "image"), Err(Error::Unknown(_))));
    Ok(())
}

#[test]
fn the_stamp_is_read_from_the_start_of_what_travels_beside_a_message() {
    let beside = [7u64.to_ne_bytes(), 99u64.to_ne_bytes(), 1u64.to_ne_bytes()].concat();
    assert_eq!(Stamp::read(&beside), Some(Stamp { seq: 7, sent_ns: 99 }));
    assert_eq!(Stamp::read(&beside[..12]), None);
}
