//! Module table: the engine only recognizes source code that has already been compiled; this layer handles "where it goes after compilation".
//!
//! Async resolution, caching, and JS hooks are all in a higher layer (the loader); here we only keep a static table,
//! so the request path does not need to touch the compiler again, nor does it need await.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{
    ModuleImportAttributes, ModuleLoader, ModuleScope, ModuleSource, NativeErrorKind, Value,
};

use crate::compile::{Kind, compile, kind_of};

#[derive(Debug)]
pub struct Module {
    pub specifier: String,
    pub path: String,
    pub kind: Kind,
    pub code: String,
    /// Whether it has gone through the compiler. Pure js passthrough is false.
    pub compiled: bool,
}

#[derive(Clone)]
pub struct Modules {
    sources: Rc<RefCell<BTreeMap<String, Rc<Module>>>>,
    /// The names the engine itself knows: registered as module definitions rather than as source
    /// (every `node:` module this runtime ships). The loader must leave those alone.
    native: Rc<RefCell<BTreeSet<String>>>,
    /// Modules an embedder handed over as values (a realm's `provide`), in the loader's terms.
    ///
    /// The same values are also defined in the engine, which is what makes a replacement visible to
    /// an import that came earlier. This table exists for the window before that happens: a
    /// `provide` followed by an `evaluate` in one turn loads the entry module while the engine is
    /// still entered, and this is what answers it.
    provided: Rc<RefCell<BTreeMap<String, Vec<(String, Value)>>>>,
    /// What `import.meta` needs beyond a module's own path: the entry, and the environment.
    /// Shared, so the answer does not change between the first module and the last one compiled.
    context: Rc<RefCell<crate::meta::Context>>,
}

impl Default for Modules {
    fn default() -> Self {
        Self::new()
    }
}

impl Modules {
    pub fn new() -> Self {
        Modules {
            sources: Rc::new(RefCell::new(BTreeMap::new())),
            native: Rc::new(RefCell::new(BTreeSet::new())),
            provided: Rc::new(RefCell::new(BTreeMap::new())),
            context: Rc::new(RefCell::new(crate::meta::Context::default())),
        }
    }

    /// Record that `name` is a module the engine defines itself, so resolution does not go looking
    /// for a file of that name. Every place that defines a module (crate::modules and the test
    /// harness) says so here; the loader cannot see the engine's own registry.
    pub fn declare_native(&self, name: &str) {
        self.native.borrow_mut().insert(name.to_owned());
    }

    pub fn is_native(&self, name: &str) -> bool {
        self.native.borrow().contains(name)
    }

    /// Makes `exports` importable as `name` before the engine is free to define it.
    ///
    /// From here on the name means these values for this driver: a package of the same name on disk
    /// is not looked for. That is the point -- an embedder providing "eta" means it -- but it also
    /// means a name cannot be given back.
    pub fn provide(&self, name: &str, exports: Vec<(String, Value)>) {
        self.provided.borrow_mut().insert(name.to_owned(), exports);
    }

    /// Whether `name` was handed over rather than found on disk or in the engine.
    fn is_provided(&self, name: &str) -> bool {
        self.provided.borrow().contains_key(name)
    }

    /// The exports provided for `name`, if any.
    fn provided(&self, name: &str) -> Option<Vec<(String, Value)>> {
        self.provided.borrow().get(name).cloned()
    }

    /// The module the program started at, which is what `import.meta.main` compares against.
    ///
    /// Set once per realm, before anything is evaluated: a module compiled while the entry was
    /// still unknown would answer false for the rest of its life, and it is compiled once.
    pub fn set_main(&self, name: &str) {
        self.context.borrow_mut().main = Some(name.to_owned());
    }

    /// The environment `import.meta.env` answers from.
    pub fn set_env(&self, vars: Vec<(String, String)>) {
        self.context.borrow_mut().env = Some(vars.into_iter().collect());
    }

    /// Install a module: compilation happens here, and only happens once.
    pub fn load(&self, specifier: &str, path: &str, source: &str) -> Result<Rc<Module>, String> {
        let kind = kind_of(path);
        // JSON is a module whose only export is the value.
        let code = if path.ends_with(".json") {
            format!("export default {};", source.trim_start_matches('\u{feff}'))
        } else {
            compile(path, source, &self.context.borrow())?
        };
        // A CommonJS file gets its shell: require, module, exports, __filename and __dirname.
        let code = crate::cjs::wrap(path, &code, &|name| {
            self.is_native(name) || self.get(name).is_some()
        })
        .unwrap_or(code);
        let compiled = kind != Kind::Js;
        let module = Rc::new(Module {
            specifier: specifier.to_owned(),
            path: path.to_owned(),
            kind,
            code,
            compiled,
        });
        self.sources
            .borrow_mut()
            .insert(specifier.to_owned(), Rc::clone(&module));
        Ok(module)
    }

    /// The module at `path`, read from disk the first time and from the table after that.
    ///
    /// One realm reads and compiles its own copy: the code sits in that realm's heap, and a table is
    /// not something to hand across threads.
    pub fn load_file(&self, path: &str) -> Result<Rc<Module>, String> {
        if let Some(module) = self.get(path) {
            return Ok(module);
        }
        // A URL is fetched rather than read, and what came back decides how it is compiled: a wasm
        // body is a module of a different kind, and the media type is the only honest signal. The
        // URL's own last dot says nothing -- unpkg serves .ts as text/plain.
        if crate::resolve::is_remote(path) {
            let fetched = crate::remote::body(path)?;
            // A wasm body is not a module of this kind: `WebAssembly.instantiateStreaming` is what
            // takes one, and it reads a fetch response rather than an import. Saying so beats
            // handing the bytes to the JS compiler and reporting its confusion.
            if fetched.media_type == "application/wasm" {
                return Err(format!(
                    "{path}: this is a wasm module; load it with WebAssembly.instantiateStreaming"
                ));
            }
            let source = String::from_utf8(fetched.bytes)
                .map_err(|_| format!("{path}: the body is not UTF-8"))?;
            return self.load(path, path, &source);
        }
        let source = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
        self.load(path, path, &source)
    }

    pub fn get(&self, specifier: &str) -> Option<Rc<Module>> {
        self.sources.borrow().get(specifier).cloned()
    }

    pub fn all(&self) -> Vec<Rc<Module>> {
        self.sources.borrow().values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.sources.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.borrow().is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.sources
            .borrow()
            .values()
            .map(|module| module.code.len())
            .sum()
    }
}

/// The realm's module loader: a name in, a compiled source out.
///
/// The engine asks about a name twice: once to normalize a specifier against the module that wrote
/// it, once to get the source. Resolution is `crate::resolve`; the reading and the compiling happen
/// here, once per module per realm.
///
/// A name already in the table answers before resolution is tried at all, which is what lets a
/// native module or a test install a source under a plain name and import it as that name.
pub struct Loader {
    modules: Modules,
    /// Packages an import already asked the package manager for. One realm asks once: a second
    /// request for the same name would only fail the same way.
    asked: std::collections::HashSet<String>,
}

impl Loader {
    pub fn new(modules: Modules) -> Self {
        Loader {
            modules,
            asked: std::collections::HashSet::new(),
        }
    }
}

impl ModuleLoader for Loader {
    fn normalize(
        &mut self,
        scope: &mut ModuleScope<'_>,
        referrer: &str,
        specifier: &str,
        _attributes: &ModuleImportAttributes,
    ) -> Result<String, Value> {
        if self.modules.is_native(specifier)
            || self.modules.is_provided(specifier)
            || self.modules.get(specifier).is_some()
        {
            return Ok(specifier.to_owned());
        }
        match crate::resolve::resolve(referrer, specifier) {
            Ok(path) => Ok(path),
            // Nothing on disk. If the specifier names a package, the import is a request for it:
            // install it and resolve again. Anything else keeps the first reason, which says where
            // the file was looked for.
            Err(reason) => {
                let package = crate::deps::package_of(specifier);
                let wanted = package
                    .as_ref()
                    .map(|package| self.asked.insert(package.clone()))
                    .unwrap_or(false);
                if !wanted {
                    return Err(native(scope, &reason));
                }
                match crate::deps::fetch(specifier, referrer) {
                    crate::deps::Outcome::Installed => match crate::resolve::resolve(referrer, specifier)
                    {
                        Ok(path) => Ok(path),
                        Err(reason) => Err(native(scope, &reason)),
                    },
                    crate::deps::Outcome::NotAPackage => Err(native(scope, &reason)),
                    crate::deps::Outcome::Failed(why) => {
                        Err(native(scope, &format!("{why}\n(the import was '{specifier}' from {referrer}, and {reason})")))
                    }
                }
            }
        }
    }

    fn load(
        &mut self,
        scope: &mut ModuleScope<'_>,
        name: &str,
        _attributes: &ModuleImportAttributes,
    ) -> Result<ModuleSource, Value> {
        // Values, not source: handed over as they stand, so nothing is read or compiled.
        if let Some(exports) = self.modules.provided(name) {
            return Ok(ModuleSource::Synthetic { exports });
        }
        match self.modules.load_file(name) {
            Ok(module) => Ok(ModuleSource::source(module.code.clone().into_bytes())),
            Err(reason) => Err(native(scope, &reason)),
        }
    }
}

/// The error a failed import reports: a TypeError, its message the reason resolution or reading gave.
fn native(scope: &mut ModuleScope<'_>, reason: &str) -> Value {
    scope
        .new_native_error(NativeErrorKind::Type, reason)
        .unwrap_or(Value::Undefined)
}
