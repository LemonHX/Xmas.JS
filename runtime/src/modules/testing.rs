//! xmas:test. The framework is js/test.js; this installs it as a module.
//!
//! It is not part of `modules::install`: only a realm that runs tests (see crate::testrun) or a
//! test of this module pays for it, and an app being served never sees it.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/test.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("test prelude: {error}"))?;
    let module = driver
        .context()
        .eval("globalThis.__xmas_test_module")
        .map_err(|error| format!("__xmas_test_module: {error}"))?;
    let names = [
        "describe", "test", "it", "expect", "beforeAll", "beforeEach", "afterAll", "afterEach", "mock",
        "spyOn", "jest", "vi", "setDefaultTimeout",
    ];
    let exports = super::collect(driver, &module, &names)?;
    super::define_name(driver, "xmas:test", exports)
}
