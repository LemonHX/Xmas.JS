//! WebSocket: the host functions the JS side reads and writes messages with.
//!
//! The frame codec and the socket loop are in host/websocket.rs; this file only moves messages
//! across the boundary. A message is one byte of opcode followed by the payload, so a text frame,
//! a binary frame and a close frame are told apart without a second call.

use std::rc::Rc;

use quickjs_oxide::engine::api::core::{
    Context, JsString, NativeErrorKind, RuntimeError, Scope, Value,
};

use crate::driver::Driver;
use crate::host::pending::{Job, Payload, Pending};
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/websocket.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    // The handshake: register a socket under the client's key and hand back the accept value.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__ws_new",
                1,
                move |_scope: &mut Scope<'_>, key: String| -> Result<Value, RuntimeError> {
                    let (id, accept) = state.open_ws(&key);
                    let text = format!("{id}\0{accept}");
                    Ok(Value::String(
                        JsString::try_from_utf8(&text)
                            .map_err(|_| RuntimeError::Invariant("the accept value is not text"))?,
                    ))
                },
            )
            .map_err(|error| format!("__ws_new: {error}"))?;
    }

    // One message for the realm, always as a promise: the caller writes __ws_read(id).then(...) and
    // has one shape to handle, whether the message is already queued or not.
    {
        let state = Rc::clone(&driver.server);
        let pending = driver.host.pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__ws_read",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let id = id as u64;
                    let Some(ws) = state.ws(id) else {
                        return answered(scope, &pending, None);
                    };
                    // A message already here still comes back as an answered promise: a read that
                    // sometimes answered with a value and sometimes with a promise left the caller
                    // calling .then on an ArrayBuffer.
                    if let Some(message) = ws.pull() {
                        return answered(scope, &pending, Some(message));
                    }
                    if ws.ended() {
                        return answered(scope, &pending, None);
                    }
                    if ws.waiting() {
                        return Err(RuntimeError::Invariant(
                            "a second read started before the first one answered",
                        ));
                    }
                    let (promise, job) = park_read(scope, &pending)?;
                    ws.park(job);
                    Ok(promise)
                },
            )
            .map_err(|error| format!("__ws_read: {error}"))?;
    }

    // Sending is not a promise in deno's socket, so the frame is queued and the loop writes it.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__ws_send",
                3,
                move |scope: &mut Scope<'_>, id: f64, data: Value, binary: bool| -> Result<Value, RuntimeError> {
                    let payload = bytes::take(scope, &data)?;
                    let Some(ws) = state.ws(id as u64) else {
                        return Ok(Value::Bool(false));
                    };
                    let mut message = Vec::with_capacity(payload.len() + 1);
                    message.push(if binary { 0x02 } else { 0x01 });
                    message.extend_from_slice(&payload);
                    Ok(Value::Bool(ws.send(message)))
                },
            )
            .map_err(|error| format!("__ws_send: {error}"))?;
    }

    // A close frame: the code and reason go out, and the loop stops after writing it.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__ws_close",
                3,
                move |_scope: &mut Scope<'_>,
                      id: f64,
                      code: f64,
                      reason: String|
                      -> Result<Value, RuntimeError> {
                    let Some(ws) = state.ws(id as u64) else {
                        return Ok(Value::Undefined);
                    };
                    let mut message = Vec::with_capacity(reason.len() + 3);
                    message.push(0x08);
                    message.extend_from_slice(&(code as u16).to_be_bytes());
                    message.extend_from_slice(reason.as_bytes());
                    ws.send(message);
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__ws_close: {error}"))?;
    }

    // The realm is done with this socket for good.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__ws_forget",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    state.forget_ws(id as u64);
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__ws_forget: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("websocket prelude: {error}"))?;

    let module = driver
        .context()
        .eval("globalThis.__ws_module")
        .map_err(|error| format!("xmas:websocket: {error}"))?;
    let exports = super::collect(driver, &module, &["WebSocket", "upgradeWebSocket"])?;
    super::define_name(driver, "xmas:websocket", exports)
}

/// A read answered before it returns: a message if there is one, the closed marker otherwise.
fn answered(
    scope: &mut Scope<'_>,
    pending: &Pending,
    message: Option<Vec<u8>>,
) -> Result<Value, RuntimeError> {
    let (promise, job) = park_read(scope, pending)?;
    job.finish(Ok(match message {
        Some(message) => Payload::Bytes(message),
        None => Payload::Text("closed".to_owned()),
    }));
    Ok(promise)
}

/// A promise that a message will answer, plus the ticket the frame loop finishes it with.
fn park_read(scope: &mut Scope<'_>, pending: &Pending) -> Result<(Value, Job), RuntimeError> {
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let job = pending.park(Box::new(move |context: &mut Context, reply| {
        let value = match reply {
            Ok(Payload::Bytes(bytes)) => bytes::give(context, &bytes).unwrap_or(Value::Undefined),
            Ok(Payload::Text(text)) => match JsString::try_from_utf8(&text) {
                Ok(text) => Value::String(text),
                Err(_) => Value::Undefined,
            },
            Err(message) => context
                .new_native_error(NativeErrorKind::Internal, &message)
                .unwrap_or(Value::Undefined),
        };
        let _ = capability.resolve(context, value);
    }));
    Ok((Value::Object(promise), job))
}
