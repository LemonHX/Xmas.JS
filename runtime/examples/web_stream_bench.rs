//! Microbenchmark for stream/web: what a chunk costs on its way through.
//!
//! cargo run -p xmas-runtime --release --example web_stream_bench

use xmas_runtime::testkit::Harness;

const SETUP: &str = r#"
globalThis.__bench = {};

const source = (n, size, close) => new ReadableStream({
  type: "byte",
  start(controller) {
    const chunk = new Uint8Array(size);
    chunk.fill(97);
    for (let i = 0; i < n; i += 1) controller.enqueue(chunk);
    if (close) controller.close();
  },
});

// Queued chunks served to a default reader: the shape fetch response bodies and
// every buffered stream use.
__bench.read = async (n, size) => {
  const stream = source(n, size, true);
  const reader = stream.getReader();
  let total = 0;
  const started = __now_us();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    total += value.length;
  }
  __bench.read_result = [__now_us() - started, total, n];
};

// The same bytes through a BYOB reader, which splits chunks in the native queue.
__bench.byob = async (n, size) => {
  const stream = source(n, size, true);
  const reader = stream.getReader({ mode: "byob" });
  let total = 0;
  const started = __now_us();
  for (;;) {
    const { value, done } = await reader.read(new Uint8Array(size));
    if (done) break;
    total += value.length;
  }
  __bench.byob_result = [__now_us() - started, total, n];
};

// Source -> sink, no transform: the shortest pipeTo there is.
__bench.pipe = async (n, size) => {
  let received = 0;
  const started = __now_us();
  const sink = new WritableStream({ write(chunk) { received += chunk.length; } });
  await source(n, size, true).pipeTo(sink);
  __bench.pipe_result = [__now_us() - started, received, n];
};

// Source -> identity TransformStream -> sink: what one transform stage adds.
__bench.pipeTransform = async (n, size) => {
  let received = 0;
  const transform = new TransformStream();
  const started = __now_us();
  const sink = new WritableStream({ write(chunk) { received += chunk.length; } });
  const piping = transform.readable.pipeTo(sink);
  await source(n, size, true).pipeTo(transform.writable);
  await piping;
  __bench.pipe_transform_result = [__now_us() - started, received, n];
};

// Control: the same array buffered and walked with no streams at all, so the
// stream cost can be read as the difference.
// The write side alone: one chunk through writer.write into a sink that does
// nothing, so the pipe overhead can be split from the two halves.
__bench.write = async (n, size) => {
  const chunk = new Uint8Array(size);
  const stream = new WritableStream({ write() {} });
  const writer = stream.getWriter();
  const started = __now_us();
  for (let i = 0; i < n; i += 1) await writer.write(chunk);
  __bench.write_result = [__now_us() - started, n * size, n];
};

// Control: what one await on an already-settled promise costs here. Every
// promise hop in a stream path pays this.
__bench.awaitCost = async (n) => {
  const started = __now_us();
  for (let i = 0; i < n; i += 1) await Promise.resolve(i);
  __bench.await_result = [__now_us() - started, n, n];
};

__bench.plain = (n, size) => {
  const started = __now_us();
  let total = 0;
  const chunks = [];
  for (let i = 0; i < n; i += 1) chunks.push(new Uint8Array(size));
  for (const chunk of chunks) total += chunk.length;
  __bench.plain_result = [__now_us() - started, total, n];
};
"#;

fn line(label: &str, micros: f64, chunks: f64, bytes: f64) {
    let per_chunk = micros * 1000.0 / chunks;
    let mib = bytes / (1024.0 * 1024.0) / (micros / 1_000_000.0);
    println!("{label:<22} {micros:>10.1} us  {per_chunk:>9.1} ns/chunk  {mib:>8.0} MiB/s");
}

fn main() {
    let heap: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(256 * 1024 * 1024);
    println!("heap cap: {} MiB", heap / (1024 * 1024));
    let mut h = Harness::with_heap_and_modules(heap);
    h.eval(SETUP).expect("setup");

    for (call, label) in [
        ("read", "__bench.read_result"),
        ("byob", "__bench.byob_result"),
        ("pipe", "__bench.pipe_result"),
        ("pipe+transform", "__bench.pipe_transform_result"),
    ] {
        let (n, size) = (20000.0_f64, 4096.0_f64);
        h.eval(&format!(
            "__bench.{}({n}, {size})",
            call.replace("+transform", "Transform")
        ))
        .expect("start");
        h.driver().run_until_idle_blocking().expect("pump");
        let micros = h.expect_number(&format!("{label}[0]"));
        let bytes = h.expect_number(&format!("{label}[1]"));
        let chunks = h.expect_number(&format!("{label}[2]"));
        println!("--- {n:.0} chunks of {size:.0} B");
        line(label, micros, chunks, bytes);
    }

    for (label, call) in [("write", "__bench.write_result")] {
        let _ = label;
        h.eval("__bench.write(20000, 4096)").expect("write");
        h.driver().run_until_idle_blocking().expect("pump");
        println!("--- 20000 chunks of 4096 B");
        line(
            call,
            h.expect_number("__bench.write_result[0]"),
            h.expect_number("__bench.write_result[2]"),
            h.expect_number("__bench.write_result[1]"),
        );
    }

    h.eval("__bench.awaitCost(20000)").expect("await");
    h.driver().run_until_idle_blocking().expect("pump");
    println!("--- control, one await per iteration");
    line(
        "await",
        h.expect_number("__bench.await_result[0]"),
        20000.0,
        0.0,
    );

    h.eval("__bench.plain(20000, 4096)").expect("plain");
    let micros = h.expect_number("__bench.plain_result[0]");
    let bytes = h.expect_number("__bench.plain_result[1]");
    println!("--- control, no streams");
    line("plain array", micros, 20000.0, bytes);
}
