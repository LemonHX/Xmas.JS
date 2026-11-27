//! node:child_process: spawn, exec, execFile, and the blocking versions of each.
//!
//! The process itself is `host/exec.rs`, the same file `$` runs on; what differs is the shape the
//! caller sees. Node hands back a ChildProcess with a Readable and a Writable on it and events,
//! so `js/child_process.js` builds those out of the same read/write/wait promises and this file
//! is the bridge.
//!
//! One deliberate gap, in docs/child-process.md: `stdio` is not configurable, because a child's third stream would have to be
//! a socket this host passes through by hand.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::exec::{self, Recipe, Running, Stdin};
use crate::host::pending::{self, Payload};
use crate::modules::buffer::base64_encode;
use crate::modules::{bytes, throw};

const PRELUDE: &str = include_str!("../../js/child_process.js");

/// What a read resolves with when the pipe has closed.
const EOF: &str = "eof";

/// A signal as the name Node takes ('SIGTERM', 'TERM'), a number, or nothing (which means SIGTERM).
#[cfg(unix)]
fn signal_number(text: &str) -> Option<i32> {
    if text.is_empty() {
        return Some(libc::SIGTERM);
    }
    if let Ok(number) = text.parse::<i32>() {
        return Some(number);
    }
    let upper = text.to_uppercase();
    let name = upper.strip_prefix("SIG").unwrap_or(&upper);
    Some(match name {
        "HUP" => libc::SIGHUP,
        "INT" => libc::SIGINT,
        "QUIT" => libc::SIGQUIT,
        "ILL" => libc::SIGILL,
        "TRAP" => libc::SIGTRAP,
        "ABRT" => libc::SIGABRT,
        "BUS" => libc::SIGBUS,
        "FPE" => libc::SIGFPE,
        "KILL" => libc::SIGKILL,
        "USR1" => libc::SIGUSR1,
        "SEGV" => libc::SIGSEGV,
        "USR2" => libc::SIGUSR2,
        "PIPE" => libc::SIGPIPE,
        "ALRM" => libc::SIGALRM,
        "TERM" => libc::SIGTERM,
        "CHLD" => libc::SIGCHLD,
        "CONT" => libc::SIGCONT,
        "STOP" => libc::SIGSTOP,
        "TSTP" => libc::SIGTSTP,
        "TTIN" => libc::SIGTTIN,
        "TTOU" => libc::SIGTTOU,
        _ => return None,
    })
}

#[cfg(not(unix))]
fn signal_number(text: &str) -> Option<i32> {
    if text.is_empty() { return Some(15); }
    Some(text.parse::<i32>().unwrap_or(9))
}

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
                "__cp_spawn",
                7,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      program: String,
                      arguments: String,
                      cwd: String,
                      env: String,
                      through_shell: bool,
                      stdio: String|
                      -> Result<Value, RuntimeError> {
                    let arguments = match parse_arguments(&arguments) {
                        Ok(arguments) => arguments,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let env = match parse_env(&env) {
                        Ok(env) => env,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                    let recipe = if through_shell {
                        Recipe::Shell(&program)
                    } else {
                        Recipe::Argv(&program, &arguments)
                    };
                    // "inherit" sends the child's output to this process's own, which is what
                    // a program that talks to the terminal needs; anything else is captured.
                    let stdout = if stdio == "inherit" {
                        exec::Stdout::Inherit
                    } else {
                        exec::Stdout::Pipe
                    };
                    let stdin = match stdio.as_str() {
                        "inherit" | "ignore" => Stdin::Null,
                        _ => Stdin::Pipe,
                    };
                    match exec::spawn_with_stdout(recipe, cwd, &env, stdin, stdout) {
                        Ok(running) => {
                            state.children.borrow_mut().insert(id as u64, running);
                            Ok(Value::Undefined)
                        }
                        Err(message) => Err(throw(scope, &message)),
                    }
                },
            )
            .map_err(|error| format!("__cp_spawn: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__cp_read",
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
                        None => return Err(throw(scope, "no such child")),
                    };
                    pending::start(scope, &pending, async move {
                        match reading.recv().await {
                            Ok(bytes) => Ok(Payload::Bytes(bytes)),
                            Err(_) => Ok(Payload::Text(EOF.to_string())),
                        }
                    })
                },
            )
            .map_err(|error| format!("__cp_read: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__cp_write",
                2,
                move |scope: &mut Scope<'_>, id: f64, data: Value| -> Result<Value, RuntimeError> {
                    let data = bytes::take(scope, &data)?;
                    let writing = state
                        .children
                        .borrow()
                        .get(&(id as u64))
                        .and_then(|child| child.stdin.clone());
                    let writing = match writing {
                        Some(writing) => writing,
                        None => return Err(throw(scope, "the child's input is closed")),
                    };
                    pending::start(scope, &pending, async move {
                        match writing.send(data).await {
                            Ok(()) => Ok(Payload::Text("ok".to_string())),
                            Err(_) => Err("the child is gone".to_string()),
                        }
                    })
                },
            )
            .map_err(|error| format!("__cp_write: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__cp_end",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    if let Some(child) = state.children.borrow_mut().get_mut(&(id as u64)) {
                        // Dropping the sender is how the pipe is closed.
                        child.stdin = None;
                    }
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__cp_end: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__cp_wait",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let status = state
                        .children
                        .borrow()
                        .get(&(id as u64))
                        .map(|child| child.status.clone());
                    let status = match status {
                        Some(status) => status,
                        None => return Err(throw(scope, "no such child")),
                    };
                    pending::start(scope, &pending, async move {
                        match status.recv().await {
                            Ok(text) => Ok(Payload::Text(text)),
                            Err(_) => Err("the child is gone".to_string()),
                        }
                    })
                },
            )
            .map_err(|error| format!("__cp_wait: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__cp_kill",
                2,
                move |scope: &mut Scope<'_>, id: f64, signal: String| -> Result<Value, RuntimeError> {
                    let number = match signal_number(&signal) {
                        Some(number) => number,
                        None => {
                            return Err(throw(scope, &format!("Unknown signal: {signal}")));
                        }
                    };
                    match state.children.borrow().get(&(id as u64)) {
                        Some(child) => match child.signal(number) {
                            Ok(()) => Ok(Value::Bool(true)),
                            Err(message) => Err(throw(scope, &message)),
                        },
                        None => Err(throw(scope, "no such child")),
                    }
                },
            )
            .map_err(|error| format!("__cp_kill: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__cp_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    state.children.borrow_mut().remove(&(id as u64));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__cp_close: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__cp_capture",
                6,
                move |scope: &mut Scope<'_>,
                      program: String,
                      arguments: String,
                      cwd: String,
                      env: String,
                      through_shell: bool,
                      max_buffer: f64|
                      -> Result<Value, RuntimeError> {
                    let arguments = match parse_arguments(&arguments) {
                        Ok(arguments) => arguments,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let env = match parse_env(&env) {
                        Ok(env) => env,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                    let recipe = if through_shell {
                        Recipe::Shell(&program)
                    } else {
                        Recipe::Argv(&program, &arguments)
                    };
                    // This is what "synchronous" means: the realm thread waits, and nothing else in
                    // this process does.
                    let captured = match exec::capture(recipe, cwd, &env, None) {
                        Ok(captured) => captured,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let limit = if max_buffer > 0.0 {
                        max_buffer as usize
                    } else {
                        usize::MAX
                    };
                    if captured.stdout.len() > limit || captured.stderr.len() > limit {
                        return Ok(Value::String(
                            quickjs_oxide::engine::api::core::JsString::try_from_utf8(
                                "{\"overflow\":true}",
                            )
                            .expect("constant"),
                        ));
                    }
                    let report = format!(
                        "{{\"stdout\":\"{}\",\"stderr\":\"{}\",\"status\":{}}}",
                        base64_encode(&captured.stdout),
                        base64_encode(&captured.stderr),
                        captured.status
                    );
                    let value = match quickjs_oxide::engine::api::core::JsString::try_from_utf8(&report)
                    {
                        Ok(value) => value,
                        Err(_) => return Err(throw(scope, "the output could not be read")),
                    };
                    Ok(Value::String(value))
                },
            )
            .map_err(|error| format!("__cp_capture: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("child_process prelude: {error}"))?;
    let module = driver
        .context()
        .eval("__child_process_module")
        .map_err(|error| format!("__child_process_module: {error}"))?;
    let exports = super::collect(
        driver,
        &module,
        &[
            "spawn",
            "exec",
            "execFile",
            "spawnSync",
            "execSync",
            "execFileSync",
            "ChildProcess",
            "default",
        ],
    )?;
    super::define_name(driver, "child_process", exports.clone())?;
    super::define_name(driver, "node:child_process", exports)
}

/// The arguments array JS sends, as the strings the host will pass to the program.
fn parse_arguments(json: &str) -> Result<Vec<String>, String> {
    if json.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("arguments: {error}"))?;
    let items = match value.as_array() {
        Some(items) => items,
        None => return Err("arguments: expected an array".to_string()),
    };
    let mut arguments = Vec::with_capacity(items.len());
    for item in items {
        match item.as_str() {
            Some(item) => arguments.push(item.to_string()),
            None => arguments.push(item.to_string()),
        }
    }
    Ok(arguments)
}

/// The environment additions, string values only.
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
