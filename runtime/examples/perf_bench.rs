//! What the performance clock and AsyncLocalStorage cost, per call.
//!
//! cargo run -p xmas-runtime --release --example perf_bench

use std::time::Instant;

use xmas_runtime::testkit::Harness;

fn measure(harness: &mut Harness, label: &str, body: &str, calls: u32) {
    harness.eval(body).unwrap();
    let started = Instant::now();
    harness
        .eval(&format!(
            "for (let i = 0; i < {calls}; i += 1) {{ __bench(); }}"
        ))
        .unwrap();
    let ns = started.elapsed().as_secs_f64() * 1e9 / f64::from(calls);
    println!("{label:<40} {ns:>8.1} ns/call");
}

fn main() {
    let mut harness = Harness::with_modules();
    harness
        .eval(
            "const { AsyncLocalStorage } = globalThis.__async_hooks_module;
             const storage = new AsyncLocalStorage();
             globalThis.__bench = () => 0;",
        )
        .unwrap();

    measure(
        &mut harness,
        "empty call (the floor)",
        "globalThis.__bench = () => 0;",
        500_000,
    );
    measure(
        &mut harness,
        "performance.now()",
        "const now = performance.now; globalThis.__bench = () => now();",
        500_000,
    );
    // The entry lists grow with every call, and a lookup walks them, so the
    // loop clears what it added: what is timed is one call on a short list.
    measure(
        &mut harness,
        "performance.mark('m')",
        "globalThis.__bench = () => { performance.mark('m'); performance.clearMarks(); };",
        50_000,
    );
    measure(
        &mut harness,
        "measure() with numeric start and duration",
        "globalThis.__bench = () => {
           performance.measure('x', { start: 0, duration: 1 });
           performance.clearMeasures();
         };",
        50_000,
    );
    measure(
        &mut harness,
        "measure() between two mark names",
        "performance.mark('a'); performance.mark('b');
         globalThis.__bench = () => {
           performance.measure('x', 'a', 'b');
           performance.clearMeasures();
         };",
        50_000,
    );
    measure(
        &mut harness,
        "getEntriesByName on 1000 marks",
        "performance.clearMarks();
         for (let i = 0; i < 1000; i += 1) performance.mark('n' + i);
         globalThis.__bench = () => performance.getEntriesByName('n999', 'mark').length;",
        5_000,
    );
    measure(
        &mut harness,
        "storage.run(store, fn) + getStore",
        "globalThis.__bench = () => storage.run('s', () => storage.getStore());",
        500_000,
    );
    measure(
        &mut harness,
        "storage.bind(fn) then call it",
        "const bound = storage.bind(() => storage.getStore());
         globalThis.__bench = () => bound();",
        500_000,
    );
}
