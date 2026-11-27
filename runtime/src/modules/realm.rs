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
//!
//! Killing has to stop code that is already running -- a `while` loop the model wrote -- and JS
//! gets no turn to do anything while that loop runs. So the flag lives here, in Rust, and the
//! interrupt the engine checks reads it: `kill()` calls `__realm_kill`, which sets the flag, the
//! loop unwinds with an ordinary throw on its next statement, and the driver is usable again
//! because the interrupt was only ever `some killed realm is running` rather than a latch.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// The realms of one driver, and which of them is running right now.
///
/// The engine's interrupt asks one question -- should this run stop -- and this is the answer: the
/// realm that entered is killed. A run that is not from a realm (a plain `eval`) is never
/// interrupted, so a killed realm cannot poison the rest of the driver.
#[derive(Clone, Default)]
pub struct Realms {
    /// One switch per realm. `Arc<AtomicBool>` rather than a plain flag because the one caller a
    /// JS-level `kill()` cannot serve is the interesting one: something outside this thread
    /// stopping a run that is already spinning.
    killed: Rc<RefCell<Vec<Arc<AtomicBool>>>>,
    /// The realm whose source the engine is currently evaluating, if any.
    running: Rc<Cell<Option<usize>>>,
    /// Modules `provide` handed over while the engine was entered.
    ///
    /// Defining one is `Context::define_module`, which needs the runtime free, and a host function
    /// runs inside it. So a provide is held here and defined by the driver between jobs. Defining is
    /// what makes a replacement work: it also drops the name from the map of already-evaluated
    /// modules, so an import after a second provide answers with the second one.
    staged: Rc<RefCell<Vec<(String, Vec<(String, Value)>)>>>,
}

/// A realm's kill switch, in a form another thread can hold.
///
/// This is how an embedder stops an agent: it keeps the handle, and calls `kill` from whatever
/// thread the stop comes in on. It works while the realm's code is running, which a JS call to
/// `realm.kill()` cannot, because JS does not get a turn then.
#[derive(Clone)]
pub struct KillSwitch(Arc<AtomicBool>);

impl KillSwitch {
    pub fn kill(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn killed(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Realms {
    fn alive(&self, id: usize) -> bool {
        self.killed
            .borrow()
            .get(id)
            .is_none_or(|switch| !switch.load(Ordering::Relaxed))
    }

    fn kill(&self, id: usize) {
        if let Some(switch) = self.killed.borrow().get(id) {
            switch.store(true, Ordering::Relaxed);
        }
    }

    fn add(&self) -> usize {
        let mut killed = self.killed.borrow_mut();
        killed.push(Arc::new(AtomicBool::new(false)));
        killed.len() - 1
    }

    /// The switch for realm `id`, for an embedder that has to kill it from another thread.
    pub fn switch(&self, id: usize) -> Option<KillSwitch> {
        self.killed.borrow().get(id).map(|s| KillSwitch(Arc::clone(s)))
    }

    /// Whether the evaluation in progress belongs to a realm that was killed under it.
    pub fn interrupted(&self) -> bool {
        match self.running.get() {
            Some(id) => !self.alive(id),
            None => false,
        }
    }

    /// What the interrupt is looking at, for a test that has to see why it did not fire.
    pub fn watching(&self) -> Option<usize> {
        self.running.get()
    }

    /// The realm whose source is being evaluated from now on.
    ///
    /// Set before the `import` that starts it, because the interrupt has to answer during that
    /// import and JS gets no turn then.
    fn begin(&self, id: usize) {
        self.running.set(Some(id));
    }

    /// Holds a module for `provide` until the engine is free to define it.
    fn stage(&self, name: String, exports: Vec<(String, Value)>) {
        self.staged.borrow_mut().push((name, exports));
    }

    /// The stage, emptied.
    fn take_staged(&self) -> Vec<(String, Vec<(String, Value)>)> {
        std::mem::take(&mut *self.staged.borrow_mut())
    }

    /// Nothing is being evaluated any more. Called when the evaluation settles, and by `abandon`
    /// when it was stopped instead: the interrupt has to stop answering once the run is over, or
    /// the driver would refuse every loop after it.
    pub fn end(&self) {
        self.running.set(None);
    }
}

/// An object's own string keys and their values, as a module definition takes them.
fn entries(scope: &mut Scope<'_>, object: &Value) -> Result<Vec<(String, Value)>, String> {
    let Value::Object(object) = object else {
        return Err("provide: exports must be an object".into());
    };
    let keys = scope
        .own_property_keys(object)
        .map_err(|error| format!("provide: {error}"))?;
    let mut exports = Vec::with_capacity(keys.len());
    for key in keys {
        // A symbol cannot name an export; a module's names are strings.
        let Some(name) = key.to_js_string() else {
            continue;
        };
        let name = name.to_string();
        let value = scope
            .get_property(object, &key)
            .map_err(|error| format!("provide: {name}: {error}"))?;
        exports.push((name, value));
    }
    Ok(exports)
}

/// Rejects whatever was waiting on an evaluation the interrupt stopped.
///
/// The module's evaluation was thrown out of, so no `import` answer is coming and whoever awaited
/// `evaluate` would wait forever. Returns whether anything was waiting; nothing is when the
/// interrupt came from a deadline or a driver-level cancel rather than a realm kill.
pub fn abandon(driver: &mut Driver, reason: &str) -> bool {
    let call = format!(
        "globalThis.__realm_module.abandon(new Error({}))",
        super::quote(reason)
    );
    matches!(driver.context().eval(&call), Ok(Value::Bool(true)))
}

/// Defines the modules `provide` held while the engine was entered.
///
/// The driver calls this between jobs, which is the one place the engine is free. `define_module`
/// rather than a loader-level table, because defining also empties the name from the map of
/// evaluated modules: a second provide for a name has to be seen by an import that comes after it,
/// and only this gets that.
pub fn apply_staged(driver: &mut Driver) -> Result<(), String> {
    let staged = driver.host.realms.take_staged();
    for (name, exports) in staged {
        driver
            .context()
            .define_module(&name, exports)
            .map_err(|error| format!("provide {name}: {error}"))?;
    }
    Ok(())
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let realms = Realms::default();
    let counter = Rc::new(Cell::new(0u64));

    {
        let realms = realms.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_add",
                0,
                move |_: &mut Scope<'_>| Ok::<Value, RuntimeError>(Value::Float(realms.add() as f64)),
            )
            .map_err(|error| format!("__realm_add: {error}"))?;
    }

    {
        let realms = realms.clone();
        let modules = driver.host.modules.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_provide",
                2,
                move |scope: &mut Scope<'_>, name: String, exports: Value| {
                    if name.is_empty() {
                        return Err(super::throw(scope, "provide: a module needs a name"));
                    }
                    match entries(scope, &exports) {
                        Ok(list) => {
                            // Both: the table answers an entry load in this turn, and defining
                            // what is staged makes the name visible to an import that came before.
                            modules.provide(&name, list.clone());
                            realms.stage(name, list);
                            Ok(Value::Undefined)
                        }
                        Err(reason) => Err(super::throw(scope, &reason)),
                    }
                },
            )
            .map_err(|error| format!("__realm_provide: {error}"))?;
    }

    {
        let realms = realms.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_kill",
                1,
                move |_: &mut Scope<'_>, id: f64| {
                    if id >= 0.0 {
                        realms.kill(id as usize);
                    }
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__realm_kill: {error}"))?;
    }

    {
        let realms = realms.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_alive",
                1,
                move |_: &mut Scope<'_>, id: f64| {
                    Ok::<Value, RuntimeError>(Value::Bool(id >= 0.0 && realms.alive(id as usize)))
                },
            )
            .map_err(|error| format!("__realm_alive: {error}"))?;
    }

    {
        let realms = realms.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_begin",
                1,
                move |_: &mut Scope<'_>, id: f64| {
                    if id >= 0.0 {
                        realms.begin(id as usize);
                    }
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__realm_begin: {error}"))?;
    }

    {
        let realms = realms.clone();
        driver
            .context()
            .define_function(
                &global,
                "__realm_end",
                0,
                move |_: &mut Scope<'_>| {
                    realms.end();
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__realm_end: {error}"))?;
    }

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
    driver.host.realms = realms;
    let names = ["createRealm", "Realm", "default"];
    let exports = super::collect(driver, &module, &names)?;
    super::define_name(driver, "xmas:realm", exports)
}
