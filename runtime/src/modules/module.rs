//! module: builtinModules, isBuiltin and a createRequire that explains itself. Pure JS.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/module.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver.eval(PRELUDE).map_err(|error| format!("module prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__module_module")
        .map_err(|error| format!("module exports: {error}"))?;
    let mut exports = super::collect(driver, &module, &["builtinModules", "isBuiltin", "createRequire"])?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "module", exports)
}
