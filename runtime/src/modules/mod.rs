//! Modules carried over from the old runtime, one at a time, in dependency order.
//!
//! Every module installs itself: it puts its API on the global object and also
//! registers a synthetic module under `name` and `node:name`. Host capabilities
//! come from `crate::host` and nothing here reaches for tokio.

pub mod abort;
pub mod assert;
pub mod async_hooks;
pub mod buffer;
pub mod bytes;
pub mod console;
pub mod crypto;
pub mod event;
pub mod exception;
pub mod fs;
pub mod os;
pub mod path;
pub mod perf_hooks;
pub mod process;
pub mod stream;
pub mod stream_web;
pub mod string_decoder;
pub mod timers;
pub mod tty;
pub mod url;
pub mod util;
pub mod zlib;

use crate::driver::Driver;
use quickjs_oxide::engine::api::core::Value;

pub(crate) use buffer::buffer_hex;

/// Install everything, in dependency order.
pub fn install(driver: &mut Driver) -> Result<(), String> {
    exception::install(driver)?;
    assert::install(driver)?;
    console::install(driver)?;
    timers::install(driver)?;
    event::install(driver)?;
    perf_hooks::install(driver)?;
    async_hooks::install(driver)?;
    abort::install(driver)?;
    url::install(driver)?;
    buffer::install(driver)?;
    path::install(driver)?;
    process::install(driver)?;
    tty::install(driver)?;
    util::install(driver)?;
    os::install(driver)?;
    fs::install(driver)?;
    crypto::install(driver)?;
    string_decoder::install(driver)?;
    zlib::install(driver)?;
    stream::install(driver)?;
    stream_web::install(driver)?;
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

/// Register a set of exports under one module name.
pub(crate) fn define_name(
    driver: &mut Driver,
    name: &str,
    exports: Vec<(String, Value)>,
) -> Result<(), String> {
    driver
        .context()
        .define_module(name, exports)
        .map_err(|error| format!("define_module {name}: {error}"))
}

/// Register a set of exports under both `name` and the `node:` prefixed alias.
pub(crate) fn define_with_alias(
    driver: &mut Driver,
    name: &str,
    exports: Vec<(String, Value)>,
) -> Result<(), String> {
    define_name(driver, name, exports.clone())?;
    define_name(driver, &format!("node:{name}"), exports)
}

/// A string as a JSON string literal. Small and local on purpose: it is used to
/// hand install-time data to JS once, not to serialize anything at run time.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// process.platform / os.platform() spelling: Node says darwin, not macos.
pub(crate) fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// process.arch / os.arch() spelling: Node says arm64 and x64.
pub(crate) fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}
