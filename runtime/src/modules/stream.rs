//! stream: the native core holds the chunk queues, the byte buffers and the
//! backpressure accounting; JS only carries the API shape and the events.
//!
//! Why it is split this way: every chunk a stream carries would otherwise be a
//! Buffer living in the JS heap, plus a scan over the queue for every length
//! query and a splice + concat for every read(size). Chunks stay as Vec<u8>
//! here, readableLength() is a counter, and read(size) merges in Rust. One
//! ArrayBuffer is created per chunk that actually reaches JS, instead of one
//! Buffer per push and another per read.
//!
//! objectMode chunks are arbitrary JS values, so they are kept as Value handles
//! and handed back untouched; only byte streams take the ArrayBuffer path.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{Js, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/stream.js");

/// One queued chunk. Byte streams keep the bytes; object streams keep the value.
enum Chunk {
    Bytes(Vec<u8>),
    Value(Value),
}

impl Chunk {
    /// Bytes for a byte chunk, one for an object-mode value: the same counter
    /// then serves both readableLength() and the highWaterMark comparison.
    fn size(&self) -> usize {
        match self {
            Chunk::Bytes(bytes) => bytes.len(),
            Chunk::Value(_) => 1,
        }
    }
}

/// Both sides live on one record: a Duplex is one id, not two.
struct Stream {
    object_mode: bool,
    read: VecDeque<Chunk>,
    write: VecDeque<Chunk>,
    read_bytes: usize,
    write_bytes: usize,
    ended: bool,
}

impl Stream {
    fn new(object_mode: bool) -> Self {
        Stream {
            object_mode,
            read: VecDeque::new(),
            write: VecDeque::new(),
            read_bytes: 0,
            write_bytes: 0,
            ended: false,
        }
    }
}

#[derive(Default)]
struct State {
    next: u64,
    streams: HashMap<u64, Stream>,
}

type Shared = Rc<RefCell<State>>;

/// Turn a JS chunk into a queued chunk. Strings count as UTF-8, typed arrays and
/// ArrayBuffers are borrowed for the copy, object-mode values are kept as they are.
fn incoming(js: &mut impl Js, object_mode: bool, value: &Value) -> Result<Chunk, RuntimeError> {
    if object_mode {
        return Ok(Chunk::Value(value.clone()));
    }
    match value {
        Value::Undefined | Value::Null => Ok(Chunk::Bytes(Vec::new())),
        _ => bytes::with_bytes(js, value, |bytes| Chunk::Bytes(bytes.to_vec())),
    }
}

/// Hand a queued chunk back to JS. Byte chunks become an ArrayBuffer, which the
/// JS layer wraps in a Buffer without copying.
fn outgoing(js: &mut impl Js, chunk: Chunk) -> Result<Value, RuntimeError> {
    match chunk {
        Chunk::Bytes(bytes) => bytes::give(js, &bytes),
        Chunk::Value(value) => Ok(value),
    }
}

/// Pop the front of a queue, keeping the byte counter in step.
fn pop(queue: &mut VecDeque<Chunk>, counter: &mut usize) -> Option<Chunk> {
    let chunk = queue.pop_front()?;
    *counter -= chunk.size();
    Some(chunk)
}

fn push_back(stream: &mut Stream, chunk: Chunk) {
    stream.read_bytes += chunk.size();
    stream.read.push_back(chunk);
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let state: Shared = Rc::new(RefCell::new(State::default()));
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    // __stream_open(objectMode) -> id
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_open",
                1,
                move |_scope: &mut Scope<'_>, object_mode: bool| {
                    let mut state = state.borrow_mut();
                    state.next += 1;
                    let id = state.next;
                    state.streams.insert(id, Stream::new(object_mode));
                    Ok::<Value, RuntimeError>(Value::Float(id as f64))
                },
            )
            .map_err(|error| format!("__stream_open: {error}"))?;
    }

    // __stream_close(id)
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    state.borrow_mut().streams.remove(&(id as u64));
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__stream_close: {error}"))?;
    }

    // __stream_push(id, chunk) -> buffered bytes after the push, -1 once ended
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_push",
                2,
                move |scope: &mut Scope<'_>, id: f64, value: Value| {
                    let mut state = state.borrow_mut();
                    let Some(stream) = state.streams.get_mut(&(id as u64)) else {
                        return Ok(Value::Float(-1.0));
                    };
                    if stream.ended {
                        return Ok(Value::Float(-1.0));
                    }
                    let object_mode = stream.object_mode;
                    let chunk = incoming(scope, object_mode, &value)?;
                    push_back(stream, chunk);
                    Ok(Value::Float(stream.read_bytes as f64))
                },
            )
            .map_err(|error| format!("__stream_push: {error}"))?;
    }

    // __stream_unshift(id, chunk)
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_unshift",
                2,
                move |scope: &mut Scope<'_>, id: f64, value: Value| {
                    let mut state = state.borrow_mut();
                    let Some(stream) = state.streams.get_mut(&(id as u64)) else {
                        return Ok(Value::Float(-1.0));
                    };
                    let object_mode = stream.object_mode;
                    let chunk = incoming(scope, object_mode, &value)?;
                    stream.read_bytes += chunk.size();
                    stream.read.push_front(chunk);
                    Ok(Value::Float(stream.read_bytes as f64))
                },
            )
            .map_err(|error| format!("__stream_unshift: {error}"))?;
    }

    // __stream_end(id): no more data on the readable side
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_end",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    if let Some(stream) = state.borrow_mut().streams.get_mut(&(id as u64)) {
                        stream.ended = true;
                    }
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__stream_end: {error}"))?;
    }

    // __stream_length(id) -> readable buffered bytes
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_length",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let state = state.borrow();
                    let length = state
                        .streams
                        .get(&(id as u64))
                        .map(|stream| stream.read_bytes)
                        .unwrap_or(0);
                    Ok::<Value, RuntimeError>(Value::Float(length as f64))
                },
            )
            .map_err(|error| format!("__stream_length: {error}"))?;
    }

    // __stream_read(id, size) -> ArrayBuffer / object-mode value / null
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_read",
                2,
                move |scope: &mut Scope<'_>, id: f64, size: f64| {
                    let mut state = state.borrow_mut();
                    let Some(stream) = state.streams.get_mut(&(id as u64)) else {
                        return Ok(Value::Null);
                    };
                    if stream.object_mode || size < 0.0 {
                        let Some(chunk) = pop(&mut stream.read, &mut stream.read_bytes) else {
                            return Ok(Value::Null);
                        };
                        return outgoing(scope, chunk);
                    }
                    let wanted = size as usize;
                    let mut merged: Vec<u8> = Vec::new();
                    while merged.len() < wanted {
                        let Some(chunk) = pop(&mut stream.read, &mut stream.read_bytes) else {
                            break;
                        };
                        match chunk {
                            Chunk::Bytes(bytes) => merged.extend_from_slice(&bytes),
                            Chunk::Value(value) => return outgoing(scope, Chunk::Value(value)),
                        }
                    }
                    if merged.is_empty() {
                        return Ok(Value::Null);
                    }
                    if merged.len() > wanted {
                        let rest = Chunk::Bytes(merged.split_off(wanted));
                        stream.read_bytes += rest.size();
                        stream.read.push_front(rest);
                    }
                    bytes::give(scope, &merged)
                },
            )
            .map_err(|error| format!("__stream_read: {error}"))?;
    }

    // __stream_write(id, chunk) -> pending bytes after queueing
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_write",
                2,
                move |scope: &mut Scope<'_>, id: f64, value: Value| {
                    let mut state = state.borrow_mut();
                    let Some(stream) = state.streams.get_mut(&(id as u64)) else {
                        return Ok(Value::Float(-1.0));
                    };
                    let object_mode = stream.object_mode;
                    let chunk = incoming(scope, object_mode, &value)?;
                    stream.write_bytes += chunk.size();
                    stream.write.push_back(chunk);
                    Ok(Value::Float(stream.write_bytes as f64))
                },
            )
            .map_err(|error| format!("__stream_write: {error}"))?;
    }

    // __stream_shift_write(id) -> the next chunk to hand to _write, or null
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_shift_write",
                1,
                move |scope: &mut Scope<'_>, id: f64| {
                    let mut state = state.borrow_mut();
                    let Some(stream) = state.streams.get_mut(&(id as u64)) else {
                        return Ok(Value::Null);
                    };
                    let Some(chunk) = pop(&mut stream.write, &mut stream.write_bytes) else {
                        return Ok(Value::Null);
                    };
                    outgoing(scope, chunk)
                },
            )
            .map_err(|error| format!("__stream_shift_write: {error}"))?;
    }

    // __stream_wpending(id) -> writable pending bytes
    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__stream_wpending",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let state = state.borrow();
                    let pending = state
                        .streams
                        .get(&(id as u64))
                        .map(|stream| stream.write_bytes)
                        .unwrap_or(0);
                    Ok::<Value, RuntimeError>(Value::Float(pending as f64))
                },
            )
            .map_err(|error| format!("__stream_wpending: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("stream prelude: {error}"))?;

    let module = driver
        .eval("globalThis.__stream_module")
        .map_err(|error| format!("stream module: {error}"))?;
    let names = [
        "Stream",
        "Readable",
        "Writable",
        "Duplex",
        "Transform",
        "PassThrough",
        "getDefaultHighWaterMark",
        "setDefaultHighWaterMark",
        "pipeline",
        "finished",
        "promises",
        "default",
    ];
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "stream", exports)?;

    let promises = driver
        .eval("globalThis.__stream_promises")
        .map_err(|error| format!("stream/promises: {error}"))?;
    let promise_exports = super::collect(driver, &promises, &["pipeline", "finished"])?;
    super::define_with_alias(driver, "stream/promises", promise_exports)
}
