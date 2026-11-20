//! events: EventEmitter, Event, CustomEvent, EventTarget.
//!
//! The old modules/src/event/ is rquickjs's Class plus Arc<RwLock<Vec<..>>> (748 lines).
//! None of it needs a capability from the host, it is all JS semantics, so the whole block
//! is JS and the Rust side only injects the prelude and registers the result as a module.
//! EventEmitter is what stream builds on.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/events.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("events prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__events_module")
        .map_err(|error| format!("events module: {error}"))?;
    let names = [
        "default",
        "EventEmitter",
        "Event",
        "CustomEvent",
        "EventTarget",
    ];
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "events", exports)
}
