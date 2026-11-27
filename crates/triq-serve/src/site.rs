//! What the page is, and what it may ask: the page and its parts, and under `/api/` the
//! programs that can be started and those that were, the runtimes there are, and the requests
//! that start a program, stop one, shut a runtime down, and run the `triq` tool with a command
//! that was typed. To draw and to wire what is inside a runtime is the workbench's.
//!
//! The page starts programs and stops them, so it is served with care. Only a request that
//! names this machine as its host is answered, and under `/api/` only one that carries the
//! token this program made as it started, which the page it serves is given and no other page
//! in the browser can read.
use crate::feed::Feed;
use crate::http::{Answer, Request};
use crate::programs::{Programs, Typed, at_home};
use crate::records;
use serde_json::{Value, json};
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::process::Command;

const PAGE: &str = include_str!("../assets/index.html");
const STYLE: &str = include_str!("../assets/app.css");
const SCRIPT: &str = include_str!("../assets/app.js");
const LOGIC: &str = include_str!("../assets/logic.js");
const HERO: &str = include_str!("../assets/hero.js");
/// The letters of the name and of the headings: Press Start 2P, under the licence beside it.
const FONT: &[u8] = include_bytes!("../assets/press-start-2p.ttf");

/// What the page is told, and what can be asked through it: for a person who reads it, and for
/// a language model that is to use it.
const HELP: &str = include_str!("../assets/help.json");

pub struct Site {
    pub feed: Feed,
    pub programs: Programs,
    /// The `triq` tool, which is run with the commands that are typed on the page.
    pub tool: PathBuf,
    /// The file that remembers which folder was chosen for the records of what is started.
    pub choice: PathBuf,
    port: u16,
    token: String,
    /// What went wrong last, and how many things have: the page shows each once.
    said: (u64, Option<String>),
    /// Set when the page asks this program to end; and whether the programs that run are to
    /// be left running then.
    pub quit: bool,
    pub leave: bool,
}

/// A token nobody can guess: the standard library keys its hashes from the system's own
/// randomness, anew for each `RandomState`.
fn token() -> String {
    let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", part(), part())
}

/// The tool: the one beside this program, as `cargo install` puts it, or else the one on the `PATH`.
fn tool() -> PathBuf {
    let name = format!("triq{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|program| Some(program.parent()?.join(&name)));
    beside.filter(|tool| tool.is_file()).unwrap_or_else(|| name.into())
}

impl Site {
    pub fn new(feed: Feed, programs: Programs, port: u16) -> Self {
        let (tool, choice) = (tool(), records::choice());
        Self { feed, programs, tool, choice, port, token: token(), said: (0, None), quit: false, leave: false }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Answer one request.
    pub fn answer(&mut self, request: &Request) -> Answer {
        // A page of another site may send requests here, but not under this machine's own name.
        let host = request.header("host").unwrap_or_default();
        if host != format!("127.0.0.1:{}", self.port) && host != format!("localhost:{}", self.port) {
            return Answer::refused(403, "this page is served to this machine alone");
        }
        let part = |kind, text: &str| Answer { status: 200, kind, body: text.as_bytes().to_vec() };
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => part("text/html; charset=utf-8", &PAGE.replace("__TOKEN__", &self.token)),
            ("GET", "/app.css") => part("text/css; charset=utf-8", STYLE),
            ("GET", "/app.js") => part("text/javascript; charset=utf-8", SCRIPT),
            ("GET", "/logic.js") => part("text/javascript; charset=utf-8", LOGIC),
            ("GET", "/hero.js") => part("text/javascript; charset=utf-8", HERO),
            ("GET", "/press-start-2p.ttf") => Answer { status: 200, kind: "font/ttf", body: FONT.to_vec() },
            (_, path) if path.starts_with("/api/") => {
                if request.header("x-triq-token") != Some(self.token.as_str()) {
                    return Answer::refused(403, "the page's token is not with this request");
                }
                self.api(request).unwrap_or_else(|why| Answer::refused(400, &why))
            }
            _ => Answer::refused(404, "there is no such thing here"),
        }
    }

    /// Answer a request under `/api/`, or say why it cannot be done.
    fn api(&mut self, request: &Request) -> Result<Answer, String> {
        let sent = || serde_json::from_slice::<Value>(&request.body).map_err(|error| format!("not JSON: {error}"));
        let done = Answer::json(&json!({}));
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/state") => {
                self.programs.poll();
                if let Some(problem) = self.feed.said() {
                    self.said = (self.said.0 + 1, Some(problem));
                }
                let machine = self.feed.machine();
                // Of a runtime the page is told little: enough to know it by and to shut it down.
                let runtime = |home: &triq::Inventory| {
                    let started = self.programs.of(home.pid);
                    json!({ "runtime": home.runtime, "pid": home.pid, "nodes": home.nodes.len(), "started": started })
                };
                let runtimes: Vec<Value> = machine.iter().map(runtime).collect();
                let programs = self.programs.told(&machine);
                Ok(Answer::json(&json!({ "programs": programs, "runtimes": runtimes, "said": self.said })))
            }
            ("GET", "/api/machine") => {
                let machine = serde_json::to_value(self.feed.machine()).map_err(|error| error.to_string())?;
                Ok(Answer::json(&machine))
            }
            // Asked of the tool each time: it also says which folders are registered, which changes.
            ("GET", "/api/usage") => {
                let said = Command::new(&self.tool).arg("help").output();
                let said = said.map_err(|error| format!("the triq tool could not be run: {error}"))?;
                Ok(Answer::json(&json!({ "usage": String::from_utf8_lossy(&said.stdout) })))
            }
            // What a program says it takes, in its own words, or null if it does not say.
            ("POST", "/api/takes") => {
                let sent = sent()?;
                let name = sent["program"].as_str().ok_or("no program is named")?;
                let program = self.programs.found.iter().position(|(found, _)| found == name);
                let program = program.ok_or_else(|| format!("there is no program triq-{name} here"))?;
                Ok(Answer::json(&json!({ "takes": self.programs.takes(program) })))
            }
            ("GET", "/api/records") => Ok(Answer::json(&json!({ "folder": self.programs.records.folder() }))),
            // Another folder for the records: those there are go with it, and it is remembered.
            ("POST", "/api/records") => {
                let named = sent()?["folder"].as_str().unwrap_or_default().to_owned();
                // As a terminal has it: ~ is the person's own folder.
                let folder = PathBuf::from(at_home(named.trim()));
                if !folder.is_absolute() {
                    return Err(format!("{} is not a folder by its whole path, from /", folder.display()));
                }
                let moved = self.programs.records.move_to(&folder);
                moved.map_err(|error| format!("the records could not be moved to {}: {error}", folder.display()))?;
                let kept = records::choose(&self.choice, &folder);
                kept.map_err(|error| format!("{} could not be written: {error}", self.choice.display()))?;
                Ok(Answer::json(&json!({ "folder": folder })))
            }
            ("POST", "/api/command") => {
                let line = sent()?["line"].as_str().unwrap_or_default().to_owned();
                let typed = self.programs.typed(&self.tool, &line);
                match typed.map_err(|error| format!("could not run that: {error}"))? {
                    Some(Typed::Started(id)) => Ok(Answer::json(&json!({ "started": id }))),
                    Some(Typed::Asked(id)) => Ok(Answer::json(&json!({ "asked": id }))),
                    None => Err("nothing was typed".to_owned()),
                }
            }
            ("GET", "/api/help") => {
                Ok(Answer { status: 200, kind: "application/json", body: HELP.as_bytes().to_vec() })
            }
            ("POST", "/api/start") => {
                let sent = sent()?;
                let name = sent["program"].as_str().ok_or("no program is named")?;
                let program = self.programs.found.iter().position(|(found, _)| found == name);
                let program = program.ok_or_else(|| format!("there is no program triq-{name} here"))?;
                let told = sent["told"].as_str().unwrap_or_default();
                let id = self
                    .programs
                    .start(program, told)
                    .map_err(|error| format!("could not start triq-{name}: {error}"))?;
                Ok(Answer::json(&json!({ "id": id })))
            }
            ("POST", "/api/stop") => {
                let id = sent()?["id"].as_u64().ok_or("no number of a program that was started")?;
                let place = self.programs.place(id).ok_or("no program that was started has that number")?;
                if let Some(runtime) = self.programs.stop(place, &self.feed.machine()) {
                    self.feed.shut_down(&runtime);
                }
                Ok(done)
            }
            ("POST", "/api/shutdown") => {
                let sent = sent()?;
                let runtime = sent["runtime"].as_str().ok_or("no runtime is named")?;
                if !self.feed.machine().iter().any(|home| home.runtime == runtime) {
                    return Err(format!("there is no runtime {runtime} on this machine"));
                }
                self.feed.shut_down(runtime);
                Ok(done)
            }
            ("POST", "/api/forget") => {
                self.programs.forget(sent().ok().and_then(|sent| sent["id"].as_u64()));
                Ok(done)
            }
            // With `leave`, the programs that run are left running.
            ("POST", "/api/quit") => {
                (self.quit, self.leave) = (true, sent().is_ok_and(|sent| sent["leave"] == true));
                Ok(done)
            }
            _ => Err("there is no such thing to ask".to_owned()),
        }
    }
}
