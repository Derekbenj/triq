//! As much of HTTP as one page on this machine needs: a request read from a browser, and an
//! answer written back. One request to a connection, which is closed after its answer.
use serde_json::Value;
use std::io::{self, Read, Write};

/// The most a request's first lines may be, and the most that may come after them: a browser's
/// own headers are a kilobyte or two, and what the page sends is a few lines of JSON.
const MOST_HEAD: usize = 64 * 1024;
const MOST_BODY: usize = 1024 * 1024;

/// What a browser asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `GET` or `POST`.
    pub method: String,
    /// What it asks for, without what follows a `?`.
    pub path: String,
    /// The names in small letters.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(known, _)| known == name).map(|(_, value)| value.as_str())
    }

    /// Read one request. An error for what is no request, or is larger than one of the page's.
    pub fn read(from: &mut impl Read) -> io::Result<Self> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
        // The first lines end with an empty one. They are read a byte at a time, so that
        // nothing of what comes after them is taken with them.
        let (mut head, mut byte) = (Vec::new(), [0u8]);
        while !head.ends_with(b"\r\n\r\n") {
            if from.read(&mut byte)? == 0 || head.len() >= MOST_HEAD {
                return Err(bad("the request's first lines do not end"));
            }
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        let mut lines = head.split("\r\n");
        let mut first = lines.next().unwrap_or_default().split(' ');
        let (Some(method), Some(target)) = (first.next(), first.next()) else { return Err(bad("no method and path")) };
        let named = |line: &str| {
            line.split_once(':').map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_owned()))
        };
        let headers: Vec<(String, String)> = lines.filter_map(named).collect();
        let length =
            headers.iter().find(|(name, _)| name == "content-length").map(|(_, length)| length.parse::<usize>());
        let length = length.transpose().map_err(|_| bad("a length that is no number"))?.unwrap_or(0);
        if length > MOST_BODY {
            return Err(bad("more than the page ever sends"));
        }
        let mut body = vec![0; length];
        from.read_exact(&mut body)?;
        let path = target.split('?').next().unwrap_or_default().to_owned();
        Ok(Self { method: method.to_owned(), path, headers, body })
    }
}

/// What is sent back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    /// What kind of thing the body is, as a browser is told it.
    pub kind: &'static str,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn json(value: &Value) -> Self {
        Self { status: 200, kind: "application/json", body: value.to_string().into_bytes() }
    }

    /// Words that say why not, with the status that goes with them.
    pub fn refused(status: u16, why: &str) -> Self {
        Self { status, kind: "text/plain; charset=utf-8", body: why.as_bytes().to_vec() }
    }

    pub fn write(&self, to: &mut impl Write) -> io::Result<()> {
        let said = match self.status {
            200 => "OK",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            _ => "Error",
        };
        // Nothing of it is to be kept by the browser: the page is the program's, as it is now.
        let head = format!(
            "HTTP/1.1 {} {said}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.status,
            self.kind,
            self.body.len()
        );
        to.write_all(head.as_bytes())?;
        to.write_all(&self.body)?;
        to.flush()
    }
}
