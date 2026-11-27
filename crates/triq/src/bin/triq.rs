//! `triq`: look at the runtimes on this machine and rewire them.
//!
//! The tool owns no nodes. Every change is a request to the runtime that
//! owns the node concerned.
//!
//! It is also the way to the other programs of triq. `triq <program>` runs
//! the program called `triq-<program>` that is beside the tool, or on the
//! PATH: `triq workbench` is `triq-workbench`.
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
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
  triq set <runtime/node> <json>              give a node new parameters, in place of all it has
  triq change <runtime/node> <json>           change the parameters named, and leave the rest
  triq group <runtime/node> [<group>]         put a node in a group, or in none
  triq pause <runtime/node>
  triq resume <runtime/node>
  triq remove <runtime/node>
  triq shutdown <runtime>
  triq <program> [what it is told]            run triq-<program>, as triq workbench runs triq-workbench";

/// What the tool does itself. A program is never run in the place of one of these.
const COMMANDS: [&str; 14] = [
    "list",
    "json",
    "watch",
    "spawn",
    "connect",
    "disconnect",
    "fields",
    "set",
    "change",
    "group",
    "pause",
    "resume",
    "remove",
    "shutdown",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = run(&args);
    // What this thread opened is closed, so that the tool leaves nothing of itself behind.
    triq::release();
    match done {
        Ok(ended) => ended,
        Err(error) => {
            eprintln!("triq: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> triq::Result<ExitCode> {
    match args {
        // Asked for nothing, or for what it can be told: that is said, and nothing has gone wrong.
        [] | ["help" | "--help" | "-h"] => println!("{}", usage()),
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
        ["change", node, changes] => {
            on(node, |remote, node| remote.change_params(node, serde_json::from_str(changes)?))?
        }
        ["group", node, group] => on(node, |remote, node| remote.set_group(node, group))?,
        ["group", node] => on(node, |remote, node| remote.set_group(node, ""))?,
        ["pause", node] => on(node, Remote::pause)?,
        ["resume", node] => on(node, Remote::resume)?,
        ["remove", node] => on(node, Remote::remove)?,
        ["shutdown", runtime] => Remote::open(runtime)?.shutdown()?,
        [program, told @ ..] if !COMMANDS.contains(program) => return hand_over(program, told),
        _ => return Err(Error::Other(usage())),
    }
    Ok(ExitCode::SUCCESS)
}

/// The folders a program of triq is looked for in: the one this tool is in, and then the PATH.
fn folders() -> impl Iterator<Item = PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|tool| Some(tool.parent()?.to_owned()));
    let path: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    beside.into_iter().chain(path)
}

/// Run the program `triq-<name>`, told what the tool was told after its name, and end as it ends.
fn hand_over(name: &str, told: &[&str]) -> triq::Result<ExitCode> {
    let file = format!("triq-{name}{}", std::env::consts::EXE_SUFFIX);
    let Some(program) = folders().map(|folder| folder.join(&file)).find(|program| program.is_file()) else {
        let neither = format!("no command `{name}`, and no program {file} beside triq or on the PATH");
        return Err(Error::Other(format!("{neither}\n{}", usage())));
    };
    let ended = Command::new(program).args(told).status()?;
    // One that a signal ended has no status of its own.
    Ok(ended.code().map_or(ExitCode::FAILURE, |status| ExitCode::from(status as u8)))
}

/// What the tool can be told, and the programs of triq it has found, by the names it runs them by.
fn usage() -> String {
    let mut programs: Vec<String> = Vec::new();
    for folder in folders() {
        let files = std::fs::read_dir(folder).into_iter().flatten().flatten();
        for file in files.filter(|file| file.path().is_file()) {
            let name = file.file_name().to_string_lossy().into_owned();
            let program = name.strip_prefix("triq-").and_then(|name| name.strip_suffix(std::env::consts::EXE_SUFFIX));
            // A file with something after a dot is not a program: `triq-workbench.d` says what one was built from.
            if let Some(program) =
                program.filter(|program| !program.contains('.') && !programs.iter().any(|known| known == program))
            {
                programs.push(program.to_owned());
            }
        }
    }
    programs.sort();
    let found = if programs.is_empty() { "none".to_owned() } else { programs.join(", ") };
    format!("{USAGE}\n\nprograms here: {found}")
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
        let kind = match (pool.shared, pool.polling) {
            (true, _) => ", shared",
            (_, true) => ", polling",
            _ => "",
        };
        _ = writeln!(text, "  pool {}: {} threads{kind}", pool.name, pool.threads);
    }
    for factory in &inventory.factories {
        // A kind that creates a group lists its nodes, each with its own kind.
        if let Some(group) = &factory.group {
            let nodes: Vec<String> = group.nodes.iter().map(|node| format!("{} ({})", node.name, node.kind)).collect();
            _ = writeln!(text, "  factory {}: a group of {}", factory.kind, nodes.join(", "));
            continue;
        }
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
    if !node.group.is_empty() {
        _ = write!(text, ", in group {}", node.group);
    }
    if let Some(error) = &node.last_error {
        _ = write!(text, ", {} errors, last: {error}", node.errors);
    }
    text.push('\n');
    for input in &node.inputs {
        _ = writeln!(text, "    {}", port(&input.port));
        for source in &input.sources {
            _ = writeln!(
                text,
                "      <- {} ({} received, {} lost, {} us latency, {} us longest gap)",
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
