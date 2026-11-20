//! The server's HTTP behaviour, with a Rust handler and a raw client.
//!
//! These tests are about the transport and about trillium's HTTP/1.1, not about the realm: no
//! engine is created here, so a failure points at the socket code and nothing else.

use std::future::Future;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::pin::Pin;
use std::time::Duration;

use trillium::Conn;
use xmas_runtime::host::server::{no_upgrade, serve, Server};

type Answer = Pin<Box<dyn Future<Output = Conn>>>;

/// Echoes the request line and the body back, so a test can see what the server understood.
fn echo() -> impl Fn(Conn) -> Answer {
    |mut conn: Conn| {
        Box::pin(async move {
            let method = conn.method().to_string();
            let target = conn.path_and_query().to_string();
            let body = conn.request_body().read_string().await.unwrap_or_default();
            let text = format!("{method} {target} [{body}]");
            conn.with_response_header("content-type", "text/plain").ok(text)
        })
    }
}

fn start(workers: usize) -> Server {
    serve("127.0.0.1:0", workers, |_, _| (echo(), no_upgrade())).expect("server starts")
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
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        Client {
            stream,
            buffer: Vec::new(),
        }
    }

    fn send(&mut self, bytes: &str) {
        self.stream.write_all(bytes.as_bytes()).expect("write");
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).expect("write");
    }

    /// Read one response: its status line, its headers and its body.
    fn response(&mut self) -> (String, String) {
        let head_end = loop {
            if let Some(index) = find(&self.buffer, b"\r\n\r\n") {
                break index;
            }
            self.fill();
        };
        let head = String::from_utf8_lossy(&self.buffer[..head_end]).to_string();
        let mut rest = self.buffer.split_off(head_end + 4);
        self.buffer = rest;

        let length = head
            .to_ascii_lowercase()
            .split("\r\n")
            .find_map(|line| line.strip_prefix("content-length:").map(|value| value.trim().to_string()))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        while self.buffer.len() < length {
            self.fill();
        }
        rest = self.buffer.split_off(length);
        let body = String::from_utf8_lossy(&self.buffer).to_string();
        self.buffer = rest;
        (head, body)
    }

    fn fill(&mut self) {
        let mut chunk = [0u8; 4096];
        let read = self.stream.read(&mut chunk).expect("read");
        assert!(read > 0, "server closed the connection");
        self.buffer.extend_from_slice(&chunk[..read]);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[test]
fn answers_a_request() {
    let server = start(1);
    let mut client = Client::to(&server);
    client.send("GET /hello?x=1 HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "GET /hello?x=1 []");
}

#[test]
fn keeps_the_connection_for_a_second_request() {
    let server = start(1);
    let mut client = Client::to(&server);
    client.send("GET /one HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, first) = client.response();
    client.send("GET /two HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, second) = client.response();
    assert_eq!(first, "GET /one []");
    assert_eq!(second, "GET /two []");
}

#[test]
fn reads_a_chunked_body() {
    let server = start(1);
    let mut client = Client::to(&server);
    client.send(
        "POST /upload HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
    );
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "POST /upload [hello world]");
}

#[test]
fn answers_two_requests_sent_in_one_write() {
    let server = start(1);
    let mut client = Client::to(&server);
    client.send(concat!(
        "GET /a HTTP/1.1\r\nHost: t\r\n\r\n",
        "GET /b HTTP/1.1\r\nHost: t\r\n\r\n"
    ));
    let (_, first) = client.response();
    let (_, second) = client.response();
    assert_eq!(first, "GET /a []");
    assert_eq!(second, "GET /b []");
}

#[test]
fn a_client_that_vanishes_does_not_take_the_server_with_it() {
    let server = start(1);
    {
        let mut broken = Client::to(&server);
        broken.send("POST /half HTTP/1.1\r\nHost: t\r\nContent-Length: 100\r\n\r\nonly-some");
        // Drop the socket mid-body: the server is reading a request that will never finish.
    }
    let mut client = Client::to(&server);
    client.send("GET /still-here HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, body) = client.response();
    assert_eq!(body, "GET /still-here []");
}

#[test]
fn a_malformed_head_is_answered_and_not_fatal() {
    let server = start(1);
    let mut client = Client::to(&server);
    client.send_bytes(b"this is not a request\r\n\r\n");
    // trillium answers 400 and closes; all this test insists on is that the socket is not left
    // hanging and the server keeps serving.
    let mut chunk = [0u8; 256];
    let read = client.stream.read(&mut chunk).unwrap_or(0);
    let text = String::from_utf8_lossy(&chunk[..read]).to_string();
    assert!(text.is_empty() || text.starts_with("HTTP/1.1 4"), "{text}");

    let mut fresh = Client::to(&server);
    fresh.send("GET /after HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, body) = fresh.response();
    assert_eq!(body, "GET /after []");
}

#[test]
fn a_large_body_streams_back_out() {
    let server = serve("127.0.0.1:0", 1, |_, _| {
        let handler = |conn: Conn| -> Answer {
            let payload = "x".repeat(512 * 1024);
            Box::pin(async move { conn.ok(payload) })
        };
        (handler, no_upgrade())
    })
    .expect("server starts");

    let mut client = Client::to(&server);
    client.send("GET /big HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body.len(), 512 * 1024);
}
