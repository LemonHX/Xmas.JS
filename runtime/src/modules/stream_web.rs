//! stream/web: the queue is native, the controllers, readers and promises are JS.
//!
//! llrt writes these 14,908 lines as rquickjs classes, with every WHATWG
//! algorithm running in Rust over rquickjs promises. That is not portable here:
//! our engine hands promises to JS and only lets a resolution happen from the
//! thread holding the realm, so a spec algorithm written in Rust would spend its
//! time hopping to JS to settle something. The split is the same one node:stream
//! uses: this file owns the chunk storage and the byte accounting, stream_web.js
//! owns the state machine, the promise plumbing and the API shape.
//!
//! A queue holds either bytes (a byte stream, where the counter is a byte count
//! and a partially consumed chunk keeps its offset) or arbitrary JS values (where
//! the counter is the chunk count). Nothing here understands backpressure: JS
//! asks for the size, compares it with a high water mark and decides.
//!
//! The BYOB path copies once more than a hand-written implementation would: the
//! bytes leave here as an ArrayBuffer, and JS copies them into the caller view.
//! Filling the view in place would need a mutable borrow of an ArrayBuffer from
//! the engine, which the byte bridge does not offer; the copy is a memcpy of at
//! most one view, and it keeps this file free of unsafe.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{Js, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/stream_web.js");

enum Item {
    Bytes { data: Vec<u8>, offset: usize },
    Value(Value),
}

impl Item {
    fn remaining(&self) -> usize {
        match self {
            Item::Bytes { data, offset } => data.len() - offset,
            Item::Value(_) => 1,
        }
    }
}

struct Queue {
    byte_queue: bool,
    items: VecDeque<Item>,
    /// Bytes queued for a byte queue, chunks queued for a value queue.
    size: usize,
    closed: bool,
}

impl Queue {
    fn new(byte_queue: bool) -> Self {
        Queue {
            byte_queue,
            items: VecDeque::new(),
            size: 0,
            closed: false,
        }
    }

    fn put(&mut self, item: Item) {
        self.size += item.remaining();
        self.items.push_back(item);
    }

    /// Take at most max bytes, or one whole chunk when max is not positive. A
    /// byte chunk that is larger than max keeps its remainder at the front, so a
    /// reader asking for 1 KiB at a time does not copy the tail back each round.
    fn take(&mut self, max: f64) -> Option<Item> {
        if !self.byte_queue || max <= 0.0 {
            let item = self.items.pop_front()?;
            self.size -= item.remaining();
            return Some(item);
        }
        let limit = if max.is_finite() {
            max as usize
        } else {
            usize::MAX
        };
        let mut out: Vec<u8> = Vec::new();
        while out.len() < limit {
            let room = limit - out.len();
            let Some(front) = self.items.front_mut() else {
                break;
            };
            let Item::Bytes { data, offset } = front else {
                break;
            };
            let left = data.len() - *offset;
            if left <= room {
                let Some(Item::Bytes { data, offset }) = self.items.pop_front() else {
                    break;
                };
                self.size -= data.len() - offset;
                out.extend_from_slice(&data[offset..]);
            } else {
                out.extend_from_slice(&data[*offset..*offset + room]);
                *offset += room;
                self.size -= room;
            }
            if self.items.front().is_some_and(|item| item.remaining() == 0) {
                self.items.pop_front();
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(Item::Bytes {
                data: out,
                offset: 0,
            })
        }
    }

    fn reset(&mut self) {
        self.items.clear();
        self.size = 0;
    }
}

#[derive(Default)]
struct State {
    next: u64,
    queues: HashMap<u64, Queue>,
}

type Shared = Rc<RefCell<State>>;

/// Ids arrive as f64 and are used as u64 keys; anything that cannot be one is a
/// bug in our own JS, so it gets an invariant rather than a user-facing error.
fn id_of(id: f64) -> Result<u64, RuntimeError> {
    if id.is_finite() && id > 0.0 {
        Ok(id as u64)
    } else {
        Err(RuntimeError::Invariant("stream/web: bad queue id"))
    }
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let state: Shared = Rc::new(RefCell::new(State::default()));
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_open",
                1,
                move |_scope: &mut Scope<'_>, byte_queue: bool| {
                    let mut state = state.borrow_mut();
                    state.next += 1;
                    let id = state.next;
                    state.queues.insert(id, Queue::new(byte_queue));
                    Ok::<Value, RuntimeError>(Value::Float(id as f64))
                },
            )
            .map_err(|error| format!("__sw_open: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_put",
                2,
                move |scope: &mut Scope<'_>, id: f64, chunk: Value| {
                    let id = id_of(id)?;
                    let byte_queue = {
                        let state = state.borrow();
                        let Some(queue) = state.queues.get(&id) else {
                            return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                        };
                        queue.byte_queue
                    };
                    let item = if byte_queue {
                        bytes::with_bytes(scope, &chunk, |raw| Item::Bytes {
                            data: raw.to_vec(),
                            offset: 0,
                        })?
                    } else {
                        Item::Value(chunk)
                    };
                    let mut state = state.borrow_mut();
                    let Some(queue) = state.queues.get_mut(&id) else {
                        return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                    };
                    queue.put(item);
                    Ok::<Value, RuntimeError>(Value::Float(queue.size as f64))
                },
            )
            .map_err(|error| format!("__sw_put: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_take",
                2,
                move |scope: &mut Scope<'_>, id: f64, max: f64| {
                    let id = id_of(id)?;
                    let taken = {
                        let mut state = state.borrow_mut();
                        let Some(queue) = state.queues.get_mut(&id) else {
                            return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                        };
                        queue.take(max)
                    };
                    match taken {
                        Some(Item::Bytes { data, offset }) => bytes::give(scope, &data[offset..]),
                        Some(Item::Value(value)) => Ok(value),
                        None => Ok(Value::Undefined),
                    }
                },
            )
            .map_err(|error| format!("__sw_take: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_count",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    let state = state.borrow();
                    let Some(queue) = state.queues.get(&id) else {
                        return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                    };
                    Ok::<Value, RuntimeError>(Value::Float(queue.items.len() as f64))
                },
            )
            .map_err(|error| format!("__sw_count: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_size",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    let state = state.borrow();
                    let Some(queue) = state.queues.get(&id) else {
                        return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                    };
                    Ok::<Value, RuntimeError>(Value::Float(queue.size as f64))
                },
            )
            .map_err(|error| format!("__sw_size: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_close",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    let mut state = state.borrow_mut();
                    if let Some(queue) = state.queues.get_mut(&id) {
                        queue.closed = true;
                    }
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sw_close: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_reset",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    let mut state = state.borrow_mut();
                    if let Some(queue) = state.queues.get_mut(&id) {
                        queue.reset();
                    }
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sw_reset: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_closed",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    let state = state.borrow();
                    let Some(queue) = state.queues.get(&id) else {
                        return Err(RuntimeError::Invariant("stream/web: unknown queue"));
                    };
                    Ok::<Value, RuntimeError>(Value::Bool(queue.closed))
                },
            )
            .map_err(|error| format!("__sw_closed: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__sw_drop",
                1,
                move |_scope: &mut Scope<'_>, id: f64| {
                    let id = id_of(id)?;
                    state.borrow_mut().queues.remove(&id);
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sw_drop: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("stream/web prelude: {error}"))?;

    let web = driver
        .eval("globalThis.__stream_web")
        .map_err(|error| format!("stream/web object: {error}"))?;
    let names = [
        "ReadableStream",
        "ReadableStreamDefaultReader",
        "ReadableStreamBYOBReader",
        "ReadableStreamDefaultController",
        "ReadableByteStreamController",
        "WritableStream",
        "WritableStreamDefaultWriter",
        "WritableStreamDefaultController",
        "TransformStream",
        "TransformStreamDefaultController",
        "ByteLengthQueuingStrategy",
        "CountQueuingStrategy",
    ];
    let exports = super::collect(driver, &web, &names)?;
    super::define_with_alias(driver, "stream/web", exports)?;
    Ok(())
}
