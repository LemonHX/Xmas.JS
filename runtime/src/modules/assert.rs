//! assert: a JS module with no host functions at all.
//!
//! LLRT's assert (llrt_assert/src/lib.rs) implements only assert.ok, so there
//! was nothing to copy beyond its falsy rules, which the JS layer keeps. The
//! rest of Node's surface is written there because every check it makes reads
//! JS values and prototypes, which is the side of the boundary that can do it
//! without a call per property.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/assert.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("assert prelude: {error}"))?;

    let assert = driver
        .eval("globalThis.__assert_module")
        .map_err(|error| format!("assert object: {error}"))?;
    let names = [
        "ok",
        "fail",
        "equal",
        "notEqual",
        "strictEqual",
        "notStrictEqual",
        "deepEqual",
        "notDeepEqual",
        "deepStrictEqual",
        "notDeepStrictEqual",
        "throws",
        "doesNotThrow",
        "rejects",
        "doesNotReject",
        "match",
        "doesNotMatch",
        "ifError",
        "AssertionError",
        "strict",
    ];
    let mut exports = vec![("default".to_string(), assert.clone())];
    exports.extend(super::collect(driver, &assert, &names)?);
    super::define_with_alias(driver, "assert", exports.clone())?;
    // assert/strict is the same object in Node; the strict checks are the ones
    // every function here already makes.
    super::define_name(driver, "assert/strict", exports.clone())?;
    super::define_name(driver, "node:assert/strict", exports)?;
    Ok(())
}
