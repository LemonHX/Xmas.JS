//! The surface xmas:server adds on top of plain fetch: requestIP, stop, and
//! the server's upgrade() feeding the websocket object's open/message/close callbacks.
//!
//! These go through a real realm and a real socket, the same way server_entry.rs and server_ws.rs
//! do, so a failure here points at the JS/Rust boundary (server.js, host/server.rs) and not at
//! something mocked out.

mod common;

use std::time::Duration;

use common::{start, Client};

#[test]
fn request_ip_reports_the_real_peer_address() {
    let server = start(
        "import { serve } from 'xmas:server';
         serve({
           fetch(request, server) {
             const ip = server.requestIP(request);
             return new Response(JSON.stringify(ip));
           },
         });
",
    );
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(body.contains("\"address\":\"127.0.0.1\""), "{body}");
    assert!(body.contains("\"family\":\"IPv4\""), "{body}");
}

#[test]
fn stop_closes_the_listener_so_a_new_connection_is_refused() {
    let server = start(
        "import { serve } from 'xmas:server';
         serve({
           fetch(request, server) {
             if (new URL(request.url).pathname === '/stop') {
               server.stop();
               return new Response('stopping');
             }
             return new Response('ok');
           },
         });
",
    );
    let addr = server.addr();

    let mut client = Client::to(&server);
    client.send("GET /stop HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "stopping");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(addr).is_err() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the listener never closed");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_upgrade_drives_the_websocket_handlers() {
    let server = start(
        "import { serve } from 'xmas:server';
         serve({
           fetch(request, server) {
             if (new URL(request.url).pathname !== '/ws') return new Response('not ws', { status: 404 });
             const ok = server.upgrade(request, { data: { tag: 'hi' } });
             if (!ok) return new Response('upgrade failed', { status: 400 });
             return undefined;
           },
           websocket: {
             open(raw) {
               raw.send('open:' + JSON.stringify(raw.data));
             },
             message(raw, message) {
               raw.send('echo:' + message);
             },
             close(raw) {},
           },
         });
",
    );

    let mut client = Client::to(&server);
    client.send(
        "GET /ws HTTP/1.1\r\nHost: t\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
    );

    let head = client.read_until("\r\n\r\n", Duration::from_secs(5));
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(head.contains("Sec-WebSocket-Accept:"), "{head}");

    // The open handler fires once the handshake is actually done, carrying the data that was
    // attached at upgrade() time.
    let after_open = client.read_until("open:", Duration::from_secs(5));
    assert!(after_open.contains("open:{\"tag\":\"hi\"}"), "{after_open}");
}
