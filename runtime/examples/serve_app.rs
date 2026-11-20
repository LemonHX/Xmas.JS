//! Serve a module from the command line: the shape of "xmas serve", until the cli builds again.
//!
//!   cargo run --example serve_app -- ./app.ts --port 8000 --host 127.0.0.1 --workers 4
//!
//! The entry either exports a fetch handler or calls Xmas.serve; see docs/server.md.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut entry: Option<PathBuf> = None;
    let mut port: u16 = 8000;
    let mut host = String::from("127.0.0.1");
    let mut workers: Option<usize> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => port = value,
                None => return fail("--port needs a number"),
            },
            "--host" => match args.next() {
                Some(value) => host = value,
                None => return fail("--host needs an address"),
            },
            "--workers" | "-w" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => workers = Some(value),
                None => return fail("--workers needs a number"),
            },
            "-h" | "--help" => {
                println!("serve_app <entry> [--port N] [--host ADDR] [--workers N]");
                return ExitCode::SUCCESS;
            }
            other if entry.is_none() => entry = Some(PathBuf::from(other)),
            other => return fail(&format!("unexpected argument: {other}")),
        }
    }

    let Some(entry) = entry else {
        return fail("give the entry module: serve_app ./app.ts");
    };
    let workers = workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
    });

    let addr = format!("{host}:{port}");
    let server = match xmas_runtime::entry::serve_app(&addr, workers, &entry) {
        Ok(server) => server,
        Err(error) => return fail(&error),
    };
    println!(
        "listening on http://{} with {workers} worker{}",
        server.addr(),
        if workers == 1 { "" } else { "s" }
    );

    // Ctrl-C ends the process; the server's threads stop when it is dropped, so park until then.
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    drop(server);
    println!("stopped");
    ExitCode::SUCCESS
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}
