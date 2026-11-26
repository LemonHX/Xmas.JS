//! worker_threads: the main thread only; Worker refuses to start. Pure JS.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/worker_threads.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver.eval(PRELUDE).map_err(|error| format!("worker_threads prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__worker_threads_module")
        .map_err(|error| format!("worker_threads exports: {error}"))?;
    let names = ["isMainThread", "parentPort", "workerData", "threadId", "Worker", "MessageChannel", "MessagePort", "receiveMessageOnPort"];
    let mut exports = super::collect(driver, &module, &names)?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "worker_threads", exports)
}
