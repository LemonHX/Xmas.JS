//! tty: whether a descriptor is a terminal, and thin streams over one.
//!
//! LLRT's llrt_tty is 80 lines of isatty over libc. std has IsTerminal, which
//! answers the same question for the three descriptors a realm actually has,
//! without any FFI of ours.
//!
//! Node's columns and rows come from TIOCGWINSZ, which std does not expose. The
//! environment is the only size available without an ioctl, so COLUMNS and LINES
//! are what get reported; with neither set the size stays Node's "unknown"
//! answer, [0, 0].
//!
//! ReadStream and WriteStream are wrappers over the stdio that process.js
//! already holds, because there is only ever one console per realm.

use std::io::IsTerminal;

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/tty.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(&global, "__tty_isatty", 1, move |_: &mut Scope<'_>, fd: f64| {
            Ok::<Value, RuntimeError>(Value::Bool(is_terminal(fd as i32)))
        })
        .map_err(|error| format!("__tty_isatty: {error}"))?;

    {
        let columns = number_in_env(driver, "COLUMNS").unwrap_or(0);
        let rows = number_in_env(driver, "LINES").unwrap_or(0);
        let body = format!("[{columns},{rows}]");
        driver
            .context()
            .define_function(&global, "__tty_size", 1, move |_: &mut Scope<'_>, _fd: f64| {
                let body = JsString::try_from_utf8(&body)
                    .map_err(|error| RuntimeError::Internal(error.to_string()))?;
                Ok::<Value, RuntimeError>(Value::String(body))
            })
            .map_err(|error| format!("__tty_size: {error}"))?;
    }

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

/// Only the descriptors this process owns: 0, 1 and 2. Anything else is not a
/// terminal as far as a realm can tell, because nothing here opens one.
fn is_terminal(fd: i32) -> bool {
    match fd {
        0 => std::io::stdin().is_terminal(),
        1 => std::io::stdout().is_terminal(),
        2 => std::io::stderr().is_terminal(),
        _ => false,
    }
}

fn number_in_env(driver: &Driver, key: &str) -> Option<u16> {
    driver.host.env.get(key)?.trim().parse().ok()
}
