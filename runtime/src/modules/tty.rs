//! tty: whether a descriptor is a terminal, and thin streams over one.
//!
//! LLRT's llrt_tty is 80 lines: isatty over libc, exported as a module. Node
//! also has ReadStream and WriteStream, which Node itself uses for
//! process.stdout; here they are wrappers over the stdio that process.js
//! already holds, because there is only ever one console per realm.
//!
//! The unsafe part lives in host/sysinfo.rs with the other libc calls.

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::sysinfo;

const PRELUDE: &str = include_str!("../../js/tty.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(&global, "__tty_isatty", 1, move |_: &mut Scope<'_>, fd: f64| {
            Ok::<Value, RuntimeError>(Value::Bool(sysinfo::isatty(fd as i32)))
        })
        .map_err(|error| format!("__tty_isatty: {error}"))?;

    driver
        .context()
        .define_function(&global, "__tty_size", 1, move |_: &mut Scope<'_>, fd: f64| {
            let body = match sysinfo::window_size(fd as i32) {
                Some((columns, rows)) => format!("[{columns},{rows}]"),
                None => "null".to_string(),
            };
            let body = JsString::try_from_utf8(&body)
                .map_err(|error| RuntimeError::Internal(error.to_string()))?;
            Ok::<Value, RuntimeError>(Value::String(body))
        })
        .map_err(|error| format!("__tty_size: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("tty prelude: {error}"))?;

    let tty = driver
        .eval("globalThis.__tty_module")
        .map_err(|error| format!("tty object: {error}"))?;
    let names = ["isatty", "ReadStream", "WriteStream"];
    let mut exports = super::collect(driver, &tty, &names)?;
    exports.push(("default".to_string(), tty));
    super::define_with_alias(driver, "tty", exports)?;
    Ok(())
}
