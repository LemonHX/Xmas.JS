//! async_hooks: a placeholder with the shape Node's API has, plus AsyncLocalStorage.
//!
//! LLRT's llrt_async_hooks rides on rquickjs promise hooks (C QuickJS
//! JS_SetPromiseHook) to see every promise being created and resolved. This
//! engine has no promise hook, so createHook exists and its hooks never fire.
//! That is the honest limit, and it is why the ids below are all zero: LLRT
//! returns zero from currentId too, for the same reason.
//!
//! AsyncLocalStorage is implemented for what can be implemented without
//! promise hooks: run, enterWith, exit, getStore, disable and bind. A store
//! survives synchronous code and anything bind() wrapped; it does not follow an
//! await, because there is no way to know which store a resumed continuation
//! belongs to.

const PRELUDE: &str = include_str!("../../js/async_hooks.js");

pub fn install(driver: &mut crate::driver::Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("async_hooks prelude: {error}"))?;

    let module = driver
        .eval("globalThis.__async_hooks_module")
        .map_err(|error| format!("async_hooks object: {error}"))?;
    let names = [
        "createHook",
        "executionAsyncId",
        "triggerAsyncId",
        "currentId",
        "AsyncLocalStorage",
        "AsyncResource",
    ];
    let mut exports = super::collect(driver, &module, &names)?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "async_hooks", exports)?;
    Ok(())
}
