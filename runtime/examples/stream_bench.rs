//! Microbenchmark for the stream core: what a chunk costs on the way through.
//!
//! cargo run -p xmas-runtime --release --example stream_bench

use xmas_runtime::testkit::Harness;

const SETUP: &str = r#"
globalThis.__bench = { push_us: 0, read_us: 0, got: 0, result: null };

const makeChunk = (size, text) => (text ? 'x'.repeat(size) : Buffer.alloc(size, 97));

// Push n chunks into a Readable and drain them in the same tick.
__bench.pushRead = (n, size, text) => {
  const chunk = makeChunk(size, text);
  const readable = new stream.Readable();
  const before = __now_us();
  for (let i = 0; i < n; i += 1) readable.push(chunk);
  const pushed = __now_us();
  let got = 0;
  while (readable.read() !== null) got += 1;
  const drained = __now_us();
  __bench.push_us = pushed - before;
  __bench.read_us = drained - pushed;
  __bench.got = got;
};

// The whole path: source -> PassThrough -> writable sink, events included.
__bench.pipeStart = (n, size, text) => {
  const chunk = makeChunk(size, text);
  let received = 0;
  let chunks = 0;
  const started = __now_us();
  const sink = new stream.Writable({
    write(data, encoding, done) { received += data.length; chunks += 1; done(); },
  });
  const pass = new stream.PassThrough();
  const source = new stream.Readable();
  __bench.result = null;
  sink.on('finish', () => { __bench.result = [started, __now_us(), received, chunks]; });
  source.pipe(pass).pipe(sink);
  for (let i = 0; i < n; i += 1) source.push(chunk);
  source.push(null);
};

// What one event emit costs, and what a Buffer view over an ArrayBuffer costs,
// so the per-chunk overhead can be split between the two.
__bench.emitCost = (n) => {
  const emitter = new globalThis.EventEmitter();
  emitter.on('x', (value) => value);
  const started = __now_us();
  for (let i = 0; i < n; i += 1) emitter.emit('x', i);
  __bench.emit_us = __now_us() - started;
};

__bench.viewCost = (n) => {
  const buffer = new ArrayBuffer(4096);
  const started = __now_us();
  let total = 0;
  for (let i = 0; i < n; i += 1) total += Buffer.from(buffer).length;
  __bench.view_us = __now_us() - started;
};

// One stage only: source -> sink, to see what a second stage adds.
__bench.singleStart = (n, size) => {
  const chunk = Buffer.alloc(size, 97);
  let received = 0;
  const started = __now_us();
  const sink = new stream.Writable({
    write(data, encoding, done) { received += data.length; done(); },
  });
  const source = new stream.Readable();
  __bench.single = null;
  sink.on('finish', () => { __bench.single = [started, __now_us(), received]; });
  source.pipe(sink);
  for (let i = 0; i < n; i += 1) source.push(chunk);
  source.push(null);
};

// Control: the same 4 MB of Buffers allocated and dropped, no streams at all,
// to see what the heap cap alone costs.
__bench.allocate = (n, size) => {
  const started = __now_us();
  let total = 0;
  for (let i = 0; i < n; i += 1) total += Buffer.alloc(size, 97).length;
  __bench.alloc_us = __now_us() - started;
  __bench.alloc_total = total;
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
        .unwrap_or(64 * 1024 * 1024);
    println!("heap cap: {} MiB", heap / (1024 * 1024));
    let mut h = Harness::with_heap_and_modules(heap);
    h.eval(SETUP).expect("setup");

    println!("== push + read");
    for (n, size, text) in [
        (20_000.0_f64, 4096.0_f64, false),
        (5_000.0, 4096.0, false),
        (20_000.0, 4096.0, true),
        (20_000.0, 64.0, false),
    ] {
        h.eval(&format!("__bench.pushRead({n}, {size}, {text})"))
            .expect("pushRead");
        let push_us = h.expect_number("__bench.push_us");
        let read_us = h.expect_number("__bench.read_us");
        let got = h.expect_number("__bench.got");
        let bytes = got * size;
        let kind = if text { "string" } else { "buffer" };
        println!("--- {got:.0} chunks of {size:.0} B {kind}");
        line("push", push_us, got, bytes);
        line("read", read_us, got, bytes);
    }

    println!("== per-operation costs");
    for (n, name) in [(100_000.0_f64, "emit"), (100_000.0, "view")] {
        let call = if name == "emit" {
            format!("__bench.emitCost({n})")
        } else {
            format!("__bench.viewCost({n})")
        };
        h.eval(&call).expect("cost");
        let label = if name == "emit" { "__bench.emit_us" } else { "__bench.view_us" };
        let micros = h.expect_number(label);
        line(name, micros, n, 0.0);
    }

    h.eval("__bench.singleStart(20000, 4096)").expect("single");
    h.driver().run_until_idle_blocking().expect("pump");
    let started = h.expect_number("__bench.single[0]");
    let ended = h.expect_number("__bench.single[1]");
    let received = h.expect_number("__bench.single[2]");
    println!("--- 20000 chunks of 4096 B, one stage (no PassThrough)");
    line("pipe1", ended - started, 20000.0, received);

    println!("== allocation control");
    for (n, size) in [(20_000.0_f64, 4096.0_f64)] {
        h.eval(&format!("__bench.allocate({n}, {size})")).expect("allocate");
        let alloc_us = h.expect_number("__bench.alloc_us");
        println!("--- {n:.0} fresh Buffers of {size:.0} B");
        line("alloc", alloc_us, n, n * size);
    }

    println!("== pipe PassThrough");
    for (n, size, text) in [
        (20_000.0_f64, 4096.0_f64, false),
        (5_000.0, 4096.0, false),
        (20_000.0, 4096.0, true),
        (20_000.0, 64.0, false),
    ] {
        h.eval(&format!("__bench.pipeStart({n}, {size}, {text})"))
            .expect("pipeStart");
        h.driver().run_until_idle_blocking().expect("pump");
        let started = h.expect_number("__bench.result[0]");
        let ended = h.expect_number("__bench.result[1]");
        let received = h.expect_number("__bench.result[2]");
        let kind = if text { "string" } else { "buffer" };
        println!("--- {n:.0} chunks of {size:.0} B {kind}");
        line("pipe", ended - started, n, received);
    }
}
