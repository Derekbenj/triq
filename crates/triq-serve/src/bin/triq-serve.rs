//! `triq-serve [--port 7474] [--no-open]`: triq's programs in a browser. It serves one page, to
//! this machine alone, and opens it. The page starts triq's programs, as processes of their
//! own, shows what they print, and stops them; and it has a line to type the tool's commands on.
//!
//! `triq serve` runs it, with the `triq` tool beside it. The programs it started are stopped
//! when the page's Quit is pressed; Ctrl-C ends it as it ends anything.
use std::net::TcpListener;
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use triq_serve::feed::Feed;
use triq_serve::programs::Programs;
use triq_serve::site::Site;

const USAGE: &str = "\
usage: triq-serve [--port 7474] [--no-open]
  serves a page to this machine alone, and opens it in the browser:
  start triq's programs there, as many of each as are wanted, stop them,
  and type the commands of the triq tool, with their answers drawn";

/// Show a page in the person's own browser, as each system does it.
fn open(address: &str) {
    let opener: &[&str] = match std::env::consts::OS {
        "macos" => &["open"],
        "windows" => &["cmd", "/C", "start"],
        _ => &["xdg-open"],
    };
    let _ = Command::new(opener[0]).args(&opener[1..]).arg(address).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn main() -> ExitCode {
    let (mut port, mut opens) = (7474u16, true);
    let mut told = std::env::args().skip(1);
    while let Some(word) = told.next() {
        match word.as_str() {
            "--no-open" => opens = false,
            "--port" => match told.next().and_then(|given| given.parse().ok()) {
                Some(given) => port = given,
                None => {
                    eprintln!("triq-serve: --port needs a number, such as --port 7474");
                    return ExitCode::FAILURE;
                }
            },
            _ => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
        }
    }
    // To this machine alone: nothing outside it can reach the page.
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("triq-serve: the page cannot be served on port {port}: {error}. Try --port with another.");
            return ExitCode::FAILURE;
        }
    };
    let site = Arc::new(Mutex::new(Site::new(Feed::start(), Programs::here(), port)));
    let address = format!("http://127.0.0.1:{port}/");
    println!("triq-serve: the page is at {address}");
    println!("  for a tool: curl -H 'x-triq-token: {}' {address}api/help", site.lock().unwrap().token());
    if opens {
        open(&address);
    }
    let served = triq_serve::serve(&listener, &site);
    // What was started here ends with it: each runtime is asked to shut down first.
    let mut site = site.lock().unwrap();
    let machine = site.feed.machine();
    site.programs.close(&machine);
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("triq-serve: {error}");
            ExitCode::FAILURE
        }
    }
}
