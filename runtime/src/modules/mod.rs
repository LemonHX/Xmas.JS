//! Modules carried over from the old runtime, one at a time, in dependency order.
//!
//! Every module installs itself: it puts its API on the global object and also
//! registers a synthetic module under `name` and `node:name`. Host capabilities
//! come from `crate::host` and nothing here reaches for tokio.

pub mod abort;
pub mod console;
pub mod event;
pub mod timers;

use crate::driver::Driver;
use quickjs_oxide::engine::api::core::Value;

/// Install everything, in dependency order.
pub fn install(driver: &mut Driver) -> Result<(), String> {
    console::install(driver)?;
    timers::install(driver)?;
    event::install(driver)?;
    abort::install(driver)?;
    Ok(())
}

/// Build a TypeError to throw at JS. Host functions report bad user input this
/// way: `Invariant` means the embedder broke its own contract, not the script.
pub(crate) fn throw(
    scope: &mut quickjs_oxide::engine::api::core::Scope<'_>,
    message: &str,
) -> quickjs_oxide::engine::api::core::RuntimeError {
    use quickjs_oxide::engine::api::core::{NativeErrorKind, RuntimeError};
    match scope.new_native_error(NativeErrorKind::Type, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

/// Read a list of exports off an object, skipping the ones that are undefined.
fn collect(
    driver: &mut Driver,
    object: &Value,
    names: &[&str],
) -> Result<Vec<(String, Value)>, String> {
    use quickjs_oxide::engine::api::core::{Js, JsString, PropertyKey};
    let Value::Object(source) = object else {
        return Err("expected an object of exports".into());
    };
    let mut exports = Vec::new();
    for name in names {
        let key = PropertyKey::from_js_string(JsString::try_from_utf8(name).expect("export name"));
        let value = driver
            .context()
            .get_property(source, &key)
            .map_err(|error| format!("{name}: {error}"))?;
        if matches!(value, Value::Undefined) {
            continue;
        }
        exports.push(((*name).to_string(), value));
    }
    Ok(exports)
}

/// Register a set of exports under both `name` and the `node:` prefixed alias.
pub(crate) fn define_with_alias(
    driver: &mut Driver,
    name: &str,
    exports: Vec<(String, Value)>,
) -> Result<(), String> {
    let context = driver.context();
    context
        .define_module(name, exports.clone())
        .map_err(|error| format!("define_module {name}: {error}"))?;
    let alias = format!("node:{name}");
    context
        .define_module(&alias, exports)
        .map_err(|error| format!("define_module {alias}: {error}"))?;
    Ok(())
}
