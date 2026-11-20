//! process: environment, arguments, working directory, clocks, stdio and exit.
//!
//! Everything that does not change during a run is handed to JS once, at install
//! time, as JSON: reading process.env.HOME or process.argv[2] afterwards is a
//! property read in the engine and costs no host call. Only the things that
//! genuinely change cross the boundary: cwd, chdir, hrtime, memoryUsage, stdio
//! writes and exit.

use std::cell::Cell;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Stream, Value};

use crate::driver::Driver;
use crate::host::env::Env;
use crate::modules::{node_arch, node_platform, quote};

const PRELUDE: &str = include_str!("../../js/process.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    let env = driver.host.env.clone();
    {
        let env = env.clone();
        driver
            .context()
            .define_function(&global, "__process_meta", 0, move |_: &mut Scope<'_>| {
                text(&meta(&env))
            })
            .map_err(|error| format!("__process_meta: {error}"))?;
    }

    {
        let env = env.clone();
        driver
            .context()
            .define_function(&global, "__process_cwd", 0, move |_: &mut Scope<'_>| {
                text(&env.cwd().to_string_lossy())
            })
            .map_err(|error| format!("__process_cwd: {error}"))?;
    }

    {
        let env = env.clone();
        driver
            .context()
            .define_function(
                &global,
                "__process_chdir",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    let target = std::path::PathBuf::from(&path);
                    std::env::set_current_dir(&target)
                        .map_err(|error| super::throw(scope, &error.to_string()))?;
                    env.set_cwd(std::env::current_dir().unwrap_or(target));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__process_chdir: {error}"))?;
    }

    {
        let env = env.clone();
        driver
            .context()
            .define_function(
                &global,
                "__process_set_env",
                2,
                move |_: &mut Scope<'_>, key: String, value: String| {
                    env.set(&key, &value);
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__process_set_env: {error}"))?;
    }

    {
        let env = env.clone();
        driver
            .context()
            .define_function(
                &global,
                "__process_del_env",
                1,
                move |_: &mut Scope<'_>, key: String| {
                    env.remove(&key);
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__process_del_env: {error}"))?;
    }

    {
        let clock = driver.host.clock.clone();
        driver
            .context()
            .define_function(&global, "__process_now_ns", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(clock.elapsed_ns()))
            })
            .map_err(|error| format!("__process_now_ns: {error}"))?;
    }

    {
        let clock = driver.host.clock.clone();
        driver
            .context()
            .define_function(&global, "__process_uptime", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(clock.elapsed_ns() / 1e9))
            })
            .map_err(|error| format!("__process_uptime: {error}"))?;
    }

    {
        let heap_used = driver.heap_used();
        let cap = driver.heap_cap;
        driver
            .context()
            .define_function(&global, "__process_memory", 0, move |_: &mut Scope<'_>| {
                text(&memory(&heap_used, cap))
            })
            .map_err(|error| format!("__process_memory: {error}"))?;
    }

    {
        let console = driver.host.console.clone();
        driver
            .context()
            .define_function(
                &global,
                "__process_write",
                2,
                move |_: &mut Scope<'_>, stream: String, body: String| {
                    let stream = if stream == "stderr" {
                        Stream::Stderr
                    } else {
                        Stream::Stdout
                    };
                    console.raw(stream, body.as_bytes());
                    Ok::<Value, RuntimeError>(Value::Bool(true))
                },
            )
            .map_err(|error| format!("__process_write: {error}"))?;
    }

    {
        use std::io::IsTerminal;
        driver
            .context()
            .define_function(
                &global,
                "__process_isatty",
                1,
                move |_: &mut Scope<'_>, fd: f64| {
                    let answer = match fd as i32 {
                        0 => std::io::stdin().is_terminal(),
                        1 => std::io::stdout().is_terminal(),
                        2 => std::io::stderr().is_terminal(),
                        _ => false,
                    };
                    Ok::<Value, RuntimeError>(Value::Bool(answer))
                },
            )
            .map_err(|error| format!("__process_isatty: {error}"))?;
    }

    {
        let console = driver.host.console.clone();
        driver
            .context()
            .define_function(
                &global,
                "__process_exit",
                1,
                move |scope: &mut Scope<'_>, code: f64| -> Result<Value, RuntimeError> {
                    let code = code as i32;
                    // Under a capturing host (tests, benchmarks) exiting would take
                    // the harness with it, so it raises instead and the test can
                    // assert on it. A real run exits the process.
                    if console.is_captured() {
                        return Err(super::throw(scope, &format!("process.exit({code})")));
                    }
                    std::process::exit(code);
                },
            )
            .map_err(|error| format!("__process_exit: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("process prelude: {error}"))?;

    let process = driver
        .eval("globalThis.process")
        .map_err(|error| format!("process object: {error}"))?;
    let names = [
        "env",
        "argv",
        "argv0",
        "execPath",
        "platform",
        "arch",
        "pid",
        "ppid",
        "version",
        "versions",
        "cwd",
        "chdir",
        "nextTick",
        "hrtime",
        "uptime",
        "memoryUsage",
        "exit",
        "stdout",
        "stderr",
        "on",
        "off",
        "once",
        "emit",
    ];
    let mut exports = vec![("default".to_string(), process.clone())];
    exports.extend(super::collect(driver, &process, &names)?);
    super::define_with_alias(driver, "process", exports)?;
    Ok(())
}

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body =
        JsString::try_from_utf8(body).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

/// What does not change: paths, ids, versions and the environment snapshot.
fn meta(env: &Env) -> String {
    let args = env.args();
    let mut out = String::with_capacity(512);
    out.push('{');
    out.push_str("\"execPath\":");
    out.push_str(&quote(
        &std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ));
    out.push_str(",\"platform\":");
    out.push_str(&quote(node_platform()));
    out.push_str(",\"arch\":");
    out.push_str(&quote(node_arch()));
    out.push_str(&format!(",\"pid\":{}", std::process::id()));
    // No parent process to name: a realm is not a child of anything a script
    // can reach.
    out.push_str(",\"ppid\":0");
    out.push_str(",\"version\":");
    out.push_str(&quote(concat!("v", env!("CARGO_PKG_VERSION"))));
    out.push_str(",\"argv\":[");
    for (index, argument) in args.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&quote(argument));
    }
    out.push(']');
    out.push_str(",\"env\":{");
    for (index, (key, value)) in env.all().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&quote(key));
        out.push(':');
        out.push_str(&quote(value));
    }
    out.push('}');
    out.push_str(",\"versions\":{");
    out.push_str(&format!("\"xmas\":{}", quote(env!("CARGO_PKG_VERSION"))));
    out.push('}');
    out.push('}');
    out
}

/// memoryUsage: the heap numbers come from the engine's own report, the
/// resident part from the kernel. external and arrayBuffers are reported as
/// zero until the engine tracks them separately.
fn memory(heap_used: &Rc<Cell<usize>>, cap: usize) -> String {
    // Every number is the realm's own heap. Resident memory belongs to the
    // machine, not to this realm, so it is not reported: rss is the heap the
    // engine has reserved. external and arrayBuffers stay zero because buffers
    // and typed arrays are allocated inside that same heap here.
    let used = heap_used.get();
    format!(
        "{{\"rss\":{used},\"heapTotal\":{cap},\"heapUsed\":{used},\"external\":0,\"arrayBuffers\":0}}"
    )
}
