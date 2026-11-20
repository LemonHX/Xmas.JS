//! WebSocket over a real socket, spoken by hand.
//!
//! The client here does the handshake and the framing itself, so what it tests is the server, not a
//! second copy of our own client code. The accept value is the one from the RFC's own example.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::{find, start};
use xmas_runtime::host::server::Server;

const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

const ECHO: &str = r#"
Xmas.serve(function (request) {
  if (request.headers.get('upgrade') === null) {
    return new Response('plain');
  }
  const { socket, response } = Xmas.upgradeWebSocket(request);
  socket.onmessage = function (event) {
    if (typeof event.data === 'string') {
      socket.send('echo:' + event.data);
    } else {
      socket.send(event.data);
    }
  };
  socket.onclose = function () {};
  return response;
});
"#;

struct Ws {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Ws {
    fn connect(server: &Server) -> (String, Ws) {
        let mut stream = TcpStream::connect(server.addr()).expect("connect");
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        let handshake = format!(
            "GET /ws HTTP/1.1\r\nHost: t\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(handshake.as_bytes()).expect("write");
        let mut ws = Ws {
            stream,
            buffer: Vec::new(),
        };
        let head_end = ws.until(b"\r\n\r\n");
        let head = String::from_utf8_lossy(&ws.buffer[..head_end]).to_string();
        ws.buffer.drain(..head_end + 4);
        (head, ws)
    }

    /// A client frame is always masked, which is what makes the server unmask it.
    fn send(&mut self, opcode: u8, payload: &[u8], fin: bool) {
        let mut frame = Vec::with_capacity(payload.len() + 14);
        frame.push(if fin { 0x80 | opcode } else { opcode });
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mask = [0x37u8, 0xfa, 0x21, 0x3d];
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        self.stream.write_all(&frame).expect("write frame");
    }

    fn text(&mut self, text: &str) {
        self.send(0x1, text.as_bytes(), true);
    }

    /// One frame from the server: the server never masks, so no key to strip.
    fn read(&mut self) -> (u8, Vec<u8>) {
        let head = self.at_least(2);
        let opcode = head[0] & 0x0f;
        assert_eq!(head[1] & 0x80, 0, "a server frame must not be masked");
        let short = (head[1] & 0x7f) as usize;
        let (len, extra) = match short {
            126 => {
                let bytes = self.at_least(4);
                (u16::from_be_bytes([bytes[2], bytes[3]]) as usize, 2)
            }
            127 => {
                let bytes = self.at_least(10);
                let mut wide = [0u8; 8];
                wide.copy_from_slice(&bytes[2..10]);
                (u64::from_be_bytes(wide) as usize, 8)
            }
            other => (other, 0),
        };
        let total = 2 + extra + len;
        self.at_least(total);
        let payload = self.buffer[2 + extra..total].to_vec();
        self.buffer.drain(..total);
        (opcode, payload)
    }

    /// Read until the peer closes, reporting whatever came back.
    fn drained(&mut self) -> bool {
        let mut chunk = [0u8; 1024];
        matches!(self.stream.read(&mut chunk), Ok(0) | Err(_))
    }

    /// Have at least n bytes ready, and hand back a copy of the first n.
    fn at_least(&mut self, n: usize) -> Vec<u8> {
        while self.buffer.len() < n {
            let mut chunk = [0u8; 8192];
            let read = self.stream.read(&mut chunk).expect("read");
            assert!(read > 0, "the server closed the connection");
            self.buffer.extend_from_slice(&chunk[..read]);
        }
        self.buffer[..n].to_vec()
    }

    fn until(&mut self, needle: &[u8]) -> usize {
        loop {
            if let Some(index) = find(&self.buffer, needle) {
                return index;
            }
            let mut chunk = [0u8; 8192];
            let read = self.stream.read(&mut chunk).expect("read");
            assert!(read > 0, "the server closed the connection during the handshake");
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }
}

#[test]
fn the_handshake_is_answered_with_the_rfc_accept_value() {
    let server = start(ECHO);
    let (head, _ws) = Ws::connect(&server);
    let lower = head.to_ascii_lowercase();
    assert!(lower.starts_with("http/1.1 101"), "{head}");
    assert!(lower.contains("upgrade: websocket"), "{head}");
    assert!(lower.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="), "{head}");
}

#[test]
fn a_text_message_is_answered_through_the_handler() {
    let server = start(ECHO);
    let (_head, mut ws) = Ws::connect(&server);
    ws.text("hello");
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x1);
    assert_eq!(String::from_utf8_lossy(&payload), "echo:hello");
}

#[test]
fn a_binary_message_comes_back_as_binary() {
    let server = start(ECHO);
    let (_head, mut ws) = Ws::connect(&server);
    ws.send(0x2, &[0, 1, 2, 250], true);
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x2);
    assert_eq!(payload, vec![0, 1, 2, 250]);
}

#[test]
fn a_fragmented_message_is_put_back_together() {
    let server = start(ECHO);
    let (_head, mut ws) = Ws::connect(&server);
    ws.send(0x1, b"hel", false);
    ws.send(0x0, b"lo", true);
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x1);
    assert_eq!(String::from_utf8_lossy(&payload), "echo:hello");
}

#[test]
fn a_ping_is_answered_with_the_same_payload() {
    let server = start(ECHO);
    let (_head, mut ws) = Ws::connect(&server);
    ws.send(0x9, b"beep", true);
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0xA, "expected a pong");
    assert_eq!(String::from_utf8_lossy(&payload), "beep");
}

#[test]
fn a_close_frame_is_answered_and_the_connection_ends() {
    let server = start(ECHO);
    let (_head, mut ws) = Ws::connect(&server);
    let mut payload = Vec::new();
    payload.extend_from_slice(&1000u16.to_be_bytes());
    ws.send(0x8, &payload, true);
    let (opcode, echoed) = ws.read();
    assert_eq!(opcode, 0x8);
    assert_eq!(u16::from_be_bytes([echoed[0], echoed[1]]), 1000);
    assert!(ws.drained(), "the connection stayed open after the close");
}

const CLOSER: &str = r#"
Xmas.serve(function (request) {
  const { socket, response } = Xmas.upgradeWebSocket(request);
  socket.onopen = function () {
    setTimeout(function () { socket.close(1000, 'done'); }, 10);
  };
  return response;
});
"#;

#[test]
fn the_handler_can_close_the_socket() {
    let server = start(CLOSER);
    let (_head, mut ws) = Ws::connect(&server);
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x8, "expected a close frame from the handler");
    assert_eq!(u16::from_be_bytes([payload[0], payload[1]]), 1000);
    assert_eq!(String::from_utf8_lossy(&payload[2..]), "done");
}

const SHOUT: &str = r#"
Xmas.serve(function (request) {
  if (request.headers.get('upgrade') === null) return new Response('still http');
  const { socket, response } = Xmas.upgradeWebSocket(request);
  socket.onopen = function () { socket.send('onopen ran'); };
  socket.onmessage = function (event) { socket.send('got ' + event.data); };
  return response;
});
"#;

#[test]
fn the_socket_says_when_it_is_open() {
    let server = start(SHOUT);
    let (_head, mut ws) = Ws::connect(&server);
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x1);
    assert_eq!(String::from_utf8_lossy(&payload), "onopen ran");
    ws.text("more");
    let (opcode, payload) = ws.read();
    assert_eq!(opcode, 0x1);
    assert_eq!(String::from_utf8_lossy(&payload), "got more");
}

#[test]
fn a_request_that_is_not_an_upgrade_is_still_http() {
    let server = start(SHOUT);
    let mut stream = TcpStream::connect(server.addr()).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET /plain HTTP/1.1\r\nHost: t\r\n\r\n")
        .expect("write");
    // The connection stays open, so read until the answer is all there rather than to the end.
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while find(&buffer, b"still http").is_none() {
        let read = stream.read(&mut chunk).expect("read");
        assert!(read > 0, "the server closed the connection");
        buffer.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&buffer).to_string();
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.ends_with("still http"), "{text}");
}
