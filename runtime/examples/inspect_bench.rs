//! What util.inspect and util.format cost.
//!
//! Numbers only mean something inside one run (the machine changes clock), so
//! this prints a batch at a time and reports per call. Run with
//! cargo run -p xmas-runtime --release --example inspect_bench

use std::time::Instant;

use xmas_runtime::testkit::{number_of, Harness};

fn measure(harness: &mut Harness, label: &str, body: &str, calls: u32) {
    // once outside the clock: the work the loop repeats must be set up already
    harness.eval(body).unwrap();
    let started = Instant::now();
    harness
        .eval(&format!("for (let i = 0; i < {calls}; i += 1) {{ __bench(); }}"))
        .unwrap();
    let took = started.elapsed();
    let per_call = took.as_secs_f64() * 1e9 / f64::from(calls);
    println!("{label:<34} {:>8.0} ns/call", per_call);
}

fn main() {
    let mut harness = Harness::with_modules();

    let calls = 200_000;
    measure(
        &mut harness,
        "inspect small object",
        "const small = { a: 1, b: 'two' }; globalThis.__bench = () => util.inspect(small);",
        calls,
    );
    measure(
        &mut harness,
        "inspect nested object",
        "const nested = { a: { b: { c: { d: 1 } } } }; globalThis.__bench = () => util.inspect(nested);",
        calls,
    );
    measure(
        &mut harness,
        "inspect array of 100 numbers",
        "const list = Array.from({ length: 100 }, (_, i) => i); globalThis.__bench = () => util.inspect(list);",
        20_000,
    );
    measure(
        &mut harness,
        "inspect string",
        "globalThis.__bench = () => util.inspect('text');",
        calls,
    );
    measure(
        &mut harness,
        "format %s %d",
        "globalThis.__bench = () => util.format('%s is %d', 'age', 30);",
        calls,
    );
    measure(
        &mut harness,
        "format with no spec",
        "globalThis.__bench = () => util.format('plain text');",
        calls,
    );
    measure(
        &mut harness,
        "isDeepStrictEqual on two objects",
        "const one = { a: [1, { b: 2 }] }; const two = { a: [1, { b: 2 }] }; globalThis.__bench = () => util.isDeepStrictEqual(one, two);",
        calls / 2,
    );

    // a number the loop above cannot hide: the cost floor of an empty call
    let started = Instant::now();
    harness
        .eval("globalThis.__bench = () => 0; for (let i = 0; i < 200000; i += 1) __bench();")
        .unwrap();
    println!(
        "{:<34} {:>8.0} ns/call",
        "empty call, for scale",
        started.elapsed().as_secs_f64() * 1e9 / 200_000.0
    );

    assert!(number_of(&harness.eval("1").unwrap()).is_some());
}
