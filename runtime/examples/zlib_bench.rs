//! What compression costs, per call, in one run.
//!
//! cargo run -p xmas-runtime --release --example zlib_bench

use std::time::Instant;

use xmas_runtime::testkit::{number_of, Harness};

fn measure(harness: &mut Harness, label: &str, body: &str, calls: u32) {
    harness.eval(body).unwrap();
    let started = Instant::now();
    harness
        .eval(&format!("for (let i = 0; i < {calls}; i += 1) {{ __bench(); }}"))
        .unwrap();
    let micros = started.elapsed().as_secs_f64() * 1e6 / f64::from(calls);
    println!("{label:<38} {micros:>9.1} us/call");
}

fn main() {
    let mut harness = Harness::with_modules();
    harness
        .eval(
            "const text = 'the quick brown fox jumps over the lazy dog. '.repeat(1000);
             const small = Buffer.from('hello hello hello');
             const gzipBig = Buffer.from(__zlib_module.gzipSync(Buffer.from(text)));
             const gzipSmall = Buffer.from(__zlib_module.gzipSync(small));
             const brotliBig = Buffer.from(__zlib_module.brotliCompressSync(Buffer.from(text)));",
        )
        .unwrap();
    let size = harness.expect_number("text.length");
    println!("payload {size} bytes of text\n");

    measure(&mut harness, "gzipSync small", "globalThis.__bench = () => __zlib_module.gzipSync(small);", 20_000);
    measure(&mut harness, "gunzipSync small", "globalThis.__bench = () => __zlib_module.gunzipSync(gzipSmall);", 20_000);
    measure(&mut harness, "gzipSync 45 KiB", "globalThis.__bench = () => __zlib_module.gzipSync(Buffer.from(text));", 200);
    measure(&mut harness, "gunzipSync 45 KiB", "globalThis.__bench = () => __zlib_module.gunzipSync(gzipBig);", 500);
    measure(&mut harness, "brotliCompressSync 45 KiB", "globalThis.__bench = () => __zlib_module.brotliCompressSync(Buffer.from(text));", 50);
    measure(&mut harness, "brotliDecompressSync 45 KiB", "globalThis.__bench = () => __zlib_module.brotliDecompressSync(brotliBig);", 500);
    measure(&mut harness, "crc32 45 KiB", "globalThis.__bench = () => __zlib_module.crc32(Buffer.from(text));", 1000);

    // For scale: one JS call and one 45 KiB copy through the bytes bridge.
    measure(&mut harness, "empty call", "globalThis.__bench = () => 0;", 200_000);
    measure(&mut harness, "Buffer.from(ArrayBuffer) 45 KiB", "globalThis.__bench = () => Buffer.from(gzipBig);", 20_000);

    assert!(number_of(&harness.eval("1").unwrap()).is_some());
}
