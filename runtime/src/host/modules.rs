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

    /// Install a module: compilation happens here, and only happens once.
    pub fn load(&self, specifier: &str, path: &str, source: &str) -> Result<Rc<Module>, String> {
        let kind = kind_of(path);
        let code = compile(path, source)?;
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
}

impl Loader {
    pub fn new(modules: Modules) -> Self {
        Loader { modules }
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
        if self.modules.is_native(specifier) || self.modules.get(specifier).is_some() {
            return Ok(specifier.to_owned());
        }
        match crate::resolve::resolve(referrer, specifier) {
            Ok(path) => Ok(path),
            Err(reason) => Err(native(scope, &reason)),
        }
    }

    fn load(
        &mut self,
        scope: &mut ModuleScope<'_>,
        name: &str,
        _attributes: &ModuleImportAttributes,
    ) -> Result<ModuleSource, Value> {
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
