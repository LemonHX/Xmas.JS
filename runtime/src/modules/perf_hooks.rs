//! perf_hooks: the performance clock, marks and measures.
//!
//! LLRT's llrt_perf_hooks is thin as well: a Performance class with now(),
//! timeOrigin and toJSON, exported as a module and installed as a global. Node
//! also has marks, measures and PerformanceObserver, and those are what tooling
//! actually calls, so they are in the prelude here.
//!
//! Only the clock crosses the boundary. now() is one host call returning an
//! f64: about 150 ns, against roughly 30 ns for Node's v8 clock. Everything
//! else (entry lists, lookups, observers) is a plain JS array.

use quickjs_oxide::engine::api::core::{RuntimeError, Scope, Value};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/perf_hooks.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let clock = driver.host.clock.clone();
        driver
            .context()
            .define_function(&global, "__perf_now_ns", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(clock.elapsed_ns()))
            })
            .map_err(|error| format!("__perf_now_ns: {error}"))?;
    }

    {
        // timeOrigin is the wall clock at the moment the realm's clock started,
        // which is what Node reports for the process.
        let origin = unix_millis() - driver.host.clock.elapsed_us() / 1000.0;
        driver
            .context()
            .define_function(&global, "__perf_origin", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(origin))
            })
            .map_err(|error| format!("__perf_origin: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("perf_hooks prelude: {error}"))?;

    let performance = driver
        .eval("globalThis.performance")
        .map_err(|error| format!("performance object: {error}"))?;
    let module = driver
        .eval("globalThis.__perf_hooks_module")
        .map_err(|error| format!("perf_hooks exports: {error}"))?;
    let names = [
        "Performance",
        "PerformanceEntry",
        "PerformanceMark",
        "PerformanceMeasure",
        "PerformanceObserver",
        "constants",
    ];
    let mut exports = vec![("performance".to_string(), performance.clone())];
    exports.extend(super::collect(driver, &module, &names)?);
    exports.push(("default".to_string(), performance));
    super::define_with_alias(driver, "perf_hooks", exports)?;
    Ok(())
}

fn unix_millis() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}
