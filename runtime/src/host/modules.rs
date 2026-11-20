//! Module table: the engine only recognizes source code that has already been compiled; this layer handles "where it goes after compilation".
//!
//! Async resolution, caching, and JS hooks are all in a higher layer (the loader); here we only keep a static table,
//! so the request path does not need to touch the compiler again, nor does it need await.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

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
pub struct Modules(Rc<RefCell<BTreeMap<String, Rc<Module>>>>);

impl Default for Modules {
    fn default() -> Self {
        Self::new()
    }
}

impl Modules {
    pub fn new() -> Self {
        Modules(Rc::new(RefCell::new(BTreeMap::new())))
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
        self.0
            .borrow_mut()
            .insert(specifier.to_owned(), Rc::clone(&module));
        Ok(module)
    }

    pub fn get(&self, specifier: &str) -> Option<Rc<Module>> {
        self.0.borrow().get(specifier).cloned()
    }

    pub fn all(&self) -> Vec<Rc<Module>> {
        self.0.borrow().values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.0.borrow().values().map(|module| module.code.len()).sum()
    }
}
