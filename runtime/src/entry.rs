//! Serve a module as an app.
//!
//! Every worker builds its own realm, installs the modules, evaluates the entry on it and resolves
//! the fetch handler from what the entry exported. This is where "app.fetch" and "Xmas.serve" meet:
//! the entry either exports a fetch handler or calls Xmas.serve, and either way a request ends up
//! in the same place.
//!
//! Imports inside the entry are not resolved yet: the loader only knows modules that are already in
//! the table, so a bare or relative import fails with the module name in the message. See the
//! loader section of docs/server.md.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{
    ModuleImportAttributes, ModuleLoader, ModuleScope, ModuleSource, NativeErrorKind, Value,
};

use crate::driver::Driver;
use crate::host::{Host, Modules};
use crate::host::Server;
use crate::modules::server::serve;

/// The loader for a table of already-compiled modules: the engine asks by name, the table answers.
struct TableLoader {
    modules: Modules,
}

impl ModuleLoader for TableLoader {
    fn load(
        &mut self,
        scope: &mut ModuleScope<'_>,
        name: &str,
        _attributes: &ModuleImportAttributes,
    ) -> Result<ModuleSource, Value> {
        match self.modules.get(name) {
            Some(module) => Ok(ModuleSource::source(module.code.clone().into_bytes())),
            None => Err(scope
                .new_native_error(
                    NativeErrorKind::Type,
                    &format!("Cannot find module '{name}'"),
                )
                .unwrap_or(Value::Undefined)),
        }
    }
}

/// Serve the module at path on addr, with one realm per worker.
pub fn serve_app(addr: &str, workers: usize, entry: &Path) -> Result<Server, String> {
    let path = entry.to_path_buf();
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let addr = addr.to_owned();

    let listen = addr.clone();
    serve(&addr, workers, move |_index| {
        let mut driver = build(&listen)?;
        let namespace = evaluate(&mut driver, &path, &source)?;
        entry_point(&mut driver, namespace)?;
        Ok(Rc::new(RefCell::new(driver)))
    })
}

/// One realm, with the modules installed and nothing else.
fn build(addr: &str) -> Result<Driver, String> {
    let mut driver = Driver::new(Host::new()).map_err(|error| format!("build realm: {error}"))?;
    let modules = driver.host.modules.clone();
    driver
        .runtime()
        .set_module_loader(TableLoader { modules })
        .map_err(|error| format!("install loader: {error}"))?;
    crate::modules::install(&mut driver)?;
    // Where this realm is reachable, so an entry that calls Xmas.serve({ port }) can be told when
    // it asked for a different one than the listener actually has.
    let quoted = format!(
        "globalThis.__serve_addr = '{}';",
        addr.replace('\\', "\\\\").replace('\'', "\\'")
    );
    driver.eval(&quoted)?;
    Ok(driver)
}

/// Compile the entry, run it, and hand back its namespace.
///
/// execute_module's return value cannot be read for exported names, so a bootstrap module imports
/// the namespace onto the global object and the entry is evaluated as part of that import.
fn evaluate(driver: &mut Driver, path: &Path, source: &str) -> Result<Value, String> {
    let name = specifier(path)?;
    let module = driver
        .host
        .modules
        .load(&name, &name, source)
        .map_err(|error| format!("{name}: {error}"))?;
    let context = driver.context();
    context
        .compile_module_with_filename(&module.code, &name)
        .map_err(|error| format!("{name}: {error}"))?;
    let bootstrap = format!("import * as ns from '{name}'; globalThis.__xmas_entry = ns;");
    let bytecode = context
        .compile_module_with_filename(&bootstrap, "xmas:entry")
        .map_err(|error| format!("xmas:entry: {error}"))?;
    context
        .execute_module(&bytecode)
        .map_err(|error| format!("{name}: {error}"))?;

    // Module evaluation is promise-driven, so a throw at the top level of the entry arrives as a
    // rejected promise rather than an error above. Drain the jobs it queued and report it here:
    // otherwise the entry merely looks handler-less and the reason it failed is lost.
    driver.slice()?;
    let mut rejections = driver.take_rejections();
    if !rejections.is_empty() {
        let reason = rejections.remove(0);
        return Err(format!(
            "{name}: the entry threw while it was evaluated: {reason}"
        ));
    }

    driver.eval("globalThis.__xmas_entry")
}

/// Hand the namespace to the server prelude, which resolves the handler out of it.
fn entry_point(driver: &mut Driver, namespace: Value) -> Result<(), String> {
    let resolve = driver
        .context()
        .eval("__serve_entry")
        .map_err(|error| format!("__serve_entry: {error}"))?;
    let Value::Object(object) = &resolve else {
        return Err("__serve_entry is not a function".into());
    };
    let callable = driver
        .context()
        .as_callable(object)
        .map_err(|error| format!("__serve_entry: {error}"))?
        .ok_or("__serve_entry is not a function")?;
    let called = driver.context().call(&callable, Value::Undefined, &[namespace]);
    match called {
        Ok(_) => Ok(()),
        // What a handler-less entry says is the useful part, so keep the engine's rendering of it.
        Err(error) => Err(driver.render(error)),
    }
}

/// The entry's own path, as the module specifier it is registered under. Forward slashes only: a
/// specifier is a string in JS, and a backslash there is an escape.
fn specifier(path: &Path) -> Result<String, String> {
    let absolute = std::fs::canonicalize(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let text = absolute.to_string_lossy().replace('\\', "/");
    if text.contains('\'') {
        return Err(format!("the entry path must not contain a quote: {text}"));
    }
    Ok(text)
}
