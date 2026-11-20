//! What moving bytes through connect() costs: 64 MiB each way over a socket on localhost.
//!
//! cargo run -p xmas-runtime --release --example sockets_bench

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};

use xmas_runtime::testkit::Harness;

const CHUNK: usize = 4096;
const COUNT: usize = 16384;

/// Read and throw away, until the client goes away.
fn sink() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut buffer = vec![0u8; 64 * 1024];
        while let Ok(read) = socket.read(&mut buffer) {
            if read == 0 {
                return;
            }
        }
    });
    port
}

/// Write this much, then close.
fn source(total: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let block = vec![b'a'; CHUNK];
        let mut written = 0;
        while written < total {
            if socket.write_all(&block).is_err() {
                return;
            }
            written += CHUNK;
        }
        let _ = socket.shutdown(Shutdown::Both);
    });
    port
}

fn main() {
    let mut h = Harness::with_modules();
    h.module(
        "app",
        "import { connect } from 'cloudflare:sockets';\nglobalThis.connect = connect;\n",
    )
    .expect("module");

    let sink_port = sink();
    h.eval(&format!(
        "(async () => {{
            const chunk = Buffer.alloc({CHUNK}, 97);
            const socket = connect({{ hostname: '127.0.0.1', port: {sink_port} }});
            const writer = socket.writable.getWriter();
            const started = performance.now();
            for (let i = 0; i < {COUNT}; i += 1) {{
                await writer.write(chunk);
            }}
            globalThis.write_ms = performance.now() - started;
            writer.close();
            globalThis.write_done = 'yes';
        }})();"
    ))
    .expect("start");
    drive(&mut h, "write_done");
    let write_ms = h.expect_number("Number(globalThis.write_ms)");

    let source_port = source(COUNT * CHUNK);
    h.eval(&format!(
        "(async () => {{
            const socket = connect({{ hostname: '127.0.0.1', port: {source_port} }});
            const reader = socket.readable.getReader();
            const started = performance.now();
            let total = 0;
            for (;;) {{
                const step = await reader.read();
                if (step.done) {{
                    break;
                }}
                total += step.value.length;
            }}
            globalThis.read_ms = performance.now() - started;
            globalThis.read_bytes = total;
            globalThis.read_done = 'yes';
        }})();"
    ))
    .expect("start");
    drive(&mut h, "read_done");
    let read_ms = h.expect_number("Number(globalThis.read_ms)");
    let read_bytes = h.expect_number("Number(globalThis.read_bytes)");

    let total = (COUNT * CHUNK) as f64;
    report("write through connect()", write_ms, total);
    report("read through connect()", read_ms, read_bytes);
}

fn drive(h: &mut Harness, flag: &str) {
    for _ in 0..4000 {
        h.driver().run_until_idle_blocking().expect("pump");
        if h.expect_text(&format!("String(globalThis.{flag})")) == "yes" {
            return;
        }
    }
    panic!("timed out waiting for {flag}");
}

fn report(label: &str, millis: f64, bytes: f64) {
    let seconds = millis / 1000.0;
    let chunks = bytes / CHUNK as f64;
    println!(
        "{label:<24} {:>8.1} ms  {:>7.1} MiB/s  {:>6.2} us/4KiB chunk",
        millis,
        bytes / seconds / (1024.0 * 1024.0),
        seconds * 1_000_000.0 / chunks
    );
}
