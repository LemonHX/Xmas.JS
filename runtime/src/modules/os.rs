//! os: what this runtime knows about itself, shaped after Node's os module.
//!
//! It reports the runtime, not the machine. Nothing here asks the kernel
//! anything: no libc, no sysctl, no /proc. A script therefore cannot learn the
//! host's memory, its load, its kernel version, its hostname or its user ids,
//! which is what makes running unfamiliar code here defensible when there is no
//! permission model to gate it with.
//!
//! The names and shapes stay Node's so code written against Node keeps running.
//! The numbers are ours:
//!
//! - totalmem: this realm's heap cap
//! - freemem: what is left of that cap
//! - uptime: seconds since this realm's clock started
//! - loadavg: zero, because one realm has nothing to average
//! - uid, gid: -1, which is what Node reports where the ids do not exist
//! - hostname: empty, not disclosed
//!
//! What cannot change during a run is read once at install time and frozen into
//! JS. The four that do change stay host calls and return numbers, so a call
//! allocates nothing.

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::Env;
use crate::modules::{node_arch, node_platform, quote};

const PRELUDE: &str = include_str!("../../js/os.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let snapshot = snapshot(&driver.host.env);
        driver
            .context()
            .define_function(&global, "__os_static", 0, move |_: &mut Scope<'_>| {
                text(&snapshot)
            })
            .map_err(|error| format!("__os_static: {error}"))?;
    }

    {
        let cap = driver.heap_cap;
        driver
            .context()
            .define_function(&global, "__os_totalmem", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(cap as f64))
            })
            .map_err(|error| format!("__os_totalmem: {error}"))?;
    }

    {
        let heap_used = driver.heap_used();
        let cap = driver.heap_cap;
        driver
            .context()
            .define_function(&global, "__os_freemem", 0, move |_: &mut Scope<'_>| {
                let left = cap.saturating_sub(heap_used.get());
                Ok::<Value, RuntimeError>(Value::Float(left as f64))
            })
            .map_err(|error| format!("__os_freemem: {error}"))?;
    }

    {
        let clock = driver.host.clock.clone();
        driver
            .context()
            .define_function(&global, "__os_uptime", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(clock.elapsed_ns() / 1e9))
            })
            .map_err(|error| format!("__os_uptime: {error}"))?;
    }

    driver
        .context()
        .define_function(&global, "__os_loadavg", 0, move |_: &mut Scope<'_>| {
            text("[0,0,0]")
        })
        .map_err(|error| format!("__os_loadavg: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("os prelude: {error}"))?;

    let os = driver
        .eval("globalThis.__os")
        .map_err(|error| format!("os object: {error}"))?;
    let names = [
        "platform",
        "arch",
        "type",
        "release",
        "machine",
        "hostname",
        "homedir",
        "tmpdir",
        "endianness",
        "cpus",
        "totalmem",
        "freemem",
        "uptime",
        "loadavg",
        "userInfo",
        "EOL",
        "constants",
    ];
    let mut exports = vec![("default".to_string(), os.clone())];
    exports.extend(super::collect(driver, &os, &names)?);
    super::define_with_alias(driver, "os", exports)?;
    Ok(())
}

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body =
        JsString::try_from_utf8(body).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

/// The part of the answer that cannot change while the process runs.
///
/// Everything coming from the environment is read through the host's snapshot,
/// not std::env, so an embedder that injects a fake environment gets a matching
/// os module.
fn snapshot(env: &Env) -> String {
    let first =
        |keys: &[&str]| -> String { keys.iter().find_map(|key| env.get(key)).unwrap_or_default() };
    let homedir = first(&["HOME", "USERPROFILE"]);
    let user = first(&["USER", "USERNAME", "LOGNAME"]);
    let shell = first(&["SHELL", "COMSPEC"]);
    let tmpdir = {
        let declared = first(&["TMPDIR", "TEMP", "TMP"]);
        if declared.is_empty() {
            std::env::temp_dir().to_string_lossy().into_owned()
        } else {
            declared
        }
    };
    // The one thing asked of the kernel, and it goes through std: how many
    // threads that will actually run us. CPU counts size thread pools; they are
    // not a fact about the user.
    let cpus = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    let mut out = String::with_capacity(512);
    out.push_str("{\"platform\":");
    out.push_str(&quote(node_platform()));
    out.push_str(",\"arch\":");
    out.push_str(&quote(node_arch()));
    out.push_str(",\"type\":");
    out.push_str(&quote("XmasJS"));
    out.push_str(",\"release\":");
    out.push_str(&quote(env!("CARGO_PKG_VERSION")));
    out.push_str(",\"machine\":");
    out.push_str(&quote(node_arch()));
    out.push_str(",\"hostname\":");
    out.push_str(&quote(""));
    out.push_str(",\"homedir\":");
    out.push_str(&quote(&homedir));
    out.push_str(",\"tmpdir\":");
    out.push_str(&quote(&tmpdir));
    out.push_str(&format!(",\"cpus\":{cpus}"));
    out.push_str(",\"eol\":");
    out.push_str(&quote(if cfg!(windows) { "\r\n" } else { "\n" }));
    out.push_str(",\"uid\":-1");
    out.push_str(",\"gid\":-1");
    out.push_str(",\"user\":");
    out.push_str(&quote(&user));
    out.push_str(",\"shell\":");
    out.push_str(&quote(&shell));
    out.push('}');
    out
}
