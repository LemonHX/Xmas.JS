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
                        Ok(Payload::Text(stat_json(&meta)))
                    })
                },
            )
            .map_err(|error| format!("__fs_stat: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__fs_lstat",
                1,
                move |scope: &mut Scope<'_>, path: String| {
                    start(scope, &pending, async move {
                        let meta = smol::fs::symlink_metadata(path)
                            .await
                            .map_err(|error| describe(&error))?;
                        Ok(Payload::Text(stat_json(&meta)))
                    })
                },
            )
            .map_err(|error| format!("__fs_lstat: {error}"))?;
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
                2,
                move |scope: &mut Scope<'_>, path: String, follow: bool| -> Result<Value, RuntimeError> {
                    let meta = if follow { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) };
                    match meta {
                        Ok(meta) => Ok(text_value(&stat_json(&meta))),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_stat_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_fstat_sync",
                1,
                move |scope: &mut Scope<'_>, fd: f64| -> Result<Value, RuntimeError> {
                    match with_fd(fd as i32, |file| file.metadata()) {
                        Ok(meta) => Ok(text_value(&stat_json(&meta))),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_fstat_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_realpath_sync",
                1,
                move |scope: &mut Scope<'_>, path: String| -> Result<Value, RuntimeError> {
                    match std::fs::canonicalize(&path) {
                        Ok(real) => Ok(text_value(&real.to_string_lossy())),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_realpath_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_open_sync",
                3,
                move |scope: &mut Scope<'_>, path: String, flags: String, mode: f64| -> Result<Value, RuntimeError> {
                    let bits = match flag_bits(&flags) { Ok(bits) => bits, Err(error) => return Err(throw(scope, &describe(&error))) };
                    match open_fd(&path, bits, mode as u32) {
                        Ok(fd) => Ok(text_value(&fd.to_string())),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_open_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_close_sync",
                1,
                move |scope: &mut Scope<'_>, fd: f64| -> Result<Value, RuntimeError> {
                    match close_fd(fd as i32) {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_close_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_read_sync",
                3,
                move |scope: &mut Scope<'_>, fd: f64, length: f64, position: f64| -> Result<Value, RuntimeError> {
                    match read_fd(fd as i32, length as usize, position) {
                        Ok(bytes) => bytes::give(scope, &bytes),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_read_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_write_fd_sync",
                3,
                move |scope: &mut Scope<'_>, fd: f64, data: Value, position: f64| -> Result<Value, RuntimeError> {
                    let bytes = bytes::take(scope, &data)?;
                    match write_fd(fd as i32, &bytes, position) {
                        Ok(count) => Ok(text_value(&count.to_string())),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_write_fd_sync: {error}"))?;
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

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_chmod_sync",
                2,
                move |scope: &mut Scope<'_>, path: String, mode: String| -> Result<Value, RuntimeError> {
                    let bits = u32::from_str_radix(&mode, 8).map_err(|_| throw(scope, "invalid mode"))?;
                    #[cfg(unix)]
                    let outcome = {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(bits))
                    };
                    #[cfg(not(unix))]
                    let outcome = std::fs::metadata(&path).map(|_| ());
                    match outcome {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_chmod_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_rm_sync",
                2,
                move |scope: &mut Scope<'_>, path: String, flags: String| -> Result<Value, RuntimeError> {
                    let outcome: std::io::Result<Value> = (|| { let meta = std::fs::symlink_metadata(&path);
                        match meta {
                            Ok(m) if m.is_dir() => { if flags.contains('r') { std::fs::remove_dir_all(&path)? } else { std::fs::remove_dir(&path)? } }
                            Ok(_) => std::fs::remove_file(&path)?,
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound && flags.contains('f') => {}
                            Err(e) => return Err(e),
                        }
                        Ok(Value::Undefined) })();
                    match outcome {
                        Ok(value) => Ok(value),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_rm_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_rename_sync",
                2,
                move |scope: &mut Scope<'_>, from: String, to: String| -> Result<Value, RuntimeError> {
                    let outcome: std::io::Result<Value> = (|| { std::fs::rename(&from, &to)?; Ok(Value::Undefined) })();
                    match outcome {
                        Ok(value) => Ok(value),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_rename_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_link_sync",
                2,
                move |scope: &mut Scope<'_>, from: String, to: String| -> Result<Value, RuntimeError> {
                    let outcome: std::io::Result<Value> = (|| { std::fs::hard_link(&from, &to)?; Ok(Value::Undefined) })();
                    match outcome {
                        Ok(value) => Ok(value),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_link_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_symlink_sync",
                2,
                move |scope: &mut Scope<'_>, target: String, path: String| -> Result<Value, RuntimeError> {
                    #[cfg(unix)]
                    let outcome = std::os::unix::fs::symlink(&target, &path);
                    #[cfg(windows)]
                    let outcome = if std::path::Path::new(&target).is_dir() {
                        std::os::windows::fs::symlink_dir(&target, &path)
                    } else {
                        std::os::windows::fs::symlink_file(&target, &path)
                    };
                    match outcome {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_symlink_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__fs_readlink_sync",
                1,
                move |scope: &mut Scope<'_>, path: String| -> Result<Value, RuntimeError> {
                    match std::fs::read_link(&path) {
                        Ok(target) => Ok(text_value(&target.to_string_lossy())),
                        Err(error) => Err(throw(scope, &describe(&error))),
                    }
                },
            )
            .map_err(|error| format!("__fs_readlink_sync: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(&global, "__fs_constants", 0, |_: &mut Scope<'_>| -> Result<Value, RuntimeError> { Ok(text_value(&constants_json())) })
            .map_err(|error| format!("__fs_constants: {error}"))?;
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
        "mkdtemp",
        "chmod",
        "access",
        "lstat",
        "open",
        "constants",
    ]);
    let promise_names = names[2..].to_vec();
    // The synchronous family, which node's `fs` has next to the callback and promise ones.
    names.extend([
        "readdirSync",
        "statSync",
        "existsSync",
        "mkdirSync",
        "writeFileSync",
        "appendFileSync",
        "readFileSync",
        "chmodSync",
        "openSync",
        "writeSync",
        "symlinkSync",
        "readlinkSync",
        "closeSync",
        "readSync",
        "fstatSync",
        "lstatSync",
        "mkdtempSync",
        "realpathSync",
        "renameSync",
        "rmSync",
        "linkSync",
        "constants",
        "Stats",
        "Dirent",
        "createWriteStream",
        "createReadStream",
    ]);
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "fs", exports)?;
    let mut promise_exports = super::collect(driver, &promises, &promise_names)?;
    promise_exports.push(("default".to_string(), promises.clone()));
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

/// Everything Node's Stats carries, as the JSON the prelude turns into a Stats object.
fn stat_json(meta: &std::fs::Metadata) -> String {
    let ms = |time: std::io::Result<std::time::SystemTime>| -> f64 {
        time.ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    };
    #[cfg(unix)]
    let (dev, ino, mode, nlink, uid, gid, rdev, blksize, blocks, atime, ctime) = {
        use std::os::unix::fs::MetadataExt;
        (
            meta.dev(),
            meta.ino(),
            meta.mode(),
            meta.nlink(),
            meta.uid(),
            meta.gid(),
            meta.rdev(),
            meta.blksize(),
            meta.blocks(),
            meta.atime() as f64 * 1000.0 + (meta.atime_nsec() as f64) / 1e6,
            meta.ctime() as f64 * 1000.0 + (meta.ctime_nsec() as f64) / 1e6,
        )
    };
    #[cfg(not(unix))]
    let (dev, ino, mode, nlink, uid, gid, rdev, blksize, blocks, atime, ctime) = (
        0u64,
        0u64,
        if meta.is_dir() { 0o040000u32 } else { 0o100000u32 },
        1u64,
        0u32,
        0u32,
        0u64,
        4096u64,
        0u64,
        ms(meta.accessed()),
        ms(meta.modified()),
    );
    format!(
        "{{\"dev\":{dev},\"ino\":{ino},\"mode\":{mode},\"nlink\":{nlink},\"uid\":{uid},\"gid\":{gid},\"rdev\":{rdev},\"size\":{},\"blksize\":{blksize},\"blocks\":{blocks},\"atimeMs\":{atime},\"mtimeMs\":{},\"ctimeMs\":{ctime},\"birthtimeMs\":{}}}",
        meta.len(),
        ms(meta.modified()),
        ms(meta.created()),
    )
}

#[cfg(unix)]
fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

#[cfg(unix)]
fn open_fd(path: &str, flags: i32, mode: u32) -> std::io::Result<i32> {
    let c_path = std::ffi::CString::new(path)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), flags | libc::O_CLOEXEC, mode as libc::c_uint) };
    if fd < 0 { Err(last_error()) } else { Ok(fd) }
}

#[cfg(unix)]
fn close_fd(fd: i32) -> std::io::Result<()> {
    if unsafe { libc::close(fd) } < 0 { Err(last_error()) } else { Ok(()) }
}

#[cfg(unix)]
fn with_fd<T>(fd: i32, job: impl FnOnce(&std::fs::File) -> std::io::Result<T>) -> std::io::Result<T> {
    use std::os::unix::io::FromRawFd;
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    job(&file)
}

#[cfg(unix)]
fn read_fd(fd: i32, length: usize, position: f64) -> std::io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; length];
    let count = unsafe {
        if position >= 0.0 {
            libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, length, position as libc::off_t)
        } else {
            libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, length)
        }
    };
    if count < 0 {
        return Err(last_error());
    }
    buffer.truncate(count as usize);
    Ok(buffer)
}

#[cfg(unix)]
fn write_fd(fd: i32, data: &[u8], position: f64) -> std::io::Result<usize> {
    let count = unsafe {
        if position >= 0.0 {
            libc::pwrite(fd, data.as_ptr() as *const libc::c_void, data.len(), position as libc::off_t)
        } else {
            libc::write(fd, data.as_ptr() as *const libc::c_void, data.len())
        }
    };
    if count < 0 { Err(last_error()) } else { Ok(count as usize) }
}

#[cfg(not(unix))]
fn unsupported<T>() -> std::io::Result<T> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "file descriptors are not supported on this platform"))
}
#[cfg(not(unix))]
fn open_fd(_: &str, _: i32, _: u32) -> std::io::Result<i32> { unsupported() }
#[cfg(not(unix))]
fn close_fd(_: i32) -> std::io::Result<()> { unsupported() }
#[cfg(not(unix))]
fn with_fd<T>(_: i32, _: impl FnOnce(&std::fs::File) -> std::io::Result<T>) -> std::io::Result<T> { unsupported() }
#[cfg(not(unix))]
fn read_fd(_: i32, _: usize, _: f64) -> std::io::Result<Vec<u8>> { unsupported() }
#[cfg(not(unix))]
fn write_fd(_: i32, _: &[u8], _: f64) -> std::io::Result<usize> { unsupported() }

/// Node's flag strings ('r', 'w+', 'ax', ...) or a number, as the platform's open(2) bits.
#[cfg(unix)]
fn flag_bits(flags: &str) -> std::io::Result<i32> {
    if let Ok(number) = flags.parse::<i32>() {
        return Ok(number);
    }
    let bits = match flags {
        "r" | "rs" | "sr" => libc::O_RDONLY,
        "r+" | "rs+" | "sr+" => libc::O_RDWR,
        "w" => libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
        "wx" | "xw" => libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_EXCL,
        "w+" => libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
        "wx+" | "xw+" => libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC | libc::O_EXCL,
        "a" | "as" | "sa" => libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
        "ax" | "xa" => libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_EXCL,
        "a+" | "as+" | "sa+" => libc::O_RDWR | libc::O_CREAT | libc::O_APPEND,
        "ax+" | "xa+" => libc::O_RDWR | libc::O_CREAT | libc::O_APPEND | libc::O_EXCL,
        _ => return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
    };
    Ok(bits)
}
#[cfg(not(unix))]
fn flag_bits(_: &str) -> std::io::Result<i32> { unsupported() }

/// The platform's own values for fs.constants, as JSON.
#[cfg(unix)]
fn constants_json() -> String {
    format!(
        "{{\"F_OK\":{},\"R_OK\":{},\"W_OK\":{},\"X_OK\":{},\"O_RDONLY\":{},\"O_WRONLY\":{},\"O_RDWR\":{},\"O_CREAT\":{},\"O_EXCL\":{},\"O_TRUNC\":{},\"O_APPEND\":{},\"O_NOFOLLOW\":{},\"O_DIRECTORY\":{},\"S_IFMT\":{},\"S_IFREG\":{},\"S_IFDIR\":{},\"S_IFLNK\":{},\"COPYFILE_EXCL\":1}}",
        libc::F_OK, libc::R_OK, libc::W_OK, libc::X_OK, libc::O_RDONLY, libc::O_WRONLY, libc::O_RDWR,
        libc::O_CREAT, libc::O_EXCL, libc::O_TRUNC, libc::O_APPEND, libc::O_NOFOLLOW, libc::O_DIRECTORY,
        libc::S_IFMT, libc::S_IFREG, libc::S_IFDIR, libc::S_IFLNK
    )
}
#[cfg(not(unix))]
fn constants_json() -> String {
    "{\"F_OK\":0,\"R_OK\":4,\"W_OK\":2,\"X_OK\":1,\"S_IFMT\":61440,\"S_IFREG\":32768,\"S_IFDIR\":16384,\"S_IFLNK\":40960,\"COPYFILE_EXCL\":1}".to_string()
}
