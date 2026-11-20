//! os: the machine this realm runs on, shaped after Node's os module.
//!
//! The values that never change during a run are read once at install time and
//! frozen into JS. totalmem, freemem, uptime and loadavg change, so those stay
//! host calls; they return numbers, not JSON, so a call allocates nothing.

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::sysinfo;
use crate::modules::{node_arch, node_platform, quote};

const PRELUDE: &str = include_str!("../../js/os.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let snapshot = snapshot();
        driver
            .context()
            .define_function(&global, "__os_static", 0, move |_: &mut Scope<'_>| {
                text(&snapshot)
            })
            .map_err(|error| format!("__os_static: {error}"))?;
    }

    driver
        .context()
        .define_function(&global, "__os_totalmem", 0, move |_: &mut Scope<'_>| {
            Ok::<Value, RuntimeError>(Value::Float(sysinfo::total_memory() as f64))
        })
        .map_err(|error| format!("__os_totalmem: {error}"))?;

    driver
        .context()
        .define_function(&global, "__os_freemem", 0, move |_: &mut Scope<'_>| {
            Ok::<Value, RuntimeError>(Value::Float(sysinfo::free_memory() as f64))
        })
        .map_err(|error| format!("__os_freemem: {error}"))?;

    driver
        .context()
        .define_function(&global, "__os_uptime", 0, move |_: &mut Scope<'_>| {
            Ok::<Value, RuntimeError>(Value::Float(sysinfo::uptime()))
        })
        .map_err(|error| format!("__os_uptime: {error}"))?;

    driver
        .context()
        .define_function(&global, "__os_loadavg", 0, move |_: &mut Scope<'_>| {
            let [one, five, fifteen] = sysinfo::loadavg();
            text(&format!("[{one},{five},{fifteen}]"))
        })
        .map_err(|error| format!("__os_loadavg: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("os prelude: {error}"))?;

    let os = driver
        .eval("globalThis.__os")
        .map_err(|error| format!("os object: {error}"))?;
    let names = [
        "platform", "arch", "type", "release", "machine", "hostname", "homedir", "tmpdir",
        "endianness", "cpus", "totalmem", "freemem", "uptime", "loadavg", "userInfo", "EOL",
        "constants",
    ];
    let mut exports = vec![("default".to_string(), os.clone())];
    exports.extend(super::collect(driver, &os, &names)?);
    super::define_with_alias(driver, "os", exports)?;
    Ok(())
}

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body = JsString::try_from_utf8(body)
        .map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

/// The part of the answer that cannot change while the process runs.
fn snapshot() -> String {
    let homedir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default();
    let shell = std::env::var("SHELL").unwrap_or_default();
    let mut out = String::with_capacity(512);
    out.push_str("{\"platform\":");
    out.push_str(&quote(node_platform()));
    out.push_str(",\"arch\":");
    out.push_str(&quote(node_arch()));
    out.push_str(",\"type\":");
    out.push_str(&quote(&sysinfo::system_name()));
    out.push_str(",\"release\":");
    out.push_str(&quote(&sysinfo::release()));
    out.push_str(",\"machine\":");
    out.push_str(&quote(&sysinfo::machine()));
    out.push_str(",\"hostname\":");
    out.push_str(&quote(&sysinfo::hostname()));
    out.push_str(",\"homedir\":");
    out.push_str(&quote(&homedir));
    out.push_str(",\"tmpdir\":");
    out.push_str(&quote(&std::env::temp_dir().to_string_lossy()));
    out.push_str(&format!(",\"cpus\":{}", sysinfo::cpu_count()));
    out.push_str(",\"eol\":");
    out.push_str(&quote(if cfg!(windows) { "\r\n" } else { "\n" }));
    out.push_str(&format!(",\"uid\":{}", sysinfo::uid()));
    out.push_str(&format!(",\"gid\":{}", sysinfo::gid()));
    out.push_str(",\"user\":");
    out.push_str(&quote(&user));
    out.push_str(",\"shell\":");
    out.push_str(&quote(&shell));
    out.push('}');
    out
}
