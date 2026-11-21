//! fs: asynchronous file operations, shaped after Node's fs.promises.
//!
//! Every operation is: create a promise, park the settlement closure, hand the work off to smol::spawn, return the promise.
//! The work runs on smol's thread pool (smol::fs internally just uses unblock); the result goes through host::pending's
//! channel back to the thread that owns the realm and settles there, so the JS side sees a normal promise.
//!
//! All byte input/output goes through ArrayBuffer (modules::bytes), no longer detouring through base64:
//! An 8MB file encoded as base64 takes up three times as much on the heap.

use std::io::Write;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use futures_lite::{AsyncReadExt, AsyncWriteExt, StreamExt};
use quickjs_oxide::engine::api::core::{
    Context, Js, JsString, NativeErrorKind, ObjectRef, PromiseCapability, RuntimeError, Scope,
    Value,
};

use crate::driver::Driver;
use crate::host::Payload;
use crate::host::pending::{Job, Pending, Reply};
use crate::modules::bytes;
use crate::modules::quote;
use crate::modules::throw;

const PRELUDE: &str = include_str!("../../js/fs.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    use quickjs_oxide::engine::api::core::Js;

    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let pending = driver.host.pending.clone();

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_read_file",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    start(scope, &pending, async move {
                        smol::fs::read(path)
                            .await
                            .map(Payload::Bytes)
                            .map_err(|error| describe(&error))
                    })
                },
            )
            .map_err(|error| format!("__fs_read_file: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_write_file",
                4,
                move |scope: &mut Scope<'_>,
                      path: String,
                      data: Value,
                      append: bool,
                      _encoding: String| {
                    let bytes = bytes::take(scope, &data)?;
                    start(scope, &pending, async move {
                        if append {
                            let mut file = smol::fs::OpenOptions::new()
                                .create(true)
                                .write(true)
                                .append(true)
                                .open(path)
                                .await
                                .map_err(|error| describe(&error))?;
                            file.write_all(&bytes)
                                .await
                                .map_err(|error| describe(&error))?;
                            file.flush().await.map_err(|error| describe(&error))?;
                        } else {
                            smol::fs::write(path, bytes)
                                .await
                                .map_err(|error| describe(&error))?;
                        }
                        Ok(Payload::Bytes(Vec::new()))
                    })
                },
            )
            .map_err(|error| format!("__fs_write_file: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_mkdir",
                2,
                move |scope: &mut Scope<'_>, path: String, recursive: bool| {
                    start(scope, &pending, async move {
                        let outcome = if recursive {
                            smol::fs::create_dir_all(path).await
                        } else {
                            smol::fs::create_dir(path).await
                        };
                        outcome
                            .map(|()| Payload::Bytes(Vec::new()))
                            .map_err(|error| describe(&error))
                    })
                },
            )
            .map_err(|error| format!("__fs_mkdir: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_rm",
                3,
                move |scope: &mut Scope<'_>, path: String, recursive: bool, force: bool| {
                    start(scope, &pending, async move {
                        let target = PathBuf::from(&path);
                        match smol::fs::metadata(&target).await {
                            Err(_) if force => Ok(Payload::Bytes(Vec::new())),
                            Err(error) => Err(describe(&error)),
                            Ok(meta) if meta.is_dir() => {
                                if !recursive {
                                    return Err("EISDIR: target is a directory".to_string());
                                }
                                smol::fs::remove_dir_all(&target)
                                    .await
                                    .map(|()| Payload::Bytes(Vec::new()))
                                    .map_err(|error| describe(&error))
                            }
                            Ok(_) => smol::fs::remove_file(&target)
                                .await
                                .map(|()| Payload::Bytes(Vec::new()))
                                .map_err(|error| describe(&error)),
                        }
                    })
                },
            )
            .map_err(|error| format!("__fs_rm: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_rename",
                2,
                move |scope: &mut Scope<'_>, from: String, to: String| {
                    start(scope, &pending, async move {
                        smol::fs::rename(from, to)
                            .await
                            .map(|()| Payload::Bytes(Vec::new()))
                            .map_err(|error| describe(&error))
                    })
                },
            )
            .map_err(|error| format!("__fs_rename: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_copy",
                2,
                move |scope: &mut Scope<'_>, from: String, to: String| {
                    start(scope, &pending, async move {
                        let mut source = smol::fs::File::open(from)
                            .await
                            .map_err(|error| describe(&error))?;
                        let mut bytes = Vec::new();
                        source
                            .read_to_end(&mut bytes)
                            .await
                            .map_err(|error| describe(&error))?;
                        smol::fs::write(to, bytes)
                            .await
                            .map_err(|error| describe(&error))?;
                        Ok(Payload::Bytes(Vec::new()))
                    })
                },
            )
            .map_err(|error| format!("__fs_copy: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_realpath",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    start(scope, &pending, async move {
                        let real = smol::fs::canonicalize(path)
                            .await
                            .map_err(|error| describe(&error))?;
                        Ok(Payload::Text(real.to_string_lossy().into_owned()))
                    })
                },
            )
            .map_err(|error| format!("__fs_realpath: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_readdir",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    start(scope, &pending, async move {
                        let mut dir = smol::fs::read_dir(path)
                            .await
                            .map_err(|error| describe(&error))?;
                        let mut out = String::from("[");
                        let mut first = true;
                        while let Some(entry) = dir.next().await {
                            let entry = entry.map_err(|error| describe(&error))?;
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let kind = entry.file_type().await.ok();
                            let is_dir = kind.map(|kind| kind.is_dir()).unwrap_or(false);
                            let is_link = kind.map(|kind| kind.is_symlink()).unwrap_or(false);
                            if !first {
                                out.push(',');
                            }
                            first = false;
                            out.push_str(&format!(
                                "{{\"name\":{},\"dir\":{},\"link\":{}}}",
                                quote(&name),
                                is_dir,
                                is_link
                            ));
                        }
                        out.push(']');
                        Ok(Payload::Text(out))
                    })
                },
            )
            .map_err(|error| format!("__fs_readdir: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_stat",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    start(scope, &pending, async move {
                        let meta = smol::fs::metadata(path)
                            .await
                            .map_err(|error| describe(&error))?;
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                            .map(|since| since.as_secs_f64() * 1000.0)
                            .unwrap_or(0.0);
                        Ok(Payload::Text(format!(
                            "{{\"size\":{},\"dir\":{},\"link\":{},\"mtimeMs\":{}}}",
                            meta.len(),
                            meta.is_dir(),
                            meta.file_type().is_symlink(),
                            mtime
                        )))
                    })
                },
            )
            .map_err(|error| format!("__fs_stat: {error}"))?;
    }


    // ---------- the synchronous half ----------
    //
    // Node's *Sync calls, and what zx's helpers are built on (glob.sync, tmpfile). They block the
    // realm thread, which is the whole point of them: a script that wants a file now should not have
    // to become a state machine to get one.
    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_readdir_sync",
                1,
                move |scope: &mut Scope<'_>, path: String| -> Result<Value, RuntimeError> {
                    let entries = match std::fs::read_dir(&path) {
                        Ok(entries) => entries,
                        Err(error) => return Err(throw(scope, &describe(&error))),
                    };
                    let mut out = String::from("[");
                    let mut first = true;
                    for entry in entries {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(error) => return Err(throw(scope, &describe(&error))),
                        };
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let kind = entry.file_type().ok();
                        let is_dir = kind.map(|kind| kind.is_dir()).unwrap_or(false);
                        let is_link = kind.map(|kind| kind.is_symlink()).unwrap_or(false);
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        out.push_str(&format!(
                            "{{\"name\":{},\"dir\":{},\"link\":{}}}",
                            quote(&name),
                            is_dir,
                            is_link
                        ));
                    }
                    out.push(']');
                    Ok(text_value(&out))
                },
            )
            .map_err(|error| format!("__fs_readdir_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_stat_sync",
                1,
                move |scope: &mut Scope<'_>, path: String| -> Result<Value, RuntimeError> {
                    let meta = match std::fs::symlink_metadata(&path) {
                        Ok(meta) => meta,
                        Err(error) => return Err(throw(scope, &describe(&error))),
                    };
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|since| since.as_secs_f64() * 1000.0)
                        .unwrap_or(0.0);
                    Ok(text_value(&format!(
                        "{{\"size\":{},\"dir\":{},\"link\":{},\"mtimeMs\":{}}}",
                        meta.len(),
                        meta.is_dir(),
                        meta.file_type().is_symlink(),
                        mtime
                    )))
                },
            )
            .map_err(|error| format!("__fs_stat_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_read_file_sync",
                1,
                move |scope: &mut Scope<'_>, path: String| -> Result<Value, RuntimeError> {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => Ok(text_value(&text)),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_read_file_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_mkdir_sync",
                2,
                move |scope: &mut Scope<'_>, path: String, recursive: bool| -> Result<Value, RuntimeError> {
                    let outcome = if recursive {
                        std::fs::create_dir_all(&path)
                    } else {
                        std::fs::create_dir(&path)
                    };
                    match outcome {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_mkdir_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_write_file_sync",
                3,
                move |scope: &mut Scope<'_>, path: String, data: Value, append: bool| -> Result<Value, RuntimeError> {
                    let bytes = bytes::take(scope, &data)?;
                    let outcome = if append {
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&path)
                            .and_then(|mut file| file.write_all(&bytes))
                    } else {
                        std::fs::write(&path, &bytes)
                    };
                    match outcome {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_write_file_sync: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("fs prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__fs_module")
        .map_err(|error| format!("fs module: {error}"))?;
    let promises = driver
        .eval("globalThis.__fs_promises")
        .map_err(|error| format!("fs.promises: {error}"))?;
    let mut names = vec!["promises", "default"];
    names.extend([
        "readFile",
        "writeFile",
        "appendFile",
        "mkdir",
        "rm",
        "unlink",
        "rmdir",
        "rename",
        "copyFile",
        "realpath",
        "readdir",
        "stat",
        "exists",
    ]);
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "fs", exports)?;
    let promise_exports = super::collect(driver, &promises, &names[2..])?;
    super::define_with_alias(driver, "fs/promises", promise_exports)
}

/// Create a promise, park the settlement, throw the work out, and return this promise.
fn start<F>(scope: &mut Scope<'_>, pending: &Pending, job: F) -> Result<Value, RuntimeError>
where
    F: Future<Output = Reply> + Send + 'static,
{
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let parked = park(pending, capability);
    smol::spawn(async move { parked.finish(job.await) }).detach();
    Ok(Value::Object(promise))
}

fn park(pending: &Pending, capability: PromiseCapability) -> Job {
    pending.park(Box::new(move |context: &mut Context, reply: Reply| {
        settle(context, capability, reply);
    }))
}

fn settle(context: &mut Context, capability: PromiseCapability, reply: Reply) {
    match reply {
        Ok(payload) => {
            let value = match payload {
                Payload::Bytes(bytes) => bytes::give(context, &bytes).unwrap_or(Value::Undefined),
                Payload::Text(text) => match JsString::try_from_utf8(&text) {
                    Ok(text) => Value::String(text),
                    Err(_) => Value::Undefined,
                },
            };
            let _ = capability.resolve(context, value);
        }
        Err(message) => {
            let (code, text) = split_code(&message);
            let value = match context.new_native_error(NativeErrorKind::Internal, &text) {
                Ok(Value::Object(object)) => label(context, object, code),
                Ok(other) => other,
                Err(_) => Value::Undefined,
            };
            let _ = capability.reject(context, value);
        }
    }
}

/// Add name and code to the error object, sticking as close as possible to Node's fs errors.
fn label(context: &mut Context, error: ObjectRef, code: &str) -> Value {
    use quickjs_oxide::engine::api::core::{JsString, PropertyKey};
    /// Attach a field to the error object; if the name or value can't be converted to a JS string, skip it.
    fn put(context: &mut Context, object: &ObjectRef, key: &str, value: Value) {
        if let Ok(name) = JsString::try_from_utf8(key) {
            let _ = context.set_property(object, &PropertyKey::from_js_string(name), value);
        }
    }
    put(
        context,
        &error,
        "name",
        Value::String(JsString::from_static("Error")),
    );
    if let Ok(code) = JsString::try_from_utf8(code) {
        put(context, &error, "code", Value::String(code));
    }
    Value::Object(error)
}

fn split_code(message: &str) -> (&str, String) {
    match message.split_once(": ") {
        Some((code, text)) if code.len() <= 16 && code.chars().all(|c| c.is_ascii_uppercase()) => {
            (code, text.to_string())
        }
        _ => ("EIO", message.to_string()),
    }
}

/// A host function's answer as a JS string: a report, or the JSON the prelude reads.
fn text_value(value: &str) -> Value {
    match JsString::try_from_utf8(value) {
        Ok(value) => Value::String(value),
        Err(_) => Value::Undefined,
    }
}

fn describe(error: &std::io::Error) -> String {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        std::io::ErrorKind::UnexpectedEof => "EOF",
        _ => "EIO",
    };
    format!("{code}: {error}")
}
