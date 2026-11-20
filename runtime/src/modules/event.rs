//! events：EventEmitter、Event、CustomEvent、EventTarget。
//!
//! The old modules/src/event/ is rquickjs's Class + Arc<RwLock<Vec<..>>>（748 lines）。
//! This whole thing has no host-side capabilities at all，it is all JS semantics，so the whole block is written in JS，and the Rust side is only responsible for
//! inject the prelude、register the result as a module。EventEmitter is for the subsequent stream。

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/events.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("events prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__events_module")
        .map_err(|error| format!("events module: {error}"))?;
    let names = ["default", "EventEmitter", "Event", "CustomEvent", "EventTarget"];
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "events", exports)
}
