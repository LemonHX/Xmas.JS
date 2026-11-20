//! The server with a realm behind it: trillium parses the request, JS answers it.
//!
//! These tests are about the crossing, not about HTTP (tests/server.rs covers that): a handler
//! that answers immediately, one that answers after a timer, one that throws, and a realm whose
//! state has to survive from one request to the next.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::time::Duration;

use xmas_runtime::driver::Driver;
use xmas_runtime::host::server::Server;
use xmas_runtime::host::Host;
use xmas_runtime::modules;

/// Boot a realm the way the cli will: build it on the worker thread, install everything, run the
/// program. The program is what calls Xmas.serve and registers the handler.
fn boot(
    script: &'static str,
) -> impl Fn(usize) -> Result<Rc<RefCell<Driver>>, String> + Send + Sync + Clone + 'static {
    move |_index| {
        let mut driver = Driver::new(Host::capture())?;
        modules::install(&mut driver)?;
        driver.eval(script)?;
        Ok(Rc::new(RefCell::new(driver)))
    }
}

fn start(script: &'static str) -> Server {
    modules::server::serve("127.0.0.1:0", 1, boot(script)).expect("server starts")
}

struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Client {
    fn to(server: &Server) -> Self {
        let stream = TcpStream::connect(server.addr()).expect("connect");
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        Client {
            stream,
            buffer: Vec::new(),
        }
    }

    fn send(&mut self, text: &str) {
        self.stream.write_all(text.as_bytes()).expect("write");
    }

    fn response(&mut self) -> (String, String) {
        let head_end = loop {
            if let Some(index) = find(&self.buffer, b"\r\n\r\n") {
                break index;
            }
            self.fill();
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
            self.fill();
        }
        let rest = self.buffer.split_off(length);
        let body = String::from_utf8_lossy(&self.buffer).to_string();
        self.buffer = rest;
        (head, body)
    }

    fn fill(&mut self) {
        let mut chunk = [0u8; 8192];
        let read = self.stream.read(&mut chunk).expect("read");
        assert!(read > 0, "server closed the connection");
        self.buffer.extend_from_slice(&chunk[..read]);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[test]
fn a_synchronous_handler_answers() {
    let server = start(
        r#"
        Xmas.serve(function (request) {
          return new Response('hello ' + new URL(request.url).pathname);
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET /hi?x=1 HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "hello /hi");
}

#[test]
fn a_promise_answering_handler_gets_there() {
    let server = start(
        r#"
        Xmas.serve(async function (request) {
          await new Promise((resolve) => setTimeout(resolve, 10));
          return new Response('late', { status: 201 });
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET /slow HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 201"), "{head}");
    assert_eq!(body, "late");
}

#[test]
fn the_request_body_and_headers_reach_the_handler() {
    let server = start(
        r#"
        Xmas.serve(async function (request) {
          const text = await request.text();
          return new Response(request.method + ' ' + request.headers.get('x-test') + ' [' + text + ']');
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send(
        "POST /echo HTTP/1.1\r\nHost: t\r\nX-Test: 42\r\nContent-Length: 5\r\n\r\nhello",
    );
    let (_, body) = client.response();
    assert_eq!(body, "POST 42 [hello]");
}

#[test]
fn response_headers_survive_the_trip() {
    let server = start(
        r#"
        Xmas.serve(function () {
          return new Response('ok', { headers: { 'x-made-by': 'js', 'content-type': 'text/plain' } });
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, _) = client.response();
    let lower = head.to_ascii_lowercase();
    assert!(lower.contains("x-made-by: js"), "{head}");
    assert!(lower.contains("content-type: text/plain"), "{head}");
}

#[test]
fn a_handler_that_throws_becomes_a_500() {
    let server = start(
        r#"
        Xmas.serve(function () {
          throw new Error('nope');
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 500"), "{head}");
    assert!(body.contains("nope"), "{body}");
}

#[test]
fn a_handler_that_returns_something_else_becomes_a_500() {
    let server = start(
        r#"
        Xmas.serve(function () {
          return { status: 200 };
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 500"), "{head}");
    assert!(body.contains("not a Response"), "{body}");
}

#[test]
fn one_realm_serves_a_whole_connection() {
    let server = start(
        r#"
        let seen = 0;
        Xmas.serve(function () {
          seen = seen + 1;
          return new Response('request ' + seen);
        });
        "#,
    );
    let mut client = Client::to(&server);
    client.send("GET /1 HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, first) = client.response();
    client.send("GET /2 HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, second) = client.response();
    assert_eq!(first, "request 1");
    assert_eq!(second, "request 2");
}
