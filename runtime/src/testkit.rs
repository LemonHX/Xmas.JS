//! Test harness: replaces the AsyncContext::full setup in modules/src/utils/test.rs.
//!
//! The old harness wrapped the realm in rquickjs's AsyncContext; tests wrote an async closure,
//! inside which they awaited module evaluation and awaited the promise returned by a function. The new engine has no such thing:
//! everything is "run a batch of jobs, see whether the promise has settled," so await here means
//! "drive the loop until the promise has a result."
//!
//! Comparison table:
//!
//! | old harness | here |
//! | --- | --- |
//! | given_runtime() | Harness::new() |
//! | test_async_with / test_sync_with | Harness::eval + settle |
//! | call_test | Harness::call + settle |
//! | call_test_err | Harness::call_err |
//! | ModuleEvaluator::eval_js | Harness::module |
//! | ModuleEvaluator::eval_rust（ModuleDef） | Harness::define_module + rust_function |
//! | given_file | given_file |
//!
//! The module loader is real too: set_module_loader installs a loader that looks up modules by name in the src/host/modules.rs
//! table, the same shape as the loader used in production later.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use quickjs_oxide::engine::api::core::{
    FunctionOptions, Js, JsString, ModuleImportAttributes, ModuleLoader, ModuleScope, ModuleSource,
    NativeErrorKind, ObjectRef, PromiseSnapshot, PromiseState, PropertyKey, Scope, Value,
};

use crate::driver::Driver;
use crate::host::{Host, Modules};

/// How long a test waits at most. If it times out, report an error; don't let CI hang.
pub const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Thrown {
    pub name: String,
    pub message: String,
    pub text: String,
}

/// Get a module from the table in src/host/modules.rs. Same shape as the production loader:
/// name in, compiled source out; the loader does not run JS.
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
                .new_native_error(NativeErrorKind::Type, &format!("Cannot find module '{name}'"))
                .unwrap_or(Value::Undefined)),
        }
    }
}

pub struct Harness {
    driver: Driver,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    pub fn new() -> Self {
        Self::with_host(Host::capture())
    }

    pub fn with_host(host: Host) -> Self {
        let modules = host.modules.clone();
        let driver = Driver::new(host).expect("build runtime");
        driver
            .runtime()
            .set_module_loader(TableLoader { modules })
            .expect("install loader");
        Harness { driver }
    }

    /// Install the modules carried over from the old runtime (console, timers, ...).
    pub fn with_modules() -> Self {
        Self::with_heap_and_modules(crate::driver::DEFAULT_HEAP)
    }

    /// Same, with an explicit heap cap: mark-sweep pauses grow with the live set,
    /// so a module that allocates per call needs room to breathe.
    pub fn with_heap_and_modules(bytes: usize) -> Self {
        let host = Host::capture();
        let modules = host.modules.clone();
        let driver = crate::driver::Driver::with_heap(host, bytes).expect("build runtime");
        driver
            .runtime()
            .set_module_loader(TableLoader { modules })
            .expect("install loader");
        let mut harness = Harness { driver };
        crate::modules::install(&mut harness.driver).expect("install modules");
        harness
    }


    pub fn driver(&mut self) -> &mut Driver {
        &mut self.driver
    }

    pub fn host(&self) -> &Host {
        &self.driver.host
    }

    // ---------- synchronous evaluation ----------

    pub fn eval(&mut self, source: &str) -> Result<Value, String> {
        self.driver.eval(source)
    }

    pub fn expect_number(&mut self, source: &str) -> f64 {
        match self.eval(source) {
            Ok(value) => number_of(&value).unwrap_or_else(|| panic!("{source} is not a number: {value:?}")),
            Err(error) => panic!("{source} failed: {error}"),
        }
    }

    pub fn expect_text(&mut self, source: &str) -> String {
        match self.eval(source) {
            Ok(value) => text_of(&value).unwrap_or_else(|| panic!("{source} is not a string: {value:?}")),
            Err(error) => panic!("{source} failed: {error}"),
        }
    }

    /// Expect an error to be thrown, and get name / message. The old call_test_err spot.
    pub fn eval_err(&mut self, source: &str) -> Thrown {
        match self.driver.eval_raw(source) {
            Ok(value) => panic!("expected an error, but got {value:?}"),
            Err(error) => self.describe(error),
        }
    }

    // ---------- modules ----------

    /// Install a module implemented in Rust. This replaces the old harness's ModuleDef.
    pub fn define_module(&mut self, name: &str, exports: Vec<(String, Value)>) {
        self.driver
            .context_ref()
            .define_module(name, exports)
            .unwrap_or_else(|error| panic!("failed to define module {name}: {error}"));
    }

    /// Create a Rust function that can be passed into define_module. At most eight parameters; the types must be FromJs.
    pub fn rust_function<Args>(
        &mut self,
        name: &str,
        length: u32,
        body: impl quickjs_oxide::engine::api::core::HostFunction<Args>,
    ) -> Value {
        let options = FunctionOptions::new(name).length(length);
        let callable = self
            .driver
            .context()
            .new_function(options, body)
            .unwrap_or_else(|error| panic!("failed to create function {name}: {error}"));
        Value::from(callable)
    }

    /// Compile and execute a module, returning its namespace. The name also serves as the path; the extension determines how it is compiled.
    ///
    /// You cannot directly use execute_module's return value: the object it returns cannot read exported names
    /// (in practice, get_property is all undefined). The namespace must be imported out yourself,
    /// so here a bootstrap module attaches import * as ns to global.
    pub fn module(&mut self, name: &str, source: &str) -> Result<Value, String> {
        if name.contains('\'') {
            return Err(format!("module name must not contain single quotes: {name}"));
        }
        let module = self
            .driver
            .host
            .modules
            .load(name, name, source)
            .map_err(|error| format!("failed to compile {name}: {error}"))?;
        let context = self.driver.context();
        context
            .compile_module_with_filename(&module.code, name)
            .map_err(|error| format!("failed to compile {name}: {error}"))?;
        let bootstrap = format!("import * as ns from '{name}'; globalThis.__xmas_ns = ns;");
        let bytecode = context
            .compile_module_with_filename(&bootstrap, "xmas:testkit-bootstrap")
            .map_err(|error| format!("failed to bootstrap module: {error}"))?;
        context
            .execute_module(&bytecode)
            .map_err(|error| format!("failed to execute {name}: {error}"))?;
        self.driver.eval("globalThis.__xmas_ns")
    }

    /// Read a property on the namespace (or any object).
    pub fn property(&mut self, object: &Value, name: &str) -> Result<Value, String> {
        let Value::Object(object) = object else {
            return Err("not an object".into());
        };
        let key = PropertyKey::from_js_string(
            JsString::try_from_utf8(name).map_err(|error| format!("{error}"))?,
        );
        self.driver
            .context()
            .get_property(object, &key)
            .map_err(|error| format!("{name}: {error}"))
    }

    /// Get a function from a module and call it. The old call_test spot.
    pub fn call(&mut self, namespace: &Value, function: &str, args: &[Value]) -> Result<Value, String> {
        let this = namespace.clone();
        let value = self.property(&this, function)?;
        let context = self.driver.context();
        let Value::Object(callable_object) = &value else {
            return Err(format!("{function} is not a function"));
        };
        let callable = context
            .as_callable(callable_object)
            .map_err(|error| format!("{function}: {error}"))?
            .ok_or_else(|| format!("{function} is not a function"))?;
        context
            .call(&callable, this, args)
            .map_err(|error| format!("failed to call {function}: {error}"))
    }

    /// Call a function, expecting it to throw. The old call_test_err.
    pub fn call_err(&mut self, namespace: &Value, function: &str, args: &[Value]) -> Thrown {
        let this = namespace.clone();
        let value = self.property(&this, function).expect("get function");
        let context = self.driver.context();
        let Value::Object(callable_object) = &value else {
            panic!("{function} is not a function");
        };
        let callable = context
            .as_callable(callable_object)
            .expect("get callable")
            .expect("callable");
        match context.call(&callable, this, args) {
            Ok(value) => panic!("expected an error, but got {value:?}"),
            Err(error) => self.describe(error),
        }
    }

    // ---------- promise ----------

    /// If it is a promise, run until it settles; otherwise return it as is.
    pub async fn settle(&mut self, value: Value) -> Result<Value, String> {
        let object: ObjectRef = match &value {
            Value::Object(object) => object.clone(),
            _ => return Ok(value),
        };
        let Some(snapshot) = self.driver.promise(&object)? else {
            return Ok(value);
        };
        if !matches!(snapshot.state(), PromiseState::Pending) {
            return finish(snapshot);
        }

        let budget = Instant::now() + TIMEOUT;
        loop {
            self.driver.run_until_idle().await?;
            let Some(snapshot) = self.driver.promise(&object)? else {
                return Ok(value);
            };
            if !matches!(snapshot.state(), PromiseState::Pending) {
                return finish(snapshot);
            }
            if Instant::now() >= budget {
                return Err("promise never settled".into());
            }
            self.driver.wait_ready().await?;
        }
    }

    // ---------- small utilities ----------

    fn describe(&mut self, error: quickjs_oxide::engine::api::core::RuntimeError) -> Thrown {
        use quickjs_oxide::engine::api::core::RuntimeError;
        let RuntimeError::Exception(value) = error else {
            let text = format!("{error}");
            return Thrown {
                name: "RuntimeError".into(),
                message: text.clone(),
                text,
            };
        };
        let mut name = String::new();
        let mut message = String::new();
        if let Value::Object(object) = &value {
            let context = self.driver.context();
            for (field, slot) in [("name", &mut name), ("message", &mut message)] {
                let key = PropertyKey::from_js_string(JsString::from_static(field));
                if let Ok(Value::String(text)) = context.get_property(object, &key) {
                    *slot = text.to_utf8_lossy();
                }
            }
        }
        // What is thrown is usually an Error object: text uses "name: message"; only if there is no message, fall back to the debug form of the value
        let text = if message.is_empty() {
            format!("{value:?}")
        } else if name.is_empty() {
            message.clone()
        } else {
            format!("{name}: {message}")
        };
        Thrown {
            name,
            message,
            text,
        }
    }
}

fn finish(snapshot: PromiseSnapshot) -> Result<Value, String> {
    match snapshot.state() {
        PromiseState::Fulfilled => Ok(snapshot.result().clone()),
        PromiseState::Rejected => Err(format!("promise was rejected: {:?}", snapshot.result())),
        PromiseState::Pending => Err("promise is still pending".into()),
    }
}

pub fn number_of(value: &Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(*value as f64),
        Value::Float(value) => Some(*value),
        _ => None,
    }
}

pub fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_utf8_lossy()),
        _ => None,
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Old given_file: write a temporary file and leave it alone after the test.
pub fn given_file(content: &str) -> PathBuf {
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "xmas-test-{}-{unique}",
        std::process::id()
    ));
    std::fs::write(&path, content).expect("write temporary file");
    path
}

/// A minimal Rust module function, used as a fixture for define_module.
pub fn constant(_scope: &mut Scope<'_>) -> Result<Value, quickjs_oxide::engine::api::core::RuntimeError> {
    Ok(Value::Int(42))
}
