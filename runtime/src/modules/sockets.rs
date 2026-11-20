//! sockets: connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
//! spells it.
//!
//! There is no socket object on the JS side: a socket is an id, a ReadableStream and a
//! WritableStream built in js/sockets.js. A read happens because a reader asked for a chunk, a
//! write happens because a writer wrote one, so nothing here runs a loop per socket and nothing
//! buffers bytes the caller did not ask for.
//!
//! Name resolution is smol's, on a blocking thread, which is why there is no dns module under all
//! this. There is no TLS: secureTransport 'on' is refused rather than half done.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::Shutdown;
use std::rc::Rc;
use std::sync::Arc;

use futures_lite::{AsyncReadExt, AsyncWriteExt};
use quickjs_oxide::engine::api::core::{
    Context, JsString, NativeErrorKind, PromiseCapability, RuntimeError, Scope, Value,
};

use crate::driver::Driver;
use crate::host::Payload;
use crate::host::pending::Reply;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/sockets.js");

/// What a read resolves with when the other side has nothing more to say.
const EOF: &str = "eof";

type Stream = Arc<smol::Async<std::net::TcpStream>>;

/// The open sockets of this realm. An id comes from JS, the same way fetch ids do.
#[derive(Default)]
struct State {
    sockets: RefCell<BTreeMap<u64, Stream>>,
    /// Sockets still being connected. The stream travels here because the connect task runs off
    /// the realm thread and the map is not Send.
    opening: RefCell<BTreeMap<u64, smol::channel::Receiver<Stream>>>,
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let pending = driver.host.pending.clone();
    let state = Rc::new(State::default());

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sockets_connect",
                3,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      hostname: String,
                      port: f64|
                      -> Result<Value, RuntimeError> {
                    let key = id as u64;
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();

                    let (stream_tx, stream_rx) = smol::channel::unbounded::<Stream>();
                    state.opening.borrow_mut().insert(key, stream_rx.clone());

                    let state_of_settle = Rc::clone(&state);
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            let opening = state_of_settle.opening.borrow_mut().remove(&key);
                            if reply.is_ok() {
                                if let Some(opening) = opening {
                                    // The stream was sent before the job finished, so it is here.
                                    if let Ok(stream) = opening.try_recv() {
                                        state_of_settle.sockets.borrow_mut().insert(key, stream);
                                    }
                                }
                            }
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        match open(&hostname, port as u16).await {
                            Ok((stream, remote, local)) => {
                                let _ = stream_tx.send(stream).await;
                                parked.finish(Ok(Payload::Text(format!("{remote}\0{local}"))));
                            }
                            Err(message) => parked.finish(Err(message)),
                        }
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__sockets_connect: {error}"))?;
    }

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sockets_read",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let Some(stream) = state.sockets.borrow().get(&(id as u64)).cloned() else {
                        return Err(bytes::error(scope, "the socket is closed"));
                    };
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        let mut buffer = vec![0u8; 16 * 1024];
                        let mut source = &*stream;
                        let reply = match source.read(&mut buffer).await {
                            Ok(0) => Ok(Payload::Text(EOF.to_string())),
                            Ok(read) => {
                                buffer.truncate(read);
                                Ok(Payload::Bytes(buffer))
                            }
                            Err(error) => Err(error.to_string()),
                        };
                        parked.finish(reply);
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__sockets_read: {error}"))?;
    }

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sockets_write",
                2,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      chunk: Value|
                      -> Result<Value, RuntimeError> {
                    let Some(stream) = state.sockets.borrow().get(&(id as u64)).cloned() else {
                        return Err(bytes::error(scope, "the socket is closed"));
                    };
                    // Copied here, on the realm thread, because the engine is not reachable from
                    // the task that does the writing.
                    let data = bytes::take(scope, &chunk)?;
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        let mut sink = &*stream;
                        let reply = match sink.write_all(&data).await {
                            Ok(()) => Ok(Payload::Text(String::new())),
                            Err(error) => Err(error.to_string()),
                        };
                        parked.finish(reply);
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__sockets_write: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sockets_end",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    if let Some(stream) = state.sockets.borrow().get(&(id as u64)).cloned() {
                        // A half close: the other side sees the end of the body while anything it
                        // sends is still readable here.
                        let _ = stream.get_ref().shutdown(Shutdown::Write);
                    }
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sockets_end: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sockets_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    if let Some(stream) = state.sockets.borrow_mut().remove(&(id as u64)) {
                        let _ = stream.get_ref().shutdown(Shutdown::Both);
                    }
                    state.opening.borrow_mut().remove(&(id as u64));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sockets_close: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("sockets prelude: {error}"))?;
    let module = driver
        .context()
        .eval("__sockets_module")
        .map_err(|error| format!("__sockets_module: {error}"))?;
    let exports = super::collect(driver, &module, &["connect", "default"])?;
    // Two names, no node: alias: this is a web API that Cloudflare spells cloudflare:sockets.
    super::define_name(driver, "sockets", exports.clone())?;
    super::define_name(driver, "cloudflare:sockets", exports)
}

/// Connect, and report both ends as text.
async fn open(hostname: &str, port: u16) -> Result<(Stream, String, String), String> {
    let addresses = smol::net::resolve((hostname, port))
        .await
        .map_err(|error| format!("{hostname}: {error}"))?;
    let mut last = format!("{hostname}: {port} did not resolve to anything");
    for address in addresses {
        match smol::Async::<std::net::TcpStream>::connect(address).await {
            Ok(stream) => {
                let remote = stream
                    .get_ref()
                    .peer_addr()
                    .map(|address| address.to_string())
                    .unwrap_or_default();
                let local = stream
                    .get_ref()
                    .local_addr()
                    .map(|address| address.to_string())
                    .unwrap_or_default();
                return Ok((Arc::new(stream), remote, local));
            }
            Err(error) => last = error.to_string(),
        }
    }
    Err(last)
}

/// Resolve or reject the promise JS is holding.
fn settle(context: &mut Context, capability: PromiseCapability, reply: Reply) {
    match reply {
        Ok(payload) => {
            let value = match payload {
                Payload::Bytes(bytes) => bytes::give(context, &bytes).unwrap_or(Value::Undefined),
                Payload::Text(text) if text == EOF => Value::String(JsString::from_static(EOF)),
                Payload::Text(text) => match JsString::try_from_utf8(&text) {
                    Ok(text) => Value::String(text),
                    Err(_) => Value::Undefined,
                },
            };
            let _ = capability.resolve(context, value);
        }
        Err(message) => {
            let value = match context.new_native_error(NativeErrorKind::Type, &message) {
                Ok(value) => value,
                Err(_) => Value::Undefined,
            };
            let _ = capability.reject(context, value);
        }
    }
}
