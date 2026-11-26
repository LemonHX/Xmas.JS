//! diagnostics_channel: named channels with subscribers. Pure JS; libraries import it for metrics.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/diagnostics_channel.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("diagnostics_channel prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__diagnostics_channel_module")
        .map_err(|error| format!("diagnostics_channel exports: {error}"))?;
    let names = [
        "channel", "hasSubscribers", "subscribe", "unsubscribe", "tracingChannel", "Channel",
        "TracingChannel",
    ];
    let mut exports = super::collect(driver, &module, &names)?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "diagnostics_channel", exports)
}
