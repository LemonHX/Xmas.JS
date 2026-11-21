//! `$`: one line of shell, run as a command, the way zx's `$` runs it.
//!
//! The split is the one sockets uses: a command is an id on both sides. Rust owns the process
//! (`host/exec.rs` holds the child, a thread per pipe and the watcher), `js/exec.js` owns the
//! quoting and the promises. Nothing here buffers output the caller did not ask for: a read is a
//! promise that resolves when the child writes, and it resolves with "eof" when the pipe closes.
//!
//! Input travels the same way, in the other direction: `$({ input })` and `.stdin()` write to
//! the child's stdin and then close it. The helpers around `$` (cd, glob, which, tmpdir,
//! minimist, dotenv, chalk, ps, kill) are JS in the same file, and are also reachable as
//! `Xmas.shell`, which is where this runtime keeps them: not under another project's name.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::exec::{self, Running};
use crate::host::pending::{self, Payload};
use crate::modules::buffer::base64_encode;
use crate::modules::{bytes, throw};

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
                5,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      command: String,
                      cwd: String,
                      env: String,
                      writing: bool|
                      -> Result<Value, RuntimeError> {
                    let env = match parse_env(&env) {
                        Ok(env) => env,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                    let recipe = exec::Recipe::Shell(&command);
                    // A pipe on stdin unless the caller has nothing to say: a command that reads
                    // from a closed stdin gets EOF at once, which is what a script wants.
                    let input = if writing { exec::Stdin::Pipe } else { exec::Stdin::Null };
                    match exec::spawn(recipe, cwd, &env, input) {
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


    {
        let state = Rc::clone(&state);
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__exec_write",
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
                        None => return Err(throw(scope, "the command's input is closed")),
                    };
                    pending::start(scope, &pending, async move {
                        match writing.send(data).await {
                            Ok(()) => Ok(Payload::Text("ok".to_string())),
                            Err(_) => Err("the command is gone".to_string()),
                        }
                    })
                },
            )
            .map_err(|error| format!("__exec_write: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__exec_end",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    if let Some(child) = state.children.borrow_mut().get_mut(&(id as u64)) {
                        // Dropping the sender is how the pipe is closed.
                        child.stdin = None;
                    }
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__exec_end: {error}"))?;
    }

    {
        // $.sync: the same line, run to its end before this returns. The realm thread waits, which
        // is exactly what a synchronous call is.
        driver
            .context()
            .define_function(
                &global,
                "__exec_capture",
                4,
                move |scope: &mut Scope<'_>,
                      command: String,
                      cwd: String,
                      env: String,
                      input: Value|
                      -> Result<Value, RuntimeError> {
                    let env = match parse_env(&env) {
                        Ok(env) => env,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    let cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                    let stdin = match &input {
                        Value::Undefined | Value::Null => None,
                        _ => Some(bytes::take(scope, &input)?),
                    };
                    let captured = match exec::capture(
                        exec::Recipe::Shell(&command),
                        cwd,
                        &env,
                        stdin.as_deref(),
                    ) {
                        Ok(captured) => captured,
                        Err(message) => return Err(throw(scope, &message)),
                    };
                    // status is already the JSON the watcher wrote: {"code":0,"signal":null}
                    let report = format!(
                        "{{\"stdout\":\"{}\",\"stderr\":\"{}\",\"status\":{}}}",
                        base64_encode(&captured.stdout),
                        base64_encode(&captured.stderr),
                        captured.status
                    );
                    match JsString::try_from_utf8(&report) {
                        Ok(value) => Ok(Value::String(value)),
                        Err(_) => Err(throw(scope, "the output could not be read")),
                    }
                },
            )
            .map_err(|error| format!("__exec_capture: {error}"))?;
    }

    {
        // This process's own stdin, for stdin() and question(). One reader at a time: the platform
        // hands out a shared handle, and two readers would take each other's bytes.
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__stdin_read_all",
                0,
                move |scope: &mut Scope<'_>| -> Result<Value, RuntimeError> {
                    pending::start(scope, &pending, async move {
                        let bytes = smol::unblock(|| {
                            let mut bytes = Vec::new();
                            std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
                                .map(|_| bytes)
                                .map_err(|error| error.to_string())
                        })
                        .await?;
                        Ok(Payload::Bytes(bytes))
                    })
                },
            )
            .map_err(|error| format!("__stdin_read_all: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__stdin_read_line",
                0,
                move |scope: &mut Scope<'_>| -> Result<Value, RuntimeError> {
                    pending::start(scope, &pending, async move {
                        let line = smol::unblock(|| {
                            let mut line = String::new();
                            match std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
                            {
                                Ok(0) => Ok(None),
                                Ok(_) => Ok(Some(line)),
                                Err(error) => Err(error.to_string()),
                            }
                        })
                        .await?;
                        match line {
                            Some(line) => Ok(Payload::Text(line)),
                            // Nothing left to read: a string, because null would be a promise that
                            // resolved with no value and the caller cannot tell that from empty.
                            None => Ok(Payload::Text(EOF.to_string())),
                        }
                    })
                },
            )
            .map_err(|error| format!("__stdin_read_line: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("exec prelude: {error}"))?;

    // The same objects as a module of this runtime's own, so a script can name where it gets them:
    // import { $, cd } from 'xmas:shell'. The globals stay, for the one-file script.
    let shell = driver
        .context()
        .eval("globalThis.Xmas.shell")
        .map_err(|error| format!("Xmas.shell: {error}"))?;
    let exports = super::collect(
        driver,
        &shell,
        &[
            "$", "cd", "within", "syncProcessCwd", "sleep", "echo", "stdin", "question", "retry",
            "expBackoff", "spinner", "glob", "which", "tmpdir", "tmpfile", "minimist", "argv",
            "dotenv", "chalk", "ps", "kill", "fs", "os", "path", "quote", "quotePowerShell",
            "useBash", "usePowerShell", "usePwsh", "versions", "ProcessOutput", "ProcessError",
            "ProcessPromise",
        ],
    )?;
    super::define_name(driver, "xmas:shell", exports)?;

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
