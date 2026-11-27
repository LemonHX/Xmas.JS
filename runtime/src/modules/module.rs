//! module: builtinModules, isBuiltin and a createRequire that explains itself. Pure JS.

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/module.js");

/// The resolver behind `createRequire(..).resolve`, and behind `require` for a file.
///
/// The compiler turns `import.meta.resolve` into a call because the specifier is known only when
/// the program runs. A `require` built by `createRequire` is the same situation, so it asks here:
/// the answer is the path of the file, which is what `require.resolve` returns.
fn install_require_resolve(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    driver
        .context()
        .define_function(
            &global,
            "__xmas_require_resolve",
            2,
            |_scope: &mut quickjs_oxide::engine::api::core::Scope<'_>,
             referrer: String,
             specifier: String|
             -> Result<String, quickjs_oxide::engine::api::core::RuntimeError> {
                let referrer = match referrer.strip_prefix("file://") {
                    Some(path) if !crate::resolve::is_remote(&referrer) => path.to_owned(),
                    _ => referrer,
                };
                crate::resolve::resolve_require(&referrer, &specifier).map_err(|reason| {
                    quickjs_oxide::engine::api::core::RuntimeError::Internal(format!(
                        "Cannot resolve '{specifier}' from {referrer}: {reason}"
                    ))
                })
            },
        )
        .map_err(|error| format!("install require.resolve: {error}"))?;
    Ok(())
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    install_require_resolve(driver)?;
    driver.eval(PRELUDE).map_err(|error| format!("module prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__module_module")
        .map_err(|error| format!("module exports: {error}"))?;
    let mut exports = super::collect(driver, &module, &["builtinModules", "isBuiltin", "createRequire"])?;
    exports.push(("default".to_string(), module));
    super::define_with_alias(driver, "module", exports)
}
