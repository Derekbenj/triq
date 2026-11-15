//! A port struct describes itself without creating anything.
use triq::prelude::*;
use triq::{Direction, MessageType, PortOptions};

#[derive(Debug, Clone, Copy, PartialEq, ZeroCopySend)]
#[repr(C)]
#[type_name("Pose")]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Ports)]
#[allow(dead_code)]
struct FilterPorts {
    raw: Input<Pose>,
    #[port(depth = 1)]
    command: Input<u32>,
    #[port(queue = 4, subscribers = 3, hold = 5)]
    smooth: Output<Pose>,
    #[port(reliable_ms = 50)]
    image: Output<[u8]>,
}

#[test]
fn a_port_struct_lists_its_fields_in_order() {
    let spec = FilterPorts::spec();
    let names: Vec<_> = spec.iter().map(|port| port.name.as_str()).collect();
    assert_eq!(names, ["raw", "command", "smooth", "image"]);
    let directions: Vec<_> = spec.iter().map(|port| port.direction).collect();
    assert_eq!(directions, [Direction::Input, Direction::Input, Direction::Output, Direction::Output]);
}

#[test]
fn a_port_carries_the_message_type_iceoryx2_will_check() {
    let spec = FilterPorts::spec();
    assert_eq!(
        spec[0].message,
        MessageType { name: "Pose".into(), size: 28, align: 4, slice: false, header: String::new() }
    );
    assert_eq!(spec[1].message.name, "u32");
    assert_eq!(
        spec[3].message,
        MessageType { name: "u8".into(), size: 1, align: 1, slice: true, header: String::new() }
    );
    // An output and an input can be wired when their message types are equal.
    assert_eq!(spec[0].message, spec[2].message);
    assert_ne!(spec[0].message, spec[1].message);
}

#[test]
fn port_attributes_become_options() {
    let spec = FilterPorts::spec();
    assert_eq!(spec[0].options, PortOptions::default());
    assert_eq!(spec[1].options, PortOptions { depth: Some(1), ..Default::default() });
    assert_eq!(
        spec[2].options,
        PortOptions { queue: Some(4), subscribers: Some(3), hold: Some(5), ..Default::default() }
    );
    assert_eq!(spec[3].options, PortOptions { reliable_ms: Some(50), ..Default::default() });
}

#[test]
fn a_spec_survives_json() {
    let spec = FilterPorts::spec();
    let json = serde_json::to_string(&spec).unwrap();
    let back: Vec<triq::PortSpec> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, spec);
}
