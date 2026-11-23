//! Load one real file through the runtime's own loader and print what it left on the global.
//!
//! `cargo run -p xmas-runtime --example pm_probe -- <file>` is the shortest way to see whether a
//! package off npm works here: it goes through the same resolve, compile and link the runtime
//! uses, then turns the loop until the module's top-level await has settled.

use xmas_runtime::testkit;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: pm_probe <file>");
    let mut harness = testkit::Harness::with_modules();
    if let Err(error) = harness.import_file(&path) {
        println!("ERR {error}");
        return;
    }
    if let Err(error) = harness.driver().run_until_idle_blocking() {
        println!("ERR loop: {error}");
        return;
    }
    for rejection in harness.driver().take_rejections() {
        println!("REJECTED {rejection}");
    }
    match harness.eval("globalThis.__answer") {
        Ok(value) => println!("ANSWER {}", testkit::text_of(&value).unwrap_or_default()),
        Err(error) => println!("ERR {error}"),
    }
}
