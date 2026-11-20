//! sockets: connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
//! spells it.
//!
//! There is no socket object on the JS side: a socket is an id, a ReadableStream and a
//! WritableStream built in js/sockets.js. A read happens because a reader asked for a chunk, a
//! write happens because a writer wrote one, so nothing here runs a loop per socket and nothing
//! buffers bytes the caller did not ask for.
//!
//! A UDPSocket is the same arrangement one step down: no connection, so the ids point at a bound
//! datagram socket, every read is one datagram and every write is one datagram. Message edges are
//! what the caller asked for, so the queue keeps them: an empty datagram is still a datagram.
//!
//! Name resolution is smol's, on a blocking thread, which is why there is no dns module under all
//! this. There is no TLS: secureTransport 'on' is refused rather than half done.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::Shutdown;
use std::rc::Rc;
use std::sync::Arc;

use futures_lite::{AsyncReadExt, AsyncWriteExt, future};
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

/// The largest payload a UDP datagram can carry over IPv4.
const MAX_DATAGRAM: usize = 65_507;

/// A bound datagram socket, plus the end of the channel that a pending read watches for a close.
struct Datagram {
    socket: Arc<smol::net::UdpSocket>,
    stop: smol::channel::Sender<()>,
    stopped: smol::channel::Receiver<()>,
}

/// What a datagram read hands over: the socket and its close channel, both Send.
type Opened = (
    Arc<smol::net::UdpSocket>,
    smol::channel::Sender<()>,
    smol::channel::Receiver<()>,
);

/// The open sockets of this realm. An id comes from JS, the same way fetch ids do.
#[derive(Default)]
struct State {
    sockets: RefCell<BTreeMap<u64, Stream>>,
    /// Sockets still being connected. The stream travels here because the connect task runs off
    /// the realm thread and the map is not Send.
    opening: RefCell<BTreeMap<u64, smol::channel::Receiver<Stream>>>,
    datagrams: RefCell<BTreeMap<u64, Datagram>>,
    /// Datagram sockets still being bound, for the same reason.
    binding: RefCell<BTreeMap<u64, smol::channel::Receiver<Opened>>>,
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

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__udp_open",
                5,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      remote_host: String,
                      remote_port: f64,
                      local_host: String,
                      local_port: f64|
                      -> Result<Value, RuntimeError> {
                    let key = id as u64;
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();

                    let (opened_tx, opened_rx) = smol::channel::unbounded::<Opened>();
                    state.binding.borrow_mut().insert(key, opened_rx);

                    let state_of_settle = Rc::clone(&state);
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            let binding = state_of_settle.binding.borrow_mut().remove(&key);
                            if reply.is_ok() {
                                if let Some(binding) = binding {
                                    if let Ok((socket, stop, stopped)) = binding.try_recv() {
                                        state_of_settle.datagrams.borrow_mut().insert(
                                            key,
                                            Datagram {
                                                socket,
                                                stop,
                                                stopped,
                                            },
                                        );
                                    }
                                }
                            }
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        let bound = open_datagram(
                            &remote_host,
                            remote_port as u16,
                            &local_host,
                            local_port as u16,
                        )
                        .await;
                        match bound {
                            Ok((socket, stop, stopped, local)) => {
                                let _ = opened_tx.send((socket, stop, stopped)).await;
                                parked.finish(Ok(Payload::Text(local)));
                            }
                            Err(message) => parked.finish(Err(message)),
                        }
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__udp_open: {error}"))?;
    }

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__udp_send",
                2,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      chunk: Value|
                      -> Result<Value, RuntimeError> {
                    let Some(socket) = state
                        .datagrams
                        .borrow()
                        .get(&(id as u64))
                        .map(|datagram| Arc::clone(&datagram.socket))
                    else {
                        return Err(bytes::error(scope, "the socket is closed"));
                    };
                    let data = bytes::take(scope, &chunk)?;
                    if data.len() > MAX_DATAGRAM {
                        return Err(scope_range_error(
                            scope,
                            "a datagram carries at most 65507 bytes",
                        ));
                    }
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        let reply = match socket.send(&data).await {
                            Ok(_) => Ok(Payload::Text(String::new())),
                            Err(error) => Err(error.to_string()),
                        };
                        parked.finish(reply);
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__udp_send: {error}"))?;
    }

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__udp_recv",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let Some((socket, stopped)) =
                        state.datagrams.borrow().get(&(id as u64)).map(|datagram| {
                            (Arc::clone(&datagram.socket), datagram.stopped.clone())
                        })
                    else {
                        return Err(bytes::error(scope, "the socket is closed"));
                    };
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            settle(context, capability, reply);
                        }));

                    smol::spawn(async move {
                        // A datagram read has to be raceable against the socket being closed:
                        // nothing else will ever wake it, and a task parked forever keeps the
                        // socket alive.
                        let reply = match future::or(
                            async move {
                                let mut buffer = vec![0u8; MAX_DATAGRAM];
                                match socket.recv(&mut buffer).await {
                                    Ok(read) => {
                                        buffer.truncate(read);
                                        Ok(Some(buffer))
                                    }
                                    Err(error) => Err(error.to_string()),
                                }
                            },
                            async move {
                                let _ = stopped.recv().await;
                                Ok(None)
                            },
                        )
                        .await
                        {
                            // None is a socket that was closed under the read, and an empty
                            // datagram is not that: it comes back as zero bytes.
                            Ok(None) => Ok(Payload::Text(EOF.to_string())),
                            Ok(Some(bytes)) => Ok(Payload::Bytes(bytes)),
                            Err(message) => Err(message),
                        };
                        parked.finish(reply);
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__udp_recv: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__udp_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    if let Some(datagram) = state.datagrams.borrow_mut().remove(&(id as u64)) {
                        // Wake a read that is waiting: a datagram socket has no end of file, so
                        // this is the only way it hears about a close.
                        let _ = datagram.stop.try_send(());
                    }
                    state.binding.borrow_mut().remove(&(id as u64));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__udp_close: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("sockets prelude: {error}"))?;
    let module = driver
        .context()
        .eval("__sockets_module")
        .map_err(|error| format!("__sockets_module: {error}"))?;
    let exports = super::collect(driver, &module, &["connect", "UDPSocket", "default"])?;
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

/// Bind a datagram socket and point it at the remote end. Both ends report back as text, because
/// the JS side wants the local address it actually got.
async fn open_datagram(
    remote_host: &str,
    remote_port: u16,
    local_host: &str,
    local_port: u16,
) -> Result<
    (
        Arc<smol::net::UdpSocket>,
        smol::channel::Sender<()>,
        smol::channel::Receiver<()>,
        String,
    ),
    String,
> {
    let address = smol::net::resolve((remote_host, remote_port))
        .await
        .map_err(|error| format!("{remote_host}: {error}"))?
        .into_iter()
        .next()
        .ok_or_else(|| format!("{remote_host}: did not resolve to anything"))?;

    // A datagram socket is always bound: with no local preference that is the wildcard on the
    // remote's family, with one it is exactly what the caller asked for.
    let bind = if local_host.is_empty() {
        if address.is_ipv6() {
            "[::]:0".to_string()
        } else {
            "0.0.0.0:0".to_string()
        }
    } else if local_host.contains(':') {
        format!("[{local_host}]:{local_port}")
    } else {
        format!("{local_host}:{local_port}")
    };
    let socket = smol::net::UdpSocket::bind(bind.as_str())
        .await
        .map_err(|error| format!("bind {bind}: {error}"))?;
    // Connect on a datagram socket only fixes the peer: the kernel drops datagrams from anyone
    // else, which is what a socket with one remote end means.
    socket
        .connect(address)
        .await
        .map_err(|error| format!("{remote_host}:{remote_port}: {error}"))?;

    let local = socket
        .local_addr()
        .map(|address| format!("{}\u{0}{}", address.ip(), address.port()))
        .unwrap_or_default();
    let (stop, stopped) = smol::channel::unbounded();
    Ok((Arc::new(socket), stop, stopped, local))
}

/// The RangeError thrown when a value is out of range.
fn scope_range_error(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    match scope.new_native_error(NativeErrorKind::Range, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
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
