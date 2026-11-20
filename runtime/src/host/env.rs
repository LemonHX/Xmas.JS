//! Environment variables, working directory, and startup arguments.
//!
//! It takes a snapshot of the process environment: the values read on the JS side do not change during a single run,
//! and tests can also inject a fake one.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

struct Inner {
    vars: BTreeMap<String, String>,
    cwd: PathBuf,
    args: Vec<String>,
}

#[derive(Clone)]
pub struct Env(Rc<RefCell<Inner>>);

impl Env {
    pub fn snapshot() -> Self {
        Env(Rc::new(RefCell::new(Inner {
            vars: std::env::vars().collect(),
            cwd: std::env::current_dir().unwrap_or_default(),
            args: std::env::args().collect(),
        })))
    }

    pub fn empty() -> Self {
        Env(Rc::new(RefCell::new(Inner {
            vars: BTreeMap::new(),
            cwd: std::env::current_dir().unwrap_or_default(),
            args: Vec::new(),
        })))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.0.borrow().vars.get(key).cloned()
    }

    pub fn set(&self, key: &str, value: &str) {
        self.0
            .borrow_mut()
            .vars
            .insert(key.to_owned(), value.to_owned());
    }

    pub fn remove(&self, key: &str) -> Option<String> {
        self.0.borrow_mut().vars.remove(key)
    }

    pub fn len(&self) -> usize {
        self.0.borrow().vars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().vars.is_empty()
    }

    pub fn all(&self) -> Vec<(String, String)> {
        self.0
            .borrow()
            .vars
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub fn cwd(&self) -> PathBuf {
        self.0.borrow().cwd.clone()
    }

    pub fn set_cwd(&self, cwd: PathBuf) {
        self.0.borrow_mut().cwd = cwd;
    }

    pub fn args(&self) -> Vec<String> {
        self.0.borrow().args.clone()
    }
}
