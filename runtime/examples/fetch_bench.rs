//! What one fetch costs against a keep-alive server on localhost.
//!
//! cargo run -p xmas-runtime --release --example fetch_bench

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use xmas_runtime::testkit::Harness;

/// Answer requests on one connection until the client goes away.
fn serve(listener: TcpListener) {
    loop {
        let Ok((mut socket, _)) = listener.accept() else {
            return;
        };
        std::thread::spawn(move || loop {
            if !answer(&mut socket) {
                return;
            }
        });
    }
}

fn answer(socket: &mut TcpStream) -> bool {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match socket.read(&mut byte) {
            Ok(0) => return false,
            Ok(_) => {
                head.push(byte[0]);
                if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
                    break;
                }
            }
            Err(_) => return false,
        }
    }
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok")
        .is_ok()
        && socket.flush().is_ok()
}

const SETUP: &str = r#"
globalThis.__bench = {};

// Sequential requests: one at a time, so the number is the cost of a request and not of
// whatever the server can juggle.
__bench.sequential = async (port, count) => {
  const url = 'http://127.0.0.1:' + port + '/ping';
  const started = __now_us();
  for (let i = 0; i < count; i += 1) {
    const response = await fetch(url);
    await response.text();
  }
  __bench.sequential_result = [__now_us() - started, count];
};

// Concurrent requests: eight in flight, the shape a server handler actually uses.
__bench.concurrent = async (port, count, width) => {
  const url = 'http://127.0.0.1:' + port + '/ping';
  const started = __now_us();
  let next = 0;
  const worker = async () => {
    while (next < count) {
      next += 1;
      const response = await fetch(url);
      await response.text();
    }
  };
  const workers = [];
  for (let i = 0; i < width; i += 1) workers.push(worker());
  await Promise.all(workers);
  __bench.concurrent_result = [__now_us() - started, count];
};
"#;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || serve(listener));

    let mut h = Harness::with_modules();
    h.eval(SETUP).expect("setup");

    for (call, label, width) in [
        ("sequential", "__bench.sequential_result", 1.0_f64),
        ("concurrent", "__bench.concurrent_result", 8.0),
    ] {
        let count = 500.0;
        h.eval(&format!("__bench.{call}({port}, {count}, {width})"))
            .expect("start");
        h.driver().run_until_idle_blocking().expect("pump");
        let micros = h.expect_number(&format!("{label}[0]"));
        let done = h.expect_number(&format!("{label}[1]"));
        println!(
            "{label:<22} {micros:>10.1} us  {:>8.1} us/request  {:>7.0} requests/s",
            micros / done,
            done / (micros / 1_000_000.0)
        );
    }
}
