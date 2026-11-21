//! `$`: one line of shell, run as a command, the way zx's `$` runs it.
//!
//! The split is the one sockets uses: a command is an id on both sides. Rust owns the process
//! (`host/exec.rs` holds the child, a thread per pipe and the watcher), `js/exec.js` owns the
//! quoting and the promises. Nothing here buffers output the caller did not ask for: a read is a
//! promise that resolves when the child writes, and it resolves with "eof" when the pipe closes.
//!
//! There is no stdin yet. A command that wants input is a command that needs the `| file` form of
//! the shell, and that arrives with the piping this first version does not have.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::exec::{self, Running};
use crate::host::pending::{self, Payload};
use crate::modules::throw;

const PRELUDE: &str = include_str!("../../js/exec.js");

/// What a read resolves with when the pipe has closed.
const EOF: &str = "eof";

/// The commands of this realm, by id.
#[derive(Default)]
struct State {
    children: RefCell<BTreeMap<u64, Running>>,
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let pending = driver.host.pending.clone();
    let state = Rc::new(State::default());

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__exec_spawn",
                4,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      command: String,
                      cwd: String,
                      env: String|
                      -> Result<Value, RuntimeError> {
                    let env = match parse_env(&env) {
                        Ok(env) => env,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                    let recipe = exec::Recipe::Shell(&command);
                    match exec::spawn(recipe, cwd, &env, exec::Stdin::Null) {
                        Ok(running) => {
                            state.children.borrow_mut().insert(id as u64, running);
                            Ok(Value::Undefined)
                        }
                        Err(message) => Err(throw(scope, &message)),
                    }
                },
            )
            .map_err(|error| format!("__exec_spawn: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__exec_read",
                2,
                move |scope: &mut Scope<'_>, id: f64, which: String| -> Result<Value, RuntimeError> {
                    let reading = state.children.borrow().get(&(id as u64)).map(|child| {
                        if which == "stderr" {
                            child.stderr.clone()
                        } else {
                            child.stdout.clone()
                        }
                    });
                    let reading = match reading {
                        Some(reading) => reading,
                        None => return Err(throw(scope, "no such command")),
                    };
                    pending::start(scope, &pending, async move {
                        match reading.recv().await {
                            Ok(bytes) => Ok(Payload::Bytes(bytes)),
                            Err(_) => Ok(Payload::Text(EOF.to_string())),
                        }
                    })
                },
            )
            .map_err(|error| format!("__exec_read: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__exec_wait",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let status = state
                        .children
                        .borrow()
                        .get(&(id as u64))
                        .map(|child| child.status.clone());
                    let status = match status {
                        Some(status) => status,
                        None => return Err(throw(scope, "no such command")),
                    };
                    pending::start(scope, &pending, async move {
                        match status.recv().await {
                            Ok(text) => Ok(Payload::Text(text)),
                            Err(_) => Err("the command is gone".to_string()),
                        }
                    })
                },
            )
            .map_err(|error| format!("__exec_wait: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__exec_kill",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    match state.children.borrow().get(&(id as u64)) {
                        Some(child) => match child.kill() {
                            Ok(()) => Ok(Value::Bool(true)),
                            Err(message) => Err(throw(scope, &message)),
                        },
                        None => Err(throw(scope, "no such command")),
                    }
                },
            )
            .map_err(|error| format!("__exec_kill: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__exec_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    state.children.borrow_mut().remove(&(id as u64));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__exec_close: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("exec prelude: {error}"))?;

    Ok(())
}

/// The environment a command adds, as the JSON object JS sends: string values only, because the
/// platform's own launcher takes nothing else.
fn parse_env(json: &str) -> Result<Vec<(String, String)>, String> {
    if json.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("environment: {error}"))?;
    let object = match value.as_object() {
        Some(object) => object,
        None => return Err("environment: expected an object".to_string()),
    };
    let mut entries = Vec::with_capacity(object.len());
    for (name, value) in object {
        match value.as_str() {
            Some(value) => entries.push((name.clone(), value.to_string())),
            None => return Err(format!("environment: {name} is not a string")),
        }
    }
    Ok(entries)
}
