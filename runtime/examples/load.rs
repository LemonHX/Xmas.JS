//! A load client: keep-alive connections, one outstanding request each, rps and p99 at the end.
//!
//! CONNS, REQS, PORT and REQ_PATH come from the environment. Plain std sockets on purpose: it has
//! to be a client the server cannot influence.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    for line in text.split("\r\n") {
        if let Some(rest) = line.strip_prefix("content-length:") {
            return rest.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|value| value.parse().ok()).unwrap_or(8787);
    let conns: usize = std::env::var("CONNS").ok().and_then(|value| value.parse().ok()).unwrap_or(8);
    let reqs: usize = std::env::var("REQS").ok().and_then(|value| value.parse().ok()).unwrap_or(2000);
    let path = std::env::var("REQ_PATH").unwrap_or_else(|_| "/hello".to_string());

    let started = Instant::now();
    let mut threads = Vec::with_capacity(conns);
    for _ in 0..conns {
        let path = path.clone();
        threads.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            stream.set_nodelay(true).ok();
            let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n");
            let mut latencies: Vec<Duration> = Vec::with_capacity(reqs);
            let mut buffer = vec![0u8; 16384];
            for _ in 0..reqs {
                let sent = Instant::now();
                stream.write_all(request.as_bytes()).expect("write");
                let mut filled = 0;
                loop {
                    let read = stream.read(&mut buffer[filled..]).expect("read");
                    assert!(read > 0, "server closed the connection");
                    filled += read;
                    if let Some(index) = header_end(&buffer[..filled]) {
                        let need = index + 4 + content_length(&buffer[..index]);
                        while filled < need {
                            let read = stream.read(&mut buffer[filled..]).expect("read");
                            assert!(read > 0, "server closed the connection");
                            filled += read;
                        }
                        break;
                    }
                }
                latencies.push(sent.elapsed());
            }
            latencies
        }));
    }

    let mut all: Vec<Duration> = Vec::new();
    for thread in threads {
        all.extend(thread.join().expect("join"));
    }
    let elapsed = started.elapsed();
    all.sort();
    let pct = |fraction: f64| all[((all.len() as f64 * fraction) as usize).min(all.len() - 1)];

    println!(
        "requests={} elapsed={:?} rps={:.0} p50={:?} p99={:?}",
        all.len(),
        elapsed,
        all.len() as f64 / elapsed.as_secs_f64(),
        pct(0.50),
        pct(0.99)
    );
}
