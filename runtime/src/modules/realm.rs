//! xmas:realm: a source string as a module, evaluated on demand.
//!
//! The engine compiles a module from a name and a source, so running a string is that plus a way
//! to ask for it by name again. `__realm_load` is the compile-and-register half; the half that
//! runs it is a dynamic `import` of the name it answers (js/realm.js). That import is the engine's
//! own evaluation, which is why top-level `await` needs nothing from this layer: the promise the
//! import answers with settles when the module has finished, however long that takes.
//!
//! A run number goes into the name. Both this table and the engine's own record of a module key on
//! the name, and a record that already exists is handed back as it is -- so a second evaluation
//! under the same name would answer with the first run rather than running again.
//!
//! The run number sits before the extension (`esm.js` becomes `esm#1.js`), so the name still says
//! which language to compile: the compiler picks TypeScript or JavaScript by the last dot. A
//! relative import inside the source resolves against the name's own directory, which the run
//! number does not disturb.

use std::cell::Cell;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/realm.js");

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body =
        JsString::try_from_utf8(body).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

/// `esm.js` and 3 become `esm#3.js`: one run out of many, still named as what it is.
fn counted(name: &str, run: u64) -> String {
    match name.rfind('.') {
        // A dot at 0 is a dotfile, not an extension: `.env` must not become `#1.env`.
        Some(dot) if dot > 0 => format!("{}#{run}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}#{run}"),
    }
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let counter = Rc::new(Cell::new(0u64));

    {
        let modules = driver.host.modules.clone();
        let counter = counter.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_load",
                2,
                move |scope: &mut Scope<'_>, name: String, source: String| {
                    if name.is_empty() {
                        return Err(super::throw(scope, "evaluate: a module needs a name"));
                    }
                    counter.set(counter.get() + 1);
                    let specifier = counted(&name, counter.get());
                    // The loader answers a name already in the table before it tries resolution
                    // against the disk, so what is registered here is imported as this name and
                    // is never looked for as a file.
                    match modules.load(&specifier, &specifier, &source) {
                        Ok(_) => text(&specifier),
                        Err(reason) => Err(super::throw(scope, &reason)),
                    }
                },
            )
            .map_err(|error| format!("__realm_load: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("realm prelude: {error}"))?;
    let module = driver
        .context()
        .eval("globalThis.__realm_module")
        .map_err(|error| format!("__realm_module: {error}"))?;
    let names = ["createRealm", "Realm", "default"];
    let exports = super::collect(driver, &module, &names)?;
    super::define_name(driver, "xmas:realm", exports)
}
