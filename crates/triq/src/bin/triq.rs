//! `triq`: look at the runtimes on this machine and rewire them.
//!
//! The tool owns no nodes. Every change is a request to the runtime that
//! owns the node concerned.
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;
use std::time::Duration;
use triq::{Error, Inventory, NodeInfo, NodeState, Placement, PortSpec, Remote, Watch};

const USAGE: &str = "\
usage:
  triq list                                   runtimes, factories, nodes and connections
  triq json                                   the same, as JSON
  triq watch                                  the list, redrawn whenever something changes
  triq spawn <runtime> <kind> <name> [--pool <pool>] [--params <json>]
  triq connect <runtime/node/output> <runtime/node/input>
  triq disconnect <runtime/node/output> <runtime/node/input>
  triq fields <runtime/node/output>           the numbers inside what an output sends
  triq set <runtime/node> <json>              change a node's parameters
  triq pause <runtime/node>
  triq resume <runtime/node>
  triq remove <runtime/node>
  triq shutdown <runtime>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("triq: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> triq::Result {
    match args {
        ["list"] => triq::discover()?.iter().for_each(|inventory| print!("{}", describe(inventory))),
        ["json"] => println!("{}", serde_json::to_string_pretty(&triq::discover()?)?),
        ["watch"] => watch()?,
        ["spawn", runtime, kind, name, options @ ..] => {
            let (placement, params) = spawn_options(options)?;
            Remote::open(runtime)?.spawn(kind, name, placement, params)?;
        }
        ["connect", from, to] => triq::connect(from, to)?,
        ["disconnect", from, to] => triq::disconnect(from, to)?,
        ["fields", output] => fields(output)?,
        ["set", node, params] => on(node, |remote, node| remote.set_params(node, serde_json::from_str(params)?))?,
        ["pause", node] => on(node, Remote::pause)?,
        ["resume", node] => on(node, Remote::resume)?,
        ["remove", node] => on(node, Remote::remove)?,
        ["shutdown", runtime] => Remote::open(runtime)?.shutdown()?,
        _ => return Err(Error::Other(USAGE.into())),
    }
    Ok(())
}

/// Redraw the list when a runtime says it changed, and once a second for the counters.
fn watch() -> triq::Result {
    let watch = Watch::new()?;
    loop {
        let listing: String = triq::discover()?.iter().map(describe).collect();
        // Clear the terminal and start at the top.
        print!("\x1b[2J\x1b[H{listing}");
        std::io::stdout().flush()?;
        watch.wait(Duration::from_secs(1))?;
    }
}

/// The numbers inside the messages of an output and inside its header: how
/// each is stored, where it begins, and what it is called.
fn fields(output: &str) -> triq::Result {
    let [runtime, node, port] = output.split('/').collect::<Vec<_>>()[..] else {
        return Err(Error::Name(format!("{output:?} is not `runtime/node/output`")));
    };
    let layout = Remote::open(runtime)?.layout(node, port)?;
    for (part, fields) in [("message", layout.payload), ("header", layout.header)] {
        for field in fields {
            println!("{part:8} {:5} at byte {:<4} {}", format!("{:?}", field.kind), field.at, field.name);
        }
    }
    Ok(())
}

/// Apply a request to `runtime/node`.
fn on(name: &str, request: impl FnOnce(&Remote, &str) -> triq::Result) -> triq::Result {
    let (runtime, node) = name.split_once('/').ok_or_else(|| Error::Name(format!("{name:?} is not `runtime/node`")))?;
    request(&Remote::open(runtime)?, node)
}

fn spawn_options(options: &[&str]) -> triq::Result<(Placement, Value)> {
    let (mut placement, mut params) = (Placement::Dedicated, Value::Null);
    for pair in options.chunks(2) {
        match pair {
            ["--pool", pool] => placement = Placement::pool(pool),
            ["--params", json] => params = serde_json::from_str(json)?,
            _ => return Err(Error::Other(USAGE.into())),
        }
    }
    Ok((placement, params))
}

fn describe(inventory: &Inventory) -> String {
    let mut text = format!("{} (pid {})\n", inventory.runtime, inventory.pid);
    for pool in &inventory.pools {
        let kind = if pool.shared { ", shared" } else { "" };
        _ = writeln!(text, "  pool {}: {} threads{kind}", pool.name, pool.threads);
    }
    for factory in &inventory.factories {
        let ports: Vec<String> = factory.ports.iter().map(port).collect();
        let local = if factory.local { ", run by the application" } else { "" };
        _ = writeln!(text, "  factory {}: {}; params {}{local}", factory.kind, ports.join(", "), factory.params);
    }
    for node in &inventory.nodes {
        text.push_str(&describe_node(node));
    }
    text
}

fn describe_node(node: &NodeInfo) -> String {
    let placement = match &node.placement {
        None => "run by the application".to_owned(),
        Some(Placement::Dedicated) => "own thread".to_owned(),
        Some(Placement::Pool(pool)) => format!("pool {pool}"),
    };
    let state = match &node.state {
        NodeState::Running => "running".to_owned(),
        NodeState::Paused => "paused".to_owned(),
        NodeState::Stopped(panic) => format!("stopped by a panic: {panic}"),
    };
    let kind = if node.kind.is_empty() { String::new() } else { format!("{}, ", node.kind) };
    let mut text = format!("  node {}: {kind}{placement}, {state}, {} runs", node.name, node.runs);
    if let Some(error) = &node.last_error {
        _ = write!(text, ", {} errors, last: {error}", node.errors);
    }
    text.push('\n');
    for input in &node.inputs {
        _ = writeln!(text, "    {}", port(&input.port));
        for source in &input.sources {
            let state = match (&source.problem, source.connected) {
                (Some(problem), _) => problem.as_str(),
                (None, true) => "connected",
                (None, false) => "waiting for the output",
            };
            _ = writeln!(
                text,
                "      <- {} ({state}, {} received, {} lost, {} us latency, {} us longest gap)",
                source.output, source.received, source.lost, source.latency_us, source.gap_us
            );
        }
    }
    for output in &node.outputs {
        let inputs = if output.subscribers == 1 { "input" } else { "inputs" };
        _ = writeln!(
            text,
            "    {} -> {} {inputs}, {} sent, {} undelivered",
            port(&output.port),
            output.subscribers,
            output.sent,
            output.undelivered
        );
    }
    text
}

fn port(spec: &PortSpec) -> String {
    let direction = match spec.direction {
        triq::Direction::Input => "in ",
        triq::Direction::Output => "out",
    };
    format!("{direction} {}: {}", spec.name, spec.message)
}
