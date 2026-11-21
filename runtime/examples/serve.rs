//! A server to measure against: one realm per worker, a JSON answer for every path.
//!
//! PORT and WORKERS come from the environment. This is the stand-in used to compare against the
//! probe and, later, against other runtimes; it is not a production entry point.

use std::cell::RefCell;
use std::rc::Rc;

use xmas_runtime::driver::Driver;
use xmas_runtime::entry;
use xmas_runtime::host::Host;
use xmas_runtime::modules;

const SCRIPT: &str = r#"
import { serve } from 'xmas:server';
let answered = 0;
serve(function (request) {
  answered = answered + 1;
  const path = new URL(request.url).pathname;
  return new Response('{"ok":true,"n":' + answered + ',"path":"' + path + '"}', {
    headers: { 'content-type': 'application/json' },
  });
});
"#;

fn boot(_index: usize) -> Result<Rc<RefCell<Driver>>, String> {
    // No listener address here: the script registers a handler and never asks for a port.
    let mut driver = entry::realm(Host::new(), "")?;
    let namespace = entry::evaluate_named(&mut driver, "serve.ts", SCRIPT)?;
    entry::resolve_handler(&mut driver, namespace)?;
    Ok(Rc::new(RefCell::new(driver)))
}

/// With NOJS set, answer from Rust: this is the floor the realm bridge is measured against.
fn rust_only(addr: &str, workers: usize) -> xmas_runtime::host::server::Server {
    use std::future::Future;
    use std::pin::Pin;
    use trillium::Conn;
    type Answer = Pin<Box<dyn Future<Output = Conn>>>;
    xmas_runtime::host::server::serve(addr, workers, |_, _| {
        let handler = |conn: Conn| -> Answer { Box::pin(async move { conn.ok(BODY) }) };
        (handler, xmas_runtime::host::server::no_upgrade())
    })
    .expect("server starts")
}

const BODY: &str = "{\"ok\":true}";

fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8787".to_string());
    let workers: usize = std::env::var("WORKERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let addr = format!("127.0.0.1:{port}");

    if std::env::var_os("NOJS").is_some() {
        let server = rust_only(&addr, workers);
        println!("listening on {} with {workers} worker(s), no js", server.addr());
        std::thread::park();
    }

    let server = match modules::server::serve(&addr, workers, boot) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!("listening on {} with {workers} worker(s)", server.addr());
    std::thread::park();
}
