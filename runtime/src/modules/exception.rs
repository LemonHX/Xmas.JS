//! DOMException, carried over from LLRT (modules/llrt_exceptions).
//!
//! LLRT built it as a Rust class because rquickjs has class macros. There is
//! nothing native in it: the WebIDL name table, the legacy codes and the
//! QuotaExceededError subclass are all data, so the whole thing is the prelude.
//!
//! Not a module: DOMException is a global in Node too.

const PRELUDE: &str = include_str!("../../js/exception.js");

pub fn install(driver: &mut crate::driver::Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("exception prelude: {error}"))?;
    Ok(())
}
