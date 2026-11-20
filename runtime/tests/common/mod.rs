//! Shared pieces for the server tests: a realm booted on the worker thread, and a raw client that
//! does not parse anything it was not asked to parse.
#![allow(dead_code)]

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::time::{Duration, Instant};

use xmas_runtime::driver::Driver;
use xmas_runtime::host::server::Server;
use xmas_runtime::host::Host;
use xmas_runtime::modules;

/// Boot a realm the way the cli will: build it on the worker thread, install everything, run the
/// program. The program is what calls Xmas.serve and registers the handler.
pub fn boot(
    script: &'static str,
) -> impl Fn(usize) -> Result<Rc<RefCell<Driver>>, String> + Send + Sync + Clone + 'static {
    move |_index| {
        let mut driver = Driver::new(Host::capture())?;
        modules::install(&mut driver)?;
        driver.eval(script)?;
        Ok(Rc::new(RefCell::new(driver)))
    }
}

pub fn start(script: &'static str) -> Server {
    start_with(script, 1)
}

pub fn start_with(script: &'static str, workers: usize) -> Server {
    modules::server::serve("127.0.0.1:0", workers, boot(script)).expect("server starts")
}

/// Serve a module from disk the way the cli does: every worker evaluates the entry itself.
pub fn start_file(path: &std::path::Path) -> Server {
    xmas_runtime::entry::serve_app("127.0.0.1:0", 1, path).expect("server starts")
}

/// A temporary file with the given name and contents.
pub fn written(name: &str, content: &str) -> std::path::PathBuf {
    let unique = xmas_runtime::testkit::given_file(content);
    let path = unique.with_file_name(format!(
        "xmas-entry-{}-{name}",
        unique.file_name().unwrap().to_string_lossy()
    ));
    std::fs::rename(&unique, &path).expect("rename temporary file");
    path
}

/// A connection spoken to by hand.
pub struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Client {
    pub fn to(server: &Server) -> Self {
        let stream = TcpStream::connect(server.addr()).expect("connect");
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("timeout");
        Client {
            stream,
            buffer: Vec::new(),
        }
    }

    pub fn send(&mut self, text: &str) {
        self.stream.write_all(text.as_bytes()).expect("write");
    }

    /// Everything that arrives in the next window, as text. Used when a test wants to see chunks
    /// as they come rather than a finished response.
    pub fn read_for(&mut self, window: Duration) -> String {
        let deadline = Instant::now() + window;
        while Instant::now() < deadline {
            if self.fill() == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        String::from_utf8_lossy(&self.buffer).to_string()
    }

    /// Read until the needle shows up, or give up at the deadline.
    pub fn read_until(&mut self, needle: &str, within: Duration) -> String {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if find(&self.buffer, needle.as_bytes()).is_some() {
                break;
            }
            self.fill();
        }
        String::from_utf8_lossy(&self.buffer).to_string()
    }

    /// A whole response, framed by its content-length.
    ///
    /// The read timeout is short so a streaming test can end a window; a whole response therefore
    /// polls until its own deadline instead of treating one quiet moment as the end.
    pub fn response(&mut self) -> (String, String) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let head_end = loop {
            if let Some(index) = find(&self.buffer, b"\r\n\r\n") {
                break index;
            }
            assert!(self.fill_until(deadline) > 0, "server closed the connection");
        };
        let head = String::from_utf8_lossy(&self.buffer[..head_end]).to_string();
        let rest = self.buffer.split_off(head_end + 4);
        self.buffer = rest;

        let length = head
            .to_ascii_lowercase()
            .split("\r\n")
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .map(|value| value.trim().to_string())
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        while self.buffer.len() < length {
            assert!(self.fill_until(deadline) > 0, "server closed the connection");
        }
        let rest = self.buffer.split_off(length);
        let body = String::from_utf8_lossy(&self.buffer).to_string();
        self.buffer = rest;
        (head, body)
    }

    /// Take whatever has arrived so far and forget it, so the next read starts clean.
    pub fn take(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.buffer).to_string();
        self.buffer.clear();
        text
    }

    /// Read, and keep trying until something arrives or the deadline passes. A read timeout is not
    /// an error here: it only means the server is still thinking.
    fn fill_until(&mut self, deadline: Instant) -> usize {
        loop {
            let read = self.fill();
            if read > 0 || Instant::now() >= deadline {
                return read;
            }
        }
    }

    fn fill(&mut self) -> usize {
        let mut chunk = [0u8; 8192];
        match self.stream.read(&mut chunk) {
            Ok(0) => 0,
            Ok(read) => {
                self.buffer.extend_from_slice(&chunk[..read]);
                read
            }
            Err(_) => 0,
        }
    }
}

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

pub fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}
