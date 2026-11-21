//! Serve a module as an app.
//!
//! Every worker builds its own realm, installs the modules, evaluates the entry on it and resolves
//! the fetch handler from what the entry exported. This is where "app.fetch" and "Xmas.serve" meet:
//! the entry either exports a fetch handler or calls Xmas.serve, and either way a request ends up
//! in the same place.
//!
//! Imports inside the entry and inside everything it reaches are resolved against the file that
//! wrote them (crate::resolve) and loaded from disk once per realm. See docs/modules.md.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::Value;

use crate::driver::Driver;
use crate::host::{Host, Loader};
use crate::host::Server;
use crate::modules::server::serve;

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
        .set_module_loader(Loader::new(modules))
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
    // A failed import linking is thrown, and a thrown value carries its own message; the engine's own
    // Display for one is only "JavaScript exception", which says nothing about which import failed.
    let executed = context.execute_module(&bytecode);
    if let Err(error) = executed {
        return Err(format!("{name}: {}", driver.render(error)));
    }

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
