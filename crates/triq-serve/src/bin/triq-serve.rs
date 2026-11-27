//! `triq-serve [--port 7474] [--no-open]`: triq's programs in a browser. It serves one page, to
//! this machine alone, and opens it. The page starts triq's programs, as processes of their
//! own, shows what they print, and stops them; and it has a line to type the tool's commands on.
//!
//! `triq serve` runs it, with the `triq` tool beside it. The programs it started do not
//! depend on it: the page's Quit stops them or leaves them running, as is chosen there, and
//! Ctrl-C ends this program alone. The next triq-serve finds what was left running.
use std::net::TcpListener;
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use triq_serve::feed::Feed;
use triq_serve::programs::Programs;
use triq_serve::records::{self, Records};
use triq_serve::site::Site;

const USAGE: &str = "\
usage: triq-serve [--port 7474] [--no-open]
  serves a page to this machine alone, and opens it in the browser:
  start triq's programs there, as many of each as are wanted, stop them,
  and type the commands of the triq tool, with their answers drawn
  --port 7474   the port the page is served on
  --no-open     do not open the page in the browser";

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
    triq::help(USAGE);
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
    // The records of what was started are in the folder that was chosen for them, or the usual
    // one. What an earlier triq-serve left running is found by them.
    let records = Records::in_folder(records::chosen(&records::choice()).unwrap_or_else(records::usual));
    let site = Arc::new(Mutex::new(Site::new(Feed::start(), Programs::here(records), port)));
    let address = format!("http://127.0.0.1:{port}/");
    println!("triq-serve: the page is at {address}");
    println!("  for a tool: curl -H 'x-triq-token: {}' {address}api/help", site.lock().unwrap().token());
    if opens {
        open(&address);
    }
    let served = triq_serve::serve(&listener, &site);
    // What was started here is stopped, each runtime asked to shut down first: unless the page
    // asked for it to be left running.
    let mut site = site.lock().unwrap();
    let (machine, leave) = (site.feed.machine(), site.leave);
    site.programs.close(&machine, leave);
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("triq-serve: {error}");
            ExitCode::FAILURE
        }
    }
}
