//! Latency and loss under two loads: one hop between two processes, and a mesh of nodes.
//!
//! ```text
//! triq-bench hop  [--rate 10000] [--bytes 128] [--seconds 10] [--loan]
//!     one sending process and one receiving process, each node on its own thread
//!
//! triq-bench mesh [--nodes 30] [--rate 5000] [--bytes 200] [--seconds 20]
//!                 [--groups 3] [--threads 10] [--processes] [--shared] [--dedicated]
//!     pairs of nodes sending to each other. A group is a pool of `threads`
//!     threads; with `--processes` every group is a separate program.
//! ```
//!
//! Latency runs from just before a node hands a message to its output until
//! the receiving node's `run` has it in hand. Only messages sent inside the
//! measured window count; a warm-up precedes it.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use triq::prelude::*;
use triq::{Remote, Result};

const WARMUP: Duration = Duration::from_millis(1500);
const DEADLINE_NS: u32 = 200_000;

#[derive(Ports)]
struct DuplexPorts {
    input: Input<[u8]>,
    #[port(queue = 32, subscribers = 2)]
    output: Output<[u8]>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct DuplexParams {
    /// Zero for a node that only receives.
    period_us: u64,
    bytes: usize,
    /// Fill a loan in place rather than copying a prepared buffer.
    loan: bool,
    /// The measured window, in nanoseconds since the Unix epoch.
    from_ns: u64,
    until_ns: u64,
}

/// Sends a message every period and times the ones it receives.
struct Duplex {
    params: DuplexParams,
    payload: Vec<u8>,
    generated: u64,
    latencies_ns: Vec<u32>,
}

/// Filled as nodes are dropped at shutdown: messages generated, and the latency of each one received.
static RESULTS: Mutex<Vec<(u64, Vec<u32>)>> = Mutex::new(Vec::new());

fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

impl Node for Duplex {
    type Ports = DuplexPorts;
    type Params = DuplexParams;

    fn new(params: DuplexParams) -> Result<Self> {
        let window = params.until_ns.saturating_sub(params.from_ns) as f64 / 1e9;
        let expected = if params.period_us == 0 { 0.0 } else { window * 1e6 / params.period_us as f64 };
        Ok(Self {
            payload: vec![7; params.bytes.max(8)],
            generated: 0,
            // Room for a peer sending at the same rate, so recording never allocates.
            latencies_ns: Vec::with_capacity((expected.max(window * 20_000.0) * 1.2) as usize),
            params,
        })
    }

    fn trigger(&self) -> Trigger {
        match self.params.period_us {
            0 => Trigger::Input,
            period => Trigger::InputOrEvery(Duration::from_micros(period)),
        }
    }

    fn run(&mut self, ports: &mut DuplexPorts) -> Result {
        while let Some(message) = ports.input.recv()? {
            let sent = u64::from_le_bytes(message[..8].try_into().unwrap());
            if (self.params.from_ns..self.params.until_ns).contains(&sent) {
                self.latencies_ns.push(now_ns().saturating_sub(sent).min(u32::MAX as u64) as u32);
            }
        }
        Ok(())
    }

    fn tick(&mut self, ports: &mut DuplexPorts) -> Result {
        if self.params.loan {
            let mut loan = ports.output.loan(self.payload.len())?;
            let sent = now_ns();
            loan[..8].copy_from_slice(&sent.to_le_bytes());
            self.generated += u64::from((self.params.from_ns..self.params.until_ns).contains(&sent));
            loan.send()?;
        } else {
            let sent = now_ns();
            self.payload[..8].copy_from_slice(&sent.to_le_bytes());
            self.generated += u64::from((self.params.from_ns..self.params.until_ns).contains(&sent));
            ports.output.send_slice(&self.payload)?;
        }
        self.run(ports)
    }
}

impl Drop for Duplex {
    fn drop(&mut self) {
        RESULTS.lock().unwrap().push((self.generated, std::mem::take(&mut self.latencies_ns)));
    }
}

/// A program that owns nodes: `worker <runtime> <pools> <threads> <shared> <results file>`.
fn worker(args: &[String]) -> Result {
    let (name, pools, threads, shared, file) =
        (&args[0], args[1].parse::<usize>()?, args[2].parse()?, &args[3], &args[4]);
    let mut builder = Runtime::builder(name);
    for pool in 0..pools {
        let pool = format!("work{pool}");
        builder = if shared == "true" { builder.shared_pool(&pool, threads) } else { builder.pool(&pool, threads) };
    }
    let rt = builder.start()?;
    rt.register::<Duplex>("duplex")?;
    rt.wait();
    let counters = rt.inventory();
    drop(rt);
    let results = std::mem::take(&mut *RESULTS.lock().unwrap());
    let generated: u64 = results.iter().map(|(generated, _)| generated).sum();
    let latencies: Vec<u32> = results.into_iter().flat_map(|(_, latencies)| latencies).collect();
    let lost: u64 =
        counters.nodes.iter().flat_map(|node| &node.inputs).flat_map(|input| &input.sources).map(|s| s.lost).sum();
    std::fs::write(
        file,
        serde_json::to_vec(&json!({ "generated": generated, "latencies_ns": latencies, "lost": lost }))?,
    )?;
    Ok(())
}

struct Options(HashMap<String, String>);

impl Options {
    fn parse(args: &[String]) -> Self {
        let mut options = HashMap::new();
        let mut args = args.iter().peekable();
        while let Some(arg) = args.next() {
            let key = arg.trim_start_matches("--").to_owned();
            let flag = args.peek().is_none_or(|next| next.starts_with("--"));
            options.insert(key, if flag { "true".into() } else { args.next().unwrap().clone() });
        }
        Self(options)
    }
    fn number(&self, key: &str, default: usize) -> usize {
        self.0.get(key).map_or(default, |value| value.parse().expect("a number"))
    }
    fn flag(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

/// Worker programs, stopped and reaped when the benchmark ends.
struct Workers {
    programs: Vec<(String, Child, String)>,
}

impl Workers {
    fn start(count: usize, pools: usize, threads: usize, shared: bool) -> Result<Self> {
        let directory = std::env::temp_dir();
        let programs = (0..count)
            .map(|index| {
                let name = format!("bench{}-{index}", std::process::id());
                let file = directory.join(format!("{name}.json")).to_string_lossy().into_owned();
                let args = [name.clone(), pools.to_string(), threads.to_string(), shared.to_string(), file.clone()];
                let child = Command::new(std::env::current_exe()?).arg("worker").args(args).spawn()?;
                Ok((name, child, file))
            })
            .collect::<Result<Vec<_>>>()?;
        let workers = Self { programs };
        let deadline = Instant::now() + Duration::from_secs(5);
        while workers.programs.iter().any(|(name, ..)| Remote::open(name).is_err()) {
            if Instant::now() > deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(workers)
    }

    /// Stop the workers and merge what their nodes recorded.
    fn finish(mut self) -> Result<(u64, u64, Vec<u32>)> {
        let (mut generated, mut lost, mut latencies) = (0, 0, Vec::new());
        for (name, child, file) in &mut self.programs {
            Remote::open(name)?.shutdown()?;
            child.wait()?;
            let result: serde_json::Value = serde_json::from_slice(&std::fs::read(&*file)?)?;
            let _ = std::fs::remove_file(&*file);
            generated += result["generated"].as_u64().unwrap_or(0);
            lost += result["lost"].as_u64().unwrap_or(0);
            latencies
                .extend(result["latencies_ns"].as_array().into_iter().flatten().map(|v| v.as_u64().unwrap() as u32));
        }
        self.programs.clear();
        Ok((generated, lost, latencies))
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for (_, child, _) in &mut self.programs {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn report(
    label: &str,
    seconds: usize,
    senders: usize,
    rate: usize,
    generated: u64,
    lost: u64,
    mut latencies: Vec<u32>,
) {
    latencies.sort_unstable();
    let at = |quantile: f64| -> f64 {
        if latencies.is_empty() {
            return f64::NAN;
        }
        latencies[((latencies.len() as f64 * quantile).ceil() as usize).clamp(1, latencies.len()) - 1] as f64 / 1000.0
    };
    let received = latencies.len() as u64;
    let on_time = latencies.partition_point(|latency| *latency <= DEADLINE_NS);
    let nominal = (senders * rate * seconds) as f64;
    println!("{label}");
    println!(
        "  generated {generated} ({:.2}% of nominal), received {received}, missing {}, overwritten in a queue {lost}",
        generated as f64 * 100.0 / nominal,
        generated.saturating_sub(received)
    );
    println!(
        "  latency us: p50 {:.1}  p95 {:.1}  p99 {:.1}  p99.9 {:.1}  max {:.1}",
        at(0.5),
        at(0.95),
        at(0.99),
        at(0.999),
        at(1.0)
    );
    println!("  within 200 us: {on_time} ({:.3}% of generated)", on_time as f64 * 100.0 / generated.max(1) as f64);
}

fn hop(options: &Options) -> Result {
    let (rate, bytes, seconds) =
        (options.number("rate", 10_000), options.number("bytes", 128), options.number("seconds", 10));
    let workers = Workers::start(2, 0, 0, false)?;
    let (sender, receiver) = (workers.programs[0].0.clone(), workers.programs[1].0.clone());
    let from_ns = now_ns() + WARMUP.as_nanos() as u64;
    let until_ns = from_ns + seconds as u64 * 1_000_000_000;
    let params = |period_us: usize| json!({ "period_us": period_us, "bytes": bytes, "loan": options.flag("loan"), "from_ns": from_ns, "until_ns": until_ns });
    Remote::open(&receiver)?.spawn("duplex", "rx", Placement::Dedicated, params(0))?;
    Remote::open(&sender)?.spawn("duplex", "tx", Placement::Dedicated, params(1_000_000 / rate))?;
    triq::connect(&format!("{sender}/tx/output"), &format!("{receiver}/rx/input"))?;
    std::thread::sleep(WARMUP + Duration::from_secs(seconds as u64) + Duration::from_millis(500));
    let (generated, lost, latencies) = workers.finish()?;
    let kind = if options.flag("loan") { "filled in place" } else { "copied in" };
    report(
        &format!("hop: two processes, {bytes} bytes {kind}, {rate} Hz, {seconds} s"),
        seconds,
        1,
        rate,
        generated,
        lost,
        latencies,
    );
    Ok(())
}

fn mesh(options: &Options) -> Result {
    let (nodes, rate, bytes, seconds) = (
        options.number("nodes", 30),
        options.number("rate", 5_000),
        options.number("bytes", 200),
        options.number("seconds", 20),
    );
    let (groups, threads) = (options.number("groups", 3), options.number("threads", 10));
    let (processes, shared, dedicated) = (options.flag("processes"), options.flag("shared"), options.flag("dedicated"));
    let workers = if processes {
        Workers::start(groups, 1, threads, shared)?
    } else {
        Workers::start(1, groups, threads, shared)?
    };

    // Where node `index` lives. Partners sit in neighbouring groups, so every message crosses groups.
    let home = |index: usize| -> (String, Placement) {
        let group = (index / 2 + index % 2) % groups;
        let (runtime, pool) = if processes { (group, 0) } else { (0, group) };
        let placement = if dedicated { Placement::Dedicated } else { Placement::pool(&format!("work{pool}")) };
        (workers.programs[runtime].0.clone(), placement)
    };
    let setup = WARMUP + Duration::from_millis(40 * nodes as u64);
    let from_ns = now_ns() + setup.as_nanos() as u64;
    let until_ns = from_ns + seconds as u64 * 1_000_000_000;
    let params = json!({ "period_us": 1_000_000 / rate, "bytes": bytes, "from_ns": from_ns, "until_ns": until_ns });
    for index in 0..nodes {
        let (runtime, placement) = home(index);
        Remote::open(&runtime)?.spawn("duplex", &format!("n{index}"), placement, params.clone())?;
    }
    for index in 0..nodes {
        let partner = index ^ 1;
        triq::connect(&format!("{}/n{partner}/output", home(partner).0), &format!("{}/n{index}/input", home(index).0))?;
    }
    assert!(now_ns() < from_ns, "setup took longer than the warm-up; raise it");
    std::thread::sleep(Duration::from_nanos(until_ns - now_ns()) + Duration::from_millis(500));
    let (generated, lost, latencies) = workers.finish()?;
    let layout = match (processes, dedicated) {
        (_, true) => format!("{} process(es), every node on its own thread", if processes { groups } else { 1 }),
        (true, _) => format!("{groups} processes, each a pool of {threads} threads"),
        (false, _) => format!("one process, {groups} pools of {threads} threads"),
    };
    let kind = if shared { " (shared pools)" } else { "" };
    report(
        &format!("mesh: {nodes} nodes in pairs, {bytes} bytes, {rate} Hz each, {seconds} s; {layout}{kind}"),
        seconds,
        nodes,
        rate,
        generated,
        lost,
        latencies,
    );
    Ok(())
}

fn main() -> Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("worker") => worker(&args[1..]),
        Some("hop") => hop(&Options::parse(&args[1..])),
        Some("mesh") => mesh(&Options::parse(&args[1..])),
        _ => Err(Error::Other("usage: triq-bench hop|mesh [options]; see the top of main.rs".into())),
    }
}
