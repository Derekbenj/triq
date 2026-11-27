//! triq's programs in a browser: a page served to this machine alone, which starts them as
//! processes of their own, more than one of each if that is wanted, shows what they print,
//! and stops them.
pub mod feed;
pub mod http;
pub mod programs;
pub mod site;

use http::{Answer, Request};
use site::Site;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Answer the browser until the page asks this program to end. Each connection has a thread of
/// its own for as long as its one request takes to come, so that a browser that opens one and
/// sends nothing holds up nobody: the page is answered one request at a time all the same.
pub fn serve(listener: &TcpListener, site: &Arc<Mutex<Site>>) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    while !site.lock().unwrap().quit {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => continue,
        };
        let site = site.clone();
        std::thread::spawn(move || {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let answer = match Request::read(&mut stream) {
                Ok(request) => site.lock().unwrap().answer(&request),
                Err(_) => Answer::refused(400, "that is no request"),
            };
            let _ = answer.write(&mut stream);
        });
    }
    Ok(())
}
