//! sockets against a canned TCP server on localhost: connect, the two streams, the promises and
//! the argument checks, with no network and no test server crate.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};

use xmas_runtime::testkit::Harness;

/// Accept one connection, upper-case everything that arrives, stop at EOF.
fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut buffer = [0u8; 1024];
        loop {
            match socket.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => {
                    let upper = buffer[..read].to_ascii_uppercase();
                    if socket.write_all(&upper).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });
    port
}

/// Accept one connection, say this, then close.
fn say(words: &'static [u8]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        socket.write_all(words).expect("write");
        socket.flush().expect("flush");
        let _ = socket.shutdown(Shutdown::Both);
    });
    port
}

/// A port nothing is listening on.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    port
}

/// Drive the loop until this global reads "yes".
fn run(h: &mut Harness, flag: &str) {
    for _ in 0..200 {
        h.driver().run_until_idle_blocking().expect("pump");
        if h.expect_text(&format!("String(globalThis.{flag})")) == "yes" {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("timed out waiting for {flag}");
}

/// A harness with connect() on the global object, imported the way a script imports it.
fn harness() -> Harness {
    let mut h = Harness::with_modules();
    h.module(
        "app",
        "import { connect } from 'cloudflare:sockets';\nglobalThis.connect = connect;\n",
    )
    .expect("module");
    h
}

#[test]
fn a_socket_writes_and_reads_back() {
    let port = echo();
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            const writer = socket.writable.getWriter();
            await writer.write(Buffer.from('hello'));
            const reader = socket.readable.getReader();
            const first = await reader.read();
            globalThis.got = __bytes_to_text(first.value, 'utf8');
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(got)"), "HELLO");
}

#[test]
fn the_readable_ends_when_the_other_side_does() {
    let port = say(b"bye");
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            const reader = socket.readable.getReader();
            const first = await reader.read();
            globalThis.text = __bytes_to_text(first.value, 'utf8');
            const second = await reader.read();
            globalThis.ended = String(second.done);
            await socket.closed;
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(text)"), "bye");
    assert_eq!(h.expect_text("String(ended)"), "true");
}

#[test]
fn opened_reports_both_ends() {
    let port = say(b"");
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            const info = await socket.opened;
            globalThis.remote = info.remoteAddress;
            globalThis.local = info.localAddress;
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(remote)"), format!("127.0.0.1:{port}"));
    assert!(h.expect_text("String(local)").starts_with("127.0.0.1:"));
}

#[test]
fn a_refused_connection_rejects_opened() {
    let port = closed_port();
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            try {{
                await socket.opened;
                globalThis.message = 'no error';
            }} catch (error) {{
                globalThis.message = String(error.message);
            }}
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_ne!(h.expect_text("String(message)"), "no error");
    assert!(!h.expect_text("String(message)").is_empty());
}

#[test]
fn close_ends_the_socket_for_writing() {
    let port = echo();
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            await socket.opened;
            await socket.close();
            await socket.closed;
            let refused = 'no';
            try {{
                const writer = socket.writable.getWriter();
                await writer.write(Buffer.from('after'));
            }} catch (error) {{
                refused = String(error.name);
            }}
            globalThis.refused = refused;
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(refused)"), "TypeError");
}

#[test]
fn the_address_and_the_options_are_checked() {
    let mut h = harness();
    h.eval(
        "const attempt = (work) => {
            try {
                work();
                return 'no error';
            } catch (error) {
                return String(error.name);
            }
        };
        globalThis.noAddress = attempt(() => connect({}));
        globalThis.zeroPort = attempt(() => connect({ hostname: 'example.com', port: 0 }));
        globalThis.sillyPort = attempt(() => connect({ hostname: 'example.com', port: 70000 }));
        globalThis.noHost = attempt(() => connect({ port: 80 }));
        globalThis.secure = attempt(() => connect({ hostname: 'example.com', port: 443 }, { secureTransport: 'on' }));",
    )
    .expect("eval");
    for name in ["noAddress", "zeroPort", "sillyPort", "noHost", "secure"] {
        assert_eq!(
            h.expect_text(&format!("String({name})")),
            "TypeError",
            "{name}"
        );
    }
}

#[test]
fn both_module_names_give_the_same_connect() {
    let mut h = Harness::with_modules();
    h.module(
        "names",
        "import * as cloudflare from 'cloudflare:sockets';\n\
         import * as plain from 'sockets';\n\
         globalThis.same = String(cloudflare.connect === plain.connect);\n\
         globalThis.kind = typeof cloudflare.connect;\n\
         globalThis.byDefault = typeof cloudflare.default.connect;\n",
    )
    .expect("module");
    assert_eq!(h.expect_text("String(same)"), "true");
    assert_eq!(h.expect_text("String(kind)"), "function");
    assert_eq!(h.expect_text("String(byDefault)"), "function");
}

#[test]
fn start_tls_refuses_instead_of_pretending() {
    let port = say(b"");
    let mut h = harness();
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {port} }});
            try {{
                await socket.startTls();
                globalThis.answer = 'no error';
            }} catch (error) {{
                globalThis.answer = String(error.name);
            }}
            globalThis.done = 'yes';
        }})();"
    ))
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(answer)"), "TypeError");
}
