//! A request read as a browser sends it, and an answer written as a browser reads it.
use serde_json::json;
use std::io::Cursor;
use triq_serve::http::{Answer, Request};

#[test]
fn a_request_is_read_with_its_headers_and_what_comes_after_them() {
    let sent = "POST /api/ask?x=1 HTTP/1.1\r\nHost: 127.0.0.1:7474\r\nX-Triq-Token: abc\r\nContent-Length: 13\r\n\r\n{\"ask\":\"x\"}  and more that is another's";
    let request = Request::read(&mut Cursor::new(sent.as_bytes())).expect("a request");
    assert_eq!((request.method.as_str(), request.path.as_str()), ("POST", "/api/ask"));
    assert_eq!(request.header("host"), Some("127.0.0.1:7474"));
    assert_eq!(request.header("x-triq-token"), Some("abc"));
    assert_eq!(request.body, b"{\"ask\":\"x\"}  ", "as many bytes as it said, and no more");

    // One with nothing after its first lines.
    let request =
        Request::read(&mut Cursor::new(&b"GET / HTTP/1.1\r\nHost: localhost:7474\r\n\r\n"[..])).expect("a request");
    assert_eq!((request.method.as_str(), request.path.as_str(), request.body.len()), ("GET", "/", 0));
}

#[test]
fn what_is_no_request_or_is_too_large_for_one_is_an_error() {
    let read = |sent: &[u8]| Request::read(&mut Cursor::new(sent));
    assert!(read(b"").is_err(), "nothing");
    assert!(read(b"GET / HTTP/1.1\r\nHost: x\r\n").is_err(), "first lines that do not end");
    assert!(read(b"nonsense\r\n\r\n").is_err(), "no method and path");
    assert!(read(b"POST / HTTP/1.1\r\nContent-Length: lots\r\n\r\n").is_err(), "a length that is no number");
    assert!(read(b"POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n").is_err(), "more than the page sends");
    assert!(read(b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort").is_err(), "less than it said");
    let endless = vec![b'a'; 70_000];
    assert!(read(&endless).is_err(), "first lines with no end");
}

#[test]
fn an_answer_says_what_it_is_how_long_it_is_and_that_it_is_the_last() {
    let mut written = Vec::new();
    Answer::json(&json!({ "ok": true })).write(&mut written).expect("write it");
    let written = String::from_utf8(written).unwrap();
    let (head, body) = written.split_once("\r\n\r\n").expect("first lines, an empty one, and the rest");
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
    for line in ["Content-Type: application/json", "Content-Length: 11", "Connection: close", "Cache-Control: no-store"]
    {
        assert!(head.contains(line), "{line} is not in {head}");
    }
    assert_eq!(body, r#"{"ok":true}"#);

    let mut written = Vec::new();
    Answer::refused(403, "not for you").write(&mut written).expect("write it");
    let written = String::from_utf8(written).unwrap();
    assert!(written.starts_with("HTTP/1.1 403 Forbidden\r\n") && written.ends_with("\r\n\r\nnot for you"), "{written}");
}
