//! HTTP server: the bridge between a connection and the realm.
//!
//! The sockets and the HTTP/1.1 parsing are trillium's, on this same thread (host/server.rs). This
//! file hands a request to the realm's JS dispatcher and turns the answer back into a Conn.
//!
//! Nothing on this path is Send. A request is a direct call into the realm, and a handler that
//! answers later calls __serve_respond, which is what the waiting connection wakes on.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use futures_lite::future;
use quickjs_oxide::engine::api::core::{
    CallableRef, Context, FromJs, IntoJs, RuntimeError, Scope, Value,
};
use trillium::Conn;
use trillium_http::Body;

use crate::driver::Driver;
use crate::host::pending::{Payload, Pending};
use crate::host::server::{self, Answer, AnswerBody, Server};
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/server.js");

/// How long a handler may take before its connection is answered with 504. Without this a handler
/// that never settles would hold the connection and its slot forever.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

/// One connection being answered.
pub type Answering = Pin<Box<dyn Future<Output = Conn>>>;

/// What the server calls for every request. Boxed so a realm that failed to boot and a realm that
/// booted are the same type to the accept loop.
pub type Handler = Box<dyn Fn(Conn) -> Answering>;

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_respond",
                5,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      status: f64,
                      status_text: String,
                      headers: String,
                      body: Value|
                      -> Result<Value, RuntimeError> {
                    let body = match body {
                        Value::Undefined | Value::Null => Vec::new(),
                        other => bytes::take(scope, &other)?,
                    };
                    let answer = Answer {
                        status: status as u16,
                        status_text,
                        headers,
                        body: AnswerBody::Bytes(body),
                    };
                    // False means the connection is already gone, which is the realm's cue to
                    // abort the request so a handler's timers stop.
                    let delivered = state.settle(id as u64, Ok(answer));
                    Ok(Value::Bool(delivered))
                },
            )
            .map_err(|error| format!("__serve_respond: {error}"))?;
    }

    // A streaming answer: the head is settled now, the body follows through __serve_chunk.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_stream",
                4,
                move |_scope: &mut Scope<'_>,
                      id: f64,
                      status: f64,
                      status_text: String,
                      headers: String|
                      -> Result<Value, RuntimeError> {
                    let id = id as u64;
                    let (_, reader) = state.open_stream(id);
                    let answer = Answer {
                        status: status as u16,
                        status_text,
                        headers,
                        body: AnswerBody::Stream(reader),
                    };
                    Ok(Value::Bool(state.settle(id, Ok(answer))))
                },
            )
            .map_err(|error| format!("__serve_stream: {error}"))?;
    }

    // One chunk of a streaming body. The promise resolves to true once the connection has taken
    // it (waiting for room if it has to), and to false once the connection is gone.
    {
        let state = Rc::clone(&driver.server);
        let pending = driver.host.pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__serve_chunk",
                2,
                move |scope: &mut Scope<'_>, id: f64, chunk: Value| -> Result<Value, RuntimeError> {
                    let id = id as u64;
                    let bytes = bytes::take(scope, &chunk)?;
                    let Some(sender) = state.stream(id) else {
                        return resolved(scope, false);
                    };
                    match sender.try_send(bytes) {
                        Ok(()) => resolved(scope, true),
                        Err(smol::channel::TrySendError::Full(bytes)) => {
                            // JS holds its next read until this resolves, so one send is in
                            // flight per answer and the chunks keep their order.
                            parked(scope, &pending, async move {
                                sender.send(bytes).await.is_ok()
                            })
                        }
                        Err(smol::channel::TrySendError::Closed(_)) => {
                            state.end_stream(id);
                            resolved(scope, false)
                        }
                    }
                },
            )
            .map_err(|error| format!("__serve_chunk: {error}"))?;
    }

    // A websocket handshake: the answer is a 101 whose body is not ours to write.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_upgrade",
                4,
                move |_scope: &mut Scope<'_>,
                      id: f64,
                      status: f64,
                      status_text: String,
                      headers: String|
                      -> Result<Value, RuntimeError> {
                    let answer = Answer {
                        status: status as u16,
                        status_text,
                        headers,
                        body: AnswerBody::Upgrade,
                    };
                    Ok(Value::Bool(state.settle(id as u64, Ok(answer))))
                },
            )
            .map_err(|error| format!("__serve_upgrade: {error}"))?;
    }

    // The body is complete.
    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_stop",
                0,
                move |_scope: &mut Scope<'_>| -> Result<Value, RuntimeError> {
                    state.stop_serving();
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__serve_stop: {error}"))?;
    }

    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_end",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    state.end_stream(id as u64);
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__serve_end: {error}"))?;
    }

    {
        let state = Rc::clone(&driver.server);
        driver
            .context()
            .define_function(
                &global,
                "__serve_fail",
                2,
                move |_scope: &mut Scope<'_>, id: f64, message: String| -> Result<Value, RuntimeError> {
                    eprintln!("request failed: {message}");
                    state.settle(id as u64, Err(message));
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__serve_fail: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("server prelude: {error}"))?;

    // Keep the dispatcher instead of looking it up by name for every request.
    let dispatch = driver
        .context()
        .eval("__serve_dispatch")
        .map_err(|error| format!("__serve_dispatch: {error}"))?;
    let dispatch = CallableRef::from_js(driver.context(), dispatch)
        .map_err(|error| format!("__serve_dispatch: {error}"))?;
    *driver.server.dispatch.borrow_mut() = Some(dispatch);

    let module = driver
        .context()
        .eval("__serve_module")
        .map_err(|error| format!("__serve_module: {error}"))?;
    let exports = super::collect(driver, &module, &["serve"])?;
    super::define_name(driver, "xmas:server", exports)
}

/// Serve on addr with one realm per worker.
///
/// boot runs on the worker thread: it builds the realm, installs the modules and evaluates the
/// program, which is where the program calls serve() from xmas:server and registers its handler. A
/// realm cannot be moved to a thread, so it has to be built there.
pub fn serve<B>(addr: &str, workers: usize, boot: B) -> Result<Server, String>
where
    B: Fn(usize) -> Result<Rc<RefCell<Driver>>, String> + Send + Sync + Clone + 'static,
{
    let count = workers.max(1);
    // Every worker says whether it got its realm. Without this a server whose workers all failed
    // would bind, answer 500 to everything and look healthy in a terminal.
    let (report, reports) = smol::channel::bounded::<Result<(), String>>(count);

    let server = server::serve(addr, count, move |index, executor, stop| {
        // A panic while booting must not leave the caller waiting for a report that never comes.
        let booted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| boot(index)))
            .unwrap_or_else(|_| Err(String::from("the realm panicked while it booted")));
        let realm = match booted {
            Ok(realm) => {
                realm.borrow().server.set_stop_signal(stop);
                let _ = report.send_blocking(Ok(()));
                realm
            }
            Err(error) => {
                eprintln!("worker {index} could not start its realm: {error}");
                let _ = report.send_blocking(Err(error.clone()));
                // Never reached: serve refuses to start unless every worker booted, and the server
                // is dropped before a connection could arrive. The signature still wants a handler.
                let handle: Handler = Box::new(move |conn: Conn| {
                    let text = format!("this worker could not start: {error}");
                    Box::pin(async move { server_error(conn, &text) })
                });
                return (handle, server::no_upgrade());
            }
        };

        // Keep the realm turning between requests: jobs, timers and host results.
        executor.spawn(Driver::pump(Rc::clone(&realm))).detach();

        let socket_realm = Rc::clone(&realm);
        let handle: Handler = Box::new(move |conn: Conn| {
            let realm = Rc::clone(&realm);
            Box::pin(answer(realm, conn))
        });

        // A connection that stopped being HTTP belongs to a socket the realm is holding. The
        // client's key is what ties this upgrade to that socket: it was registered when the
        // handler asked for the upgrade, before the answer went out.
        let upgraded: server::Upgraded = Box::new(move |upgrade: server::Upgrade<server::Transport>| {
            let key = upgrade
                .received_headers()
                .get("sec-websocket-key")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            let Some(key) = key else {
                eprintln!("an upgrade arrived with no sec-websocket-key");
                return;
            };
            let found = {
                let driver = socket_realm.borrow();
                Rc::clone(&driver.server).take_ws(&key)
            };
            let Some((ws, pipes)) = found else {
                eprintln!("an upgrade arrived that no handler asked for");
                return;
            };
            executor
                .spawn(crate::host::websocket::serve(ws, pipes, upgrade))
                .detach();
        });

        (handle, upgraded)
    })
    .map_err(|error| format!("serve {addr}: {error}"))?;

    let mut started = 0usize;
    let mut first: Option<String> = None;
    for _ in 0..count {
        match reports.recv_blocking() {
            Ok(Ok(())) => started += 1,
            Ok(Err(error)) => {
                if first.is_none() {
                    first = Some(error);
                }
            }
            // A worker that went away without a word is as bad as one that failed.
            Err(_) => {}
        }
    }

    if started < count {
        // Stop the threads that did come up: a server missing a realm is a failed start, not a
        // degraded one, and a start that fails says why.
        drop(server);
        let why = first.unwrap_or_else(|| String::from("a worker went away before it booted"));
        return Err(format!("{} of {count} workers could not start: {why}", count - started));
    }

    Ok(server)
}

/// One request, from the parsed head trillium handed over to the answer going back out.
async fn answer(realm: Rc<RefCell<Driver>>, mut conn: Conn) -> Conn {
    let method = conn.method().to_string();
    let target = conn.path_and_query().to_string();
    let headers = request_headers(&conn);
    let peer_ip = conn.peer_ip().map(|ip| ip.to_string()).unwrap_or_default();

    // A body is read into memory before the handler sees it. Streaming it would hold the realm's
    // thread inside the handler, and a request body that big is the rare case.
    let body = match conn.request_body().read_bytes().await {
        Ok(body) => body,
        Err(error) => {
            return conn
                .with_status(400)
                .with_body(format!("could not read the request body: {error}"));
        }
    };

    let (state, id, receiver) = {
        let driver = realm.borrow();
        let state = Rc::clone(&driver.server);
        let (id, receiver) = state.open();
        (state, id, receiver)
    };
    // A connection that goes away mid-request drops this future, and with it the slot: otherwise
    // every abandoned request would leave an entry behind for the life of the realm.
    let slot = Slot {
        state: Rc::clone(&state),
        id,
    };

    let thrown = {
        let mut driver = realm.borrow_mut();
        hand_over(&mut driver, &state, id, &method, &target, &headers, &body, &peer_ip)
    };
    if let Some(message) = thrown {
        state.close(id);
        return server_error(conn, &message);
    }

    // The handler answered without waiting: the answer is already in the slot, and this request
    // never leaves the thread. Otherwise the pump needs a slice to run the handler's microtasks.
    if let Ok(ready) = receiver.try_recv() {
        return settle(conn, ready);
    }
    realm.borrow().host.pending.notify();

    let outcome = future::or(
        async { Some(receiver.recv().await) },
        async {
            smol::Timer::after(ANSWER_WITHIN).await;
            None
        },
    )
    .await;

    drop(slot);
    match outcome {
        Some(Ok(ready)) => settle(conn, ready),
        None => conn.with_status(504).with_body(format!(
            "the handler did not answer within {}s",
            ANSWER_WITHIN.as_secs()
        )),
        Some(Err(error)) => server_error(conn, &format!("the answer was lost: {error}")),
    }
}

/// Holds a request's slot open for as long as the connection is waiting for it.
struct Slot {
    state: Rc<server::State>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        // Only the slot: a streaming answer is still writing its body under the same id.
        self.state.forget(self.id);
    }
}

/// Call the realm's dispatcher. An error here is the dispatcher itself failing (a bug in the JS
/// half), never the handler throwing: that arrives through __serve_fail.
fn hand_over(
    driver: &mut Driver,
    state: &Rc<server::State>,
    id: u64,
    method: &str,
    target: &str,
    headers: &str,
    body: &[u8],
    peer_ip: &str,
) -> Option<String> {
    let Some(dispatch) = state.dispatch.borrow().clone() else {
        return Some("the server module is not installed in this realm".to_string());
    };

    let outcome = {
        let context = driver.context();
        match arguments(context, id, method, target, headers, body, peer_ip) {
            Ok(args) => context.call(&dispatch, Value::Undefined, &args),
            Err(error) => Err(error),
        }
    };
    outcome.err().map(|error| driver.render(error))
}

/// The six things the dispatcher needs, in order.
fn arguments(
    context: &mut Context,
    id: u64,
    method: &str,
    target: &str,
    headers: &str,
    body: &[u8],
    peer_ip: &str,
) -> Result<Vec<Value>, RuntimeError> {
    Ok(vec![
        (id as f64).into_js(context)?,
        method.into_js(context)?,
        target.into_js(context)?,
        headers.into_js(context)?,
        bytes::give(context, body)?,
        peer_ip.into_js(context)?,
    ])
}

/// Put an answer onto the connection. The body carries its own length, so trillium frames it.
fn settle(conn: Conn, ready: Result<Answer, String>) -> Conn {
    let answer = match ready {
        Ok(answer) => answer,
        Err(message) => return server_error(conn, &message),
    };
    let mut conn = conn.with_status(answer.status);
    for (name, value) in pairs(&answer.headers) {
        conn = conn.with_response_header(name, value);
    }
    match answer.body {
        AnswerBody::Bytes(body) => conn.with_body(body),
        // No length: trillium frames it chunked and writes each chunk as the realm produces it.
        AnswerBody::Stream(reader) => conn.with_body(Body::new_streaming(reader, None)),
        // The 101 goes out, and what is left of the connection belongs to the WebSocket loop.
        AnswerBody::Upgrade => conn.upgrade(),
    }
}

/// A promise that is already settled, so the JS side has one shape to await.
fn resolved(scope: &mut Scope<'_>, answer: bool) -> Result<Value, RuntimeError> {
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let _ = capability.resolve(scope, Value::Bool(answer));
    Ok(Value::Object(promise))
}

/// A promise settled by work that finishes later, resolved to the boolean the work produced.
fn parked<F>(scope: &mut Scope<'_>, pending: &Pending, work: F) -> Result<Value, RuntimeError>
where
    F: Future<Output = bool> + Send + 'static,
{
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let job = pending.park(Box::new(move |context: &mut Context, reply| {
        let answer = match reply {
            Ok(Payload::Text(text)) => text == "yes",
            _ => false,
        };
        let _ = capability.resolve(context, Value::Bool(answer));
    }));
    smol::spawn(async move {
        let answer = if work.await { "yes" } else { "no" };
        job.finish(Ok(Payload::Text(answer.to_string())));
    })
    .detach();
    Ok(Value::Object(promise))
}

fn server_error(conn: Conn, message: &str) -> Conn {
    conn.with_status(500)
        .with_response_header("content-type", "text/plain")
        .with_body(message.to_string())
}

/// The request head as a NUL-joined name/value list, the same shape every other boundary uses.
fn request_headers(conn: &Conn) -> String {
    let mut out = String::new();
    for (name, value) in conn.request_headers().iter() {
        out.push_str(&name.to_string());
        out.push('\0');
        // A header repeated by the client arrives as several values; join them the way a single
        // header line would read, so the handler sees one string per name.
        out.push_str(value.as_str().unwrap_or_default());
        out.push('\0');
    }
    out
}

/// Split a NUL-joined header list back into pairs.
fn pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut parts = text.split('\0');
    while let (Some(name), Some(value)) = (parts.next(), parts.next()) {
        if !name.is_empty() {
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}
