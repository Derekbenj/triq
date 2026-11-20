//! A connection is between two ports that are there, and ends when either
//! goes: an input lets go of an output whose node was removed or whose program
//! stopped, and does not hear a node that takes the name later.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use std::time::Duration;
use triq::prelude::*;

#[derive(Ports)]
struct Source {
    out: Output<u64>,
}

#[derive(Ports)]
struct Sink {
    input: Input<u64>,
}

/// The outputs that the only input of a node wants to hear, and whether each is there to be heard.
fn wanted(rt: &Runtime, node: &str) -> Vec<(String, bool)> {
    let node = rt.inventory().nodes.into_iter().find(|info| info.name == node).expect("the node");
    node.inputs[0].sources.iter().map(|source| (source.output.clone(), source.connected)).collect()
}

/// Longer than a runtime takes to notice that an output is gone.
fn a_moment() {
    std::thread::sleep(Duration::from_millis(600));
}

#[test]
fn an_input_lets_go_of_an_output_whose_program_stops() -> triq::Result {
    let consumer = Runtime::new(&unique("consumer"))?;
    let sink: Sink = consumer.attach("sink")?;
    let producer_name = unique("producer");
    let output = format!("{producer_name}/source/out");
    // The producing program does not run yet: there is nothing to connect to.
    assert!(matches!(consumer.connect(&output, "sink/input"), Err(Error::Unknown(_))));
    assert!(wanted(&consumer, "sink").is_empty());

    // It runs, and the input is connected to it.
    let producer = Runtime::new(&producer_name)?;
    let source: Source = producer.attach("source")?;
    consumer.connect(&output, "sink/input")?;
    source.out.send(1)?;
    assert_eq!(collect(&sink.input, 1), [1]);

    // The program stops. What the input was connected to is gone, and so is the connection.
    drop((source, producer));
    assert!(wait_for(|| wanted(&consumer, "sink").is_empty()), "the input lets go of what is gone");

    // The program runs again, with a node of the same name. Nothing is connected to it:
    // what it sends reaches nobody.
    let producer = Runtime::new(&producer_name)?;
    let source: Source = producer.attach("source")?;
    a_moment();
    assert_eq!((source.out.subscribers(), source.out.send(2)?), (0, 0));
    assert!(wanted(&consumer, "sink").is_empty() && sink.input.recv()?.is_none());

    // Connected again, it is heard.
    consumer.connect(&output, "sink/input")?;
    source.out.send(3)?;
    assert_eq!(collect(&sink.input, 1), [3]);
    Ok(())
}

#[test]
fn a_node_that_is_removed_takes_its_connections_with_it() -> triq::Result {
    let (here, there) = (Runtime::new(&unique("here"))?, Runtime::new(&unique("there"))?);
    let output = format!("{}/source/out", here.name());
    // Two inputs hear the node: one in its own runtime, and one in another.
    let source: Source = here.attach("source")?;
    let (near, far): (Sink, Sink) = (here.attach("near")?, there.attach("far")?);
    here.connect("source/out", "near/input")?;
    there.connect(&output, "far/input")?;
    assert_eq!((wanted(&here, "near"), wanted(&there, "far")), (vec![(output.clone(), true)], vec![(output, true)]));

    // The application lets go of the node: it is removed.
    drop(source);
    let let_go = || wanted(&here, "near").is_empty() && wanted(&there, "far").is_empty();
    assert!(wait_for(let_go), "both inputs let go of it");

    // A node that takes its name is another node. Nobody is connected to it.
    let source: Source = here.attach("source")?;
    a_moment();
    assert_eq!((source.out.subscribers(), source.out.send(1)?), (0, 0));
    assert!(let_go() && near.input.recv()?.is_none() && far.input.recv()?.is_none());
    Ok(())
}

#[test]
fn a_node_that_takes_a_name_at_once_is_not_heard_for_a_moment_either() -> triq::Result {
    let (here, there) = (Runtime::new(&unique("here"))?, Runtime::new(&unique("there"))?);
    let source: Source = here.attach("source")?;
    let (near, far): (Sink, Sink) = (here.attach("near")?, there.attach("far")?);
    here.connect("source/out", "near/input")?;
    there.connect(&format!("{}/source/out", here.name()), "far/input")?;
    source.out.send(1)?;
    assert_eq!((collect(&near.input, 1), collect(&far.input, 1)), (vec![1], vec![1]));

    // The node is removed, and another takes its name before any runtime has looked and seen
    // that the first is gone. What the new one sends is not for the inputs of the old one.
    drop(source);
    let source: Source = here.attach("source")?;
    for value in 2..20 {
        source.out.send(value)?;
        assert!(near.input.recv()?.is_none() && far.input.recv()?.is_none(), "{value} was heard");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(wanted(&here, "near").is_empty() && wanted(&there, "far").is_empty());
    Ok(())
}

#[test]
fn a_connection_to_a_node_that_took_the_name_of_another_is_a_new_connection() -> triq::Result {
    let (here, there) = (Runtime::new(&unique("here"))?, Runtime::new(&unique("there"))?);
    let output = format!("{}/source/out", here.name());
    let source: Source = here.attach("source")?;
    let far: Sink = there.attach("far")?;
    there.connect(&output, "far/input")?;
    assert_eq!(source.out.subscribers(), 1);

    // A node is replaced as a tool does it: removed, created and connected, one after the other,
    // before the other runtime has looked and seen that the first node is gone.
    drop(source);
    let source: Source = here.attach("source")?;
    there.connect(&output, "far/input")?;
    // The connection that was asked for last is to the new node, and it stays.
    a_moment();
    assert_eq!(wanted(&there, "far"), [(output, true)]);
    assert_eq!(source.out.send(5)?, 1);
    assert_eq!(collect(&far.input, 1), [5]);
    Ok(())
}
