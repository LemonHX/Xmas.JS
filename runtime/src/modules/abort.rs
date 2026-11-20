//! abort: AbortController and AbortSignal, built on the EventTarget from events.
//!
//! The old modules/src/abort/ is a 341-line rquickjs Class, and it also installed the
//! prototype of EventTarget. There are no class bindings here, so AbortSignal inherits
//! EventTarget directly in JS.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/abort.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("abort prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__abort_module")
        .map_err(|error| format!("abort module: {error}"))?;
    let exports = super::collect(driver, &module, &["AbortController", "AbortSignal"])?;
    super::define_with_alias(driver, "abort", exports)
}
