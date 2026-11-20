//! console. Formatting is done in JS; Rust only receives one line of final text.
//!
//! The old implementation (modules/src/console.rs, 286 lines) wrapped a Console object in an rquickjs Class,
//! and assembled %s / %d formatting by hand in utils/console.rs. Here it is reversed: formatting is written in JS,
//! readable at a glance, and the Rust side has only one function, __console_write.

use quickjs_oxide::engine::api::core::{Js, RuntimeError, Value};

use crate::driver::Driver;
use crate::host::Level;

const PRELUDE: &str = include_str!("../../js/console.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    let console = driver.host.console.clone();
    driver
        .context()
        .define_function(
            &global,
            "__console_write",
            2,
            move |_scope: &mut quickjs_oxide::engine::api::core::Scope<'_>,
                  level: String,
                  line: String|
                  -> Result<Value, RuntimeError> {
                let level = match level.as_str() {
                    "warn" => Level::Warn,
                    "error" => Level::Error,
                    "info" => Level::Info,
                    _ => Level::Log,
                };
                console.line(level, &line);
                Ok(Value::Undefined)
            },
        )
        .map_err(|error| format!("__console_write: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("console prelude: {error}"))?;

    let console = driver
        .eval("globalThis.__console")
        .map_err(|error| format!("console object: {error}"))?;
    let Value::Object(object) = &console else {
        return Err("console is not an object".into());
    };
    let mut exports: Vec<(String, Value)> = vec![("default".to_string(), console.clone())];
    for field in [
        "log", "info", "debug", "warn", "error", "trace", "dir", "assert", "clear", "time",
        "timeEnd",
    ] {
        let key = quickjs_oxide::engine::api::core::PropertyKey::from_js_string(
            quickjs_oxide::engine::api::core::JsString::try_from_utf8(field).expect("field name"),
        );
        let value = driver
            .context()
            .get_property(object, &key)
            .map_err(|error| format!("console.{field}: {error}"))?;
        exports.push((field.to_string(), value));
    }
    define_with_alias(driver, "console", exports)
}

use super::define_with_alias;
