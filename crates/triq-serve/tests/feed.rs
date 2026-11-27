//! The page's own look at the machine, and the one thing it asks of a runtime: to shut down.
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_serve::feed::Feed;

/// Wait until `done` holds; false when three seconds pass first.
fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

#[test]
fn the_feed_sees_the_runtimes_of_the_machine_and_one_it_is_asked_to_shut_down_is_told_to() -> triq::Result {
    let name = format!("srv{}-feed", std::process::id());
    let runtime = Runtime::new(&name)?;
    let feed = Feed::start();
    let seen = |feed: &Feed| feed.machine().into_iter().find(|inventory| inventory.runtime == name);
    assert!(wait_for(|| seen(&feed).is_some()), "the runtime is seen");
    assert_eq!(seen(&feed).unwrap().pid, std::process::id());

    // What cannot be done is said, once, in words that say what was asked.
    assert_eq!(feed.said(), None);
    feed.shut_down("srv-nobody-is-called-this");
    let mut said = None;
    assert!(wait_for(|| {
        said = said.take().or_else(|| feed.said());
        said.is_some()
    }));
    let said = said.unwrap();
    assert!(said.contains("shut down srv-nobody-is-called-this") && feed.said().is_none(), "{said}");

    feed.shut_down(&name);
    assert!(wait_for(|| runtime.stopping()), "the runtime was asked to stop");
    Ok(())
}
