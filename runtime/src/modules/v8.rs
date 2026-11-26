//! v8: serialize/deserialize and heap statistics stubs. Pure JS.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/v8.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver.eval(PRELUDE).map_err(|error| format!("v8 prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__v8_module")
        .map_err(|error| format!("v8 exports: {error}"))?;
    let names = ["serialize", "deserialize", "getHeapStatistics", "setFlagsFromString", "cachedDataVersionTag"];
    let mut exports = super::collect(driver, &module, &names)?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "v8", exports)
}
