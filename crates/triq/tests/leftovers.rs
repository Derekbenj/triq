//! What a program leaves on the machine when it ends, and what triq makes of
//! what another program left.
//!
//! iceoryx2 keeps a small record of every program that uses it, and of who
//! holds each service. A program that ends without closing what it holds
//! leaves its record behind as a dead one, until another program clears it
//! away. These tests ask iceoryx2 itself what is there.
#[path = "support/helpers.rs"]
mod helpers;
use helpers::*;
use iceoryx2::node::{Node, NodeState, NodeView};
use iceoryx2::prelude::*;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use triq::{Remote, Runtime};

type Svc = iceoryx2::service::ipc_threadsafe::Service;

/// The tests count what is on the machine, so they take turns.
fn turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(PoisonError::into_inner)
}

fn nodes() -> Vec<NodeState<Svc>> {
    let mut nodes = Vec::new();
    Node::<Svc>::list(Config::global_config(), |node| {
        nodes.push(node);
        CallbackProgression::Continue
    })
    .expect("list the programs iceoryx2 knows of");
    nodes
}

/// The dead records of programs that were started as `executable`.
fn dead(executable: &str) -> Vec<u128> {
    let named = |view: &dyn NodeView| view.details().as_ref().is_some_and(|it| it.executable() == executable);
    nodes()
        .iter()
        .filter_map(|node| match node {
            NodeState::Dead(view) if named(view) => Some(view.id().value()),
            _ => None,
        })
        .collect()
}

/// How many records iceoryx2 has of this very program.
fn mine() -> usize {
    let me = std::process::id();
    let is_mine = |view: &dyn NodeView| view.id().pid::<Svc>().is_ok_and(|pid| pid.value() as u32 == me);
    nodes().iter().filter(|node| matches!(node, NodeState::Alive(view) if is_mine(view))).count()
}

/// Whether a running program holds the service of that name. `None` if there is no such service.
fn held(name: &str) -> Option<bool> {
    let mut held = None;
    Svc::list(Config::global_config(), |service| {
        if service.static_details.name().as_str() == name {
            let nodes = service.dynamic_details.iter().flat_map(|dynamic| &dynamic.nodes);
            held = Some(nodes.into_iter().any(|node| matches!(node, NodeState::Alive(_))));
        }
        CallbackProgression::Continue
    })
    .expect("list services");
    held
}

#[test]
fn the_tool_leaves_nothing_of_itself_behind() {
    let _turn = turn();
    let before = dead("triq");
    let done = Command::new(env!("CARGO_BIN_EXE_triq")).arg("list").stdout(Stdio::null()).status().expect("run triq");
    assert!(done.success());
    let left: Vec<u128> = dead("triq").into_iter().filter(|record| !before.contains(record)).collect();
    assert!(left.is_empty(), "`triq list` ended and left a dead record of itself: {left:?}");
}

#[test]
fn a_runtime_that_was_killed_is_let_go_of_by_everyone_who_looked() -> triq::Result {
    let _turn = turn();
    let name = unique("gone");
    let control = format!("{name}/_control");
    let sees = |name: &str| triq::discover().expect("discover").iter().any(|inventory| inventory.runtime == name);
    // Two parts of this program look at the machine, each keeping what it has opened: this
    // thread and another, as a tool and a workbench would.
    let (ask, asked) = mpsc::channel::<()>();
    let (say, said) = mpsc::channel::<bool>();
    let onlooker = std::thread::spawn({
        let name = name.clone();
        move || asked.iter().for_each(|()| say.send(sees(&name)).unwrap())
    });
    let other_sees = || ask.send(()).is_ok() && said.recv().unwrap();

    let mut program = station(&name);
    // Starting a program can take seconds on a machine that is busy with something else.
    let announced = (0..5).any(|_| wait_for(|| sees(&name)));
    assert!(announced && other_sees(), "the program announces itself to both");

    // Killed, it closes nothing: its control service stays behind, and both onlookers hold it.
    program.0.kill().unwrap();
    program.0.wait().unwrap();
    assert_eq!(held(&control), Some(true));
    // Neither lists it any more. And neither keeps hold of it because the other still does:
    // then it would stay for as long as both look at the machine.
    for _ in 0..2 {
        assert!(!sees(&name) && !other_sees(), "a runtime that is gone is not listed");
    }
    assert_eq!(held(&control), None, "what the dead program left is cleared away");
    drop(ask);
    onlooker.join().unwrap();
    Ok(())
}

#[test]
fn a_program_that_lets_go_has_nothing_open() -> triq::Result {
    let _turn = turn();
    let name = unique("kept");
    let runtime = Runtime::new(&name)?;
    let own = mine();

    // Opening a runtime, even its own, gives the program one more record: the one requests go out from.
    let remote = Remote::open(&name)?;
    assert_eq!(remote.inventory()?.runtime, name);
    assert_eq!(mine(), own + 1);
    // The thread keeps the runtime open after the handle is gone, to ask it again cheaply.
    drop(remote);
    assert_eq!(mine(), own + 1);

    // Let go, nothing is left of that; and the next request makes it anew.
    triq::release();
    assert_eq!(mine(), own);
    assert_eq!(Remote::open(&name)?.inventory()?.runtime, name);
    assert_eq!(mine(), own + 1);
    triq::release();
    drop(runtime);
    Ok(())
}
