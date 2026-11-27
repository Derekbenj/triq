//! What the page is served and what it may ask: programs started and stopped, and a runtime
//! of the test's own shut down. That the page is served to this machine's own browser alone.
//! And the page's own logic, tried with node where there is one.
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use triq::prelude::*;
use triq_serve::feed::Feed;
use triq_serve::http::{Answer, Request};
use triq_serve::programs::Programs;
use triq_serve::records::{Records, chosen};
use triq_serve::site::Site;

const PORT: u16 = 7474;

/// A folder for the records of a test, which is empty.
fn kept(tag: &str) -> Records {
    let folder = std::env::temp_dir().join(format!("triq-serve-{}-{tag}-site", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    Records::in_folder(folder)
}

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

/// A request as the page's own browser sends it: under this machine's name, and with the
/// page's token if `token` is given.
fn request(method: &str, path: &str, token: Option<&str>, body: Value) -> Request {
    let mut headers = vec![("host".to_owned(), format!("127.0.0.1:{PORT}"))];
    headers.extend(token.map(|token| ("x-triq-token".to_owned(), token.to_owned())));
    let body = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    Request { method: method.to_owned(), path: path.to_owned(), headers, body }
}

fn json_of(answer: &Answer) -> Value {
    assert_eq!(answer.status, 200, "{}", String::from_utf8_lossy(&answer.body));
    serde_json::from_slice(&answer.body).expect("JSON")
}

/// A program of the test's own: a script.
#[cfg(unix)]
fn script(tag: &str, name: &str, does: &str) -> (String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let folder = std::env::temp_dir().join(format!("triq-serve-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder");
    let path = folder.join(format!("triq-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{does}\n")).expect("a script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("a program");
    (name.to_owned(), path)
}

#[test]
fn the_page_is_served_to_this_machines_own_browser_and_to_no_other_page_in_it() {
    let mut site = Site::new(Feed::start(), Programs::new(Vec::new(), kept("served")), PORT);
    let token = site.token().to_owned();
    assert!(
        token.len() == 32 && token != Site::new(Feed::start(), Programs::new(Vec::new(), kept("other")), PORT).token(),
        "a token of its own"
    );

    // The page, with its token in it, and its parts.
    let page = site.answer(&request("GET", "/", None, Value::Null));
    let text = String::from_utf8(page.body.clone()).unwrap();
    assert!(page.status == 200 && page.kind.starts_with("text/html"), "{page:?}");
    assert!(text.contains(&token) && !text.contains("__TOKEN__"));
    for part in ["/app.css", "/app.js", "/logic.js", "/hero.js"] {
        let part = site.answer(&request("GET", part, None, Value::Null));
        assert!(part.status == 200 && part.body.len() > 500, "{}", part.status);
    }
    // The letters of its headings are a font that is in the program.
    let font = site.answer(&request("GET", "/press-start-2p.ttf", None, Value::Null));
    assert!(font.status == 200 && font.kind == "font/ttf" && font.body.len() > 50_000, "{}", font.status);
    assert_eq!(site.answer(&request("GET", "/nothing", None, Value::Null)).status, 404);
    // The page is its own: it fetches nothing from elsewhere.
    assert!(!text.contains("http://") && !text.contains("https://"));

    // Under another name than this machine's, nothing is answered: not even the page.
    for host in ["evil.example", "127.0.0.1:9999", "192.168.1.5:7474"] {
        let mut asked = request("GET", "/", None, Value::Null);
        asked.headers[0].1 = host.to_owned();
        assert_eq!(site.answer(&asked).status, 403, "{host}");
    }
    assert_eq!(
        site.answer(&Request {
            headers: vec![("host".into(), format!("localhost:{PORT}"))],
            ..request("GET", "/", None, Value::Null)
        })
        .status,
        200
    );

    // Without the page's token, nothing under /api/ is answered, and nothing is done.
    for (method, path) in [("GET", "/api/state"), ("POST", "/api/quit"), ("POST", "/api/start"), ("GET", "/api/help")] {
        assert_eq!(site.answer(&request(method, path, None, json!({}))).status, 403, "{path}");
        assert_eq!(site.answer(&request(method, path, Some("a guess"), json!({}))).status, 403, "{path}");
    }
    assert!(!site.quit);

    // With it, the help says what can be asked, and Quit is taken.
    let help = json_of(&site.answer(&request("GET", "/api/help", Some(&token), Value::Null)));
    let asks = ["GET /api/state", "GET /api/machine", "POST /api/command", "POST /api/start", "POST /api/stop"];
    let more = ["POST /api/shutdown", "POST /api/forget", "GET /api/usage", "GET /api/records", "POST /api/records"];
    for asked in asks.into_iter().chain(more) {
        assert!(help["requests"][asked].is_string(), "{asked}");
    }
    json_of(&site.answer(&request("POST", "/api/quit", Some(&token), json!({}))));
    assert!(site.quit && !site.leave, "what was started is stopped, unless it is said otherwise");
}

#[cfg(unix)]
#[test]
fn the_page_says_where_the_records_are_kept_and_whether_what_runs_is_left_running() {
    let talker = script("kept", "talker", "echo hello\nsleep 30");
    let records = kept("where");
    let (from, to) = (records.folder().to_owned(), kept("where-to").folder().join("of my own"));
    let mut site = Site::new(Feed::start(), Programs::new(vec![talker], records), PORT);
    site.choice = from.with_extension("choice");
    let token = site.token().to_owned();
    let ask = |site: &mut Site, method: &str, path: &str, body: Value| {
        site.answer(&request(method, path, Some(&token), body))
    };
    assert_eq!(json_of(&ask(&mut site, "GET", "/api/records", Value::Null)), json!({ "folder": from }));
    json_of(&ask(&mut site, "POST", "/api/start", json!({ "program": "talker", "told": "" })));
    assert_eq!(Records::in_folder(&from).all().len(), 1);

    // Another folder is chosen: the records there are go with it, and the choice is remembered.
    let moved = ask(&mut site, "POST", "/api/records", json!({ "folder": to }));
    assert_eq!(json_of(&moved), json!({ "folder": to }));
    assert_eq!((Records::in_folder(&from).all().len(), Records::in_folder(&to).all().len()), (0, 1));
    assert_eq!(chosen(&site.choice), Some(to.clone()));
    assert_eq!(json_of(&ask(&mut site, "GET", "/api/records", Value::Null)), json!({ "folder": to }));
    // The program prints on into its file where that is now, and is told of there.
    let started = json_of(&ask(&mut site, "GET", "/api/state", Value::Null))["programs"]["started"][0].clone();
    assert_eq!(started["log"], to.join("1.log").to_str().unwrap());
    // What is no folder by its whole path is refused, and nothing is moved.
    for folder in [json!({ "folder": "somewhere/near" }), json!({ "folder": "  " }), json!({})] {
        assert_eq!(ask(&mut site, "POST", "/api/records", folder).status, 400);
    }
    assert_eq!(chosen(&site.choice), Some(to));

    // The page may ask for what runs to be left running as this program ends.
    json_of(&ask(&mut site, "POST", "/api/quit", json!({ "leave": true })));
    assert!(site.quit && site.leave);
    site.programs.close(&[], false);
}

#[test]
fn the_page_is_told_which_runtimes_there_are_and_one_it_names_is_shut_down() -> triq::Result {
    let name = format!("srv{}-site", std::process::id());
    let runtime = Runtime::new(&name)?;
    let mut site = Site::new(Feed::start(), Programs::new(Vec::new(), kept("runtimes")), PORT);
    let token = site.token().to_owned();
    let state = |site: &mut Site| json_of(&site.answer(&request("GET", "/api/state", Some(&token), Value::Null)));
    let home = |state: &Value| {
        state["runtimes"].as_array().unwrap().iter().find(|home| home["runtime"] == name.as_str()).cloned()
    };
    let mut seen = Value::Null;
    assert!(wait_for(|| {
        seen = state(&mut site);
        home(&seen).is_some()
    }));
    // A runtime that was started elsewhere: its name, its process, how many nodes, and that no
    // program started here is it. Nothing more of it is the page's business.
    let expected = json!({ "runtime": name, "pid": std::process::id(), "nodes": 0, "started": null });
    assert_eq!(home(&seen).unwrap(), expected);
    assert_eq!(seen["said"], json!([0, null]));

    // What is no request the page makes is refused, with why: also what the workbench is for.
    let refused = [
        ("/api/shutdown", json!({ "runtime": "srv-nobody-is-called-this" })),
        ("/api/shutdown", json!({})),
        ("/api/start", json!({ "program": "nope" })),
        ("/api/stop", json!({ "id": 99 })),
        ("/api/ask", json!({ "ask": "pause", "runtime": name, "node": "nobody" })),
    ];
    for (path, body) in refused {
        let refused = site.answer(&request("POST", path, Some(&token), body));
        assert!(refused.status == 400 && !refused.body.is_empty(), "{path}: {refused:?}");
    }
    assert!(!runtime.stopping());

    // The whole of what the runtimes report is had when it is asked for: what `triq json` prints.
    let machine = json_of(&site.answer(&request("GET", "/api/machine", Some(&token), Value::Null)));
    let whole = machine.as_array().unwrap().iter().find(|home| home["runtime"] == name.as_str()).expect("the runtime");
    assert!(whole["nodes"] == json!([]) && whole["factories"] == json!([]) && whole["pools"].is_array(), "{whole}");

    // Named, the runtime is asked to shut down.
    json_of(&site.answer(&request("POST", "/api/shutdown", Some(&token), json!({ "runtime": name }))));
    assert!(wait_for(|| runtime.stopping()), "the runtime was asked to stop");
    Ok(())
}

#[cfg(unix)]
#[test]
fn programs_are_started_from_the_page_more_than_one_of_each_and_stopped() {
    let talker = script("page", "talker", "echo \"told: $*\"\nsleep 30");
    let mut site = Site::new(Feed::start(), Programs::new(vec![talker], kept("page")), PORT);
    let token = site.token().to_owned();
    let ask = |site: &mut Site, method: &str, path: &str, body: Value| {
        json_of(&site.answer(&request(method, path, Some(&token), body)))
    };
    let started = |site: &mut Site| {
        ask(site, "GET", "/api/state", Value::Null)["programs"]["started"].as_array().unwrap().clone()
    };
    assert_eq!(ask(&mut site, "GET", "/api/state", Value::Null)["programs"]["found"], json!(["talker"]));

    // Two of the one program, each told its own words.
    let first = json!({ "program": "talker", "told": "one --speed 5" });
    assert_eq!(ask(&mut site, "POST", "/api/start", first)["id"], 1);
    assert_eq!(ask(&mut site, "POST", "/api/start", json!({ "program": "talker", "told": "two" }))["id"], 2);
    let mut both = Vec::new();
    assert!(wait_for(|| {
        both = started(&mut site);
        both.len() == 2 && both.iter().all(|one| one["printed"].as_array().is_some_and(|lines| lines.len() == 1))
    }));
    assert_eq!(both[0]["told"], json!(["one", "--speed", "5"]));
    assert_eq!(both[0]["printed"], json!(["told: one --speed 5"]));
    assert_eq!((&both[1]["told"], &both[1]["ended"]), (&json!(["two"]), &Value::Null));

    // One is stopped, and the other runs on. Forgotten, the one that ended is listed no more.
    ask(&mut site, "POST", "/api/stop", json!({ "id": 1 }));
    assert!(wait_for(|| started(&mut site)[0]["ended"] == "stopped"));
    assert!(started(&mut site)[1]["ended"].is_null());
    ask(&mut site, "POST", "/api/forget", json!({ "id": 1 }));
    let left: Vec<u64> = started(&mut site).iter().map(|one| one["id"].as_u64().unwrap()).collect();
    assert_eq!(left, [2]);
    // With no number, every one that has ended is forgotten: here none, the second still runs.
    ask(&mut site, "POST", "/api/forget", json!({}));
    assert_eq!(started(&mut site).len(), 1);
    ask(&mut site, "POST", "/api/stop", json!({ "id": 2 }));
    assert!(wait_for(|| started(&mut site)[0]["ended"] == "stopped"));
}

#[cfg(unix)]
#[test]
fn a_line_typed_on_the_page_is_a_command_for_the_tool_or_a_program_to_start() {
    let talker = script("line", "talker", "echo \"told: $*\"\nsleep 30");
    let (_, tool) = script(
        "line",
        "tool",
        "if [ \"$1\" = help ]; then echo 'usage:'; echo '  triq list   runtimes'; else echo \"asked: $*\"; fi",
    );
    let mut site = Site::new(Feed::start(), Programs::new(vec![talker], kept("line")), PORT);
    site.tool = tool;
    let token = site.token().to_owned();
    let ask = |site: &mut Site, method: &str, path: &str, body: Value| {
        json_of(&site.answer(&request(method, path, Some(&token), body)))
    };
    let programs = |site: &mut Site| ask(site, "GET", "/api/state", Value::Null)["programs"].clone();

    // A command of the tool, typed with its name or without, is run by the tool, told the words.
    let typed = json!({ "line": "triq connect robot/imu/pose 'desk/viewer/input'" });
    assert_eq!(ask(&mut site, "POST", "/api/command", typed), json!({ "asked": 1 }));
    assert_eq!(ask(&mut site, "POST", "/api/command", json!({ "line": "pause robot/imu" })), json!({ "asked": 2 }));
    let mut asked = Value::Null;
    assert!(wait_for(|| {
        asked = programs(&mut site)["asked"].clone();
        asked.as_array().is_some_and(|asked| asked.len() == 2 && asked.iter().all(|one| one["ended"] == "ended"))
    }));
    assert_eq!(asked[0]["told"], json!(["connect", "robot/imu/pose", "desk/viewer/input"]));
    assert_eq!(asked[0]["printed"], json!(["asked: connect robot/imu/pose desk/viewer/input"]));
    assert_eq!(asked[1]["printed"], json!(["asked: pause robot/imu"]));

    // The name of a program is that program, started as its button starts it, told the rest.
    assert_eq!(
        ask(&mut site, "POST", "/api/command", json!({ "line": "triq talker one --speed 5" })),
        json!({ "started": 3 })
    );
    let started = programs(&mut site)["started"].clone();
    assert_eq!((&started[0]["program"], &started[0]["told"]), (&json!("talker"), &json!(["one", "--speed", "5"])));
    ask(&mut site, "POST", "/api/stop", json!({ "id": 3 }));

    // Nothing typed is nothing to do.
    for nothing in [json!({ "line": "   " }), json!({ "line": "triq" }), json!({})] {
        assert_eq!(site.answer(&request("POST", "/api/command", Some(&token), nothing)).status, 400);
    }
    // What the tool can be told is had from the tool, for the page to offer as it is typed.
    assert_eq!(
        ask(&mut site, "GET", "/api/usage", Value::Null),
        json!({ "usage": "usage:\n  triq list   runtimes\n" })
    );
}

#[test]
fn a_browser_is_answered_over_a_connection_until_the_page_asks_the_program_to_end() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let port = listener.local_addr().unwrap().port();
    let site = Arc::new(Mutex::new(Site::new(Feed::start(), Programs::new(Vec::new(), kept("browser")), port)));
    let token = site.lock().unwrap().token().to_owned();
    let serving = std::thread::spawn({
        let site = site.clone();
        move || triq_serve::serve(&listener, &site)
    });
    let ask = |sent: String| {
        let mut browser = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        browser.write_all(sent.as_bytes()).expect("send");
        let mut answer = String::new();
        browser.read_to_string(&mut answer).expect("an answer, and the connection closed");
        answer
    };
    let page = ask(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
    assert!(
        page.starts_with("HTTP/1.1 200 OK") && page.contains("<title>triq</title>"),
        "{}",
        &page[..200.min(page.len())]
    );
    // What is no request is told so, and what names no host is not answered.
    assert!(ask("nonsense\r\n\r\n".to_owned()).starts_with("HTTP/1.1 400"));
    assert!(ask("GET / HTTP/1.1\r\n\r\n".to_owned()).starts_with("HTTP/1.1 403"));
    // A connection that says nothing holds up nobody else.
    let _silent = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let state = ask(format!("GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Triq-Token: {token}\r\n\r\n"));
    assert!(state.starts_with("HTTP/1.1 200 OK") && state.contains("\"runtimes\""), "{state}");
    let quit = ask(format!(
        "POST /api/quit HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Triq-Token: {token}\r\nContent-Length: 2\r\n\r\n{{}}"
    ));
    assert!(quit.starts_with("HTTP/1.1 200 OK"));
    serving.join().expect("the serving ends").expect("without an error");
}

/// The page offers a free name to a program that ended because its runtime's name was taken,
/// and knows that by what triq says: `clash` in `assets/logic.js` reads these words.
#[test]
fn a_name_that_is_taken_is_said_in_the_words_the_page_knows_it_by() -> triq::Result {
    let name = format!("srv{}-taken", std::process::id());
    let _first = Runtime::new(&name)?;
    let Err(error) = Runtime::new(&name) else { panic!("two runtimes of one name") };
    let known = format!("a runtime named \"{name}\" is already running");
    assert!(error.to_string().contains(&known), "{error}");
    Ok(())
}

#[test]
fn the_pages_own_logic_holds_where_there_is_node_to_try_it_with() {
    let tests = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/logic.test.mjs");
    match std::process::Command::new("node").args(["--test", tests]).output() {
        Ok(run) => assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stdout)),
        // A machine without node does not try it: the page is the same page.
        Err(_) => eprintln!("node is not on this machine: the page's logic was not tried"),
    }
}
