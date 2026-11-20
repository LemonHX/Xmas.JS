//! The return channel for host async work: things like fs, crypto where 'JS gets a promise and the work happens elsewhere'.
//!
//! The shape is fixed in three steps: the host function creates a capability, parks a settlement closure to get a Job, and hands the work off to
//! smol::spawn; when it is done, Job::finish puts the result into the channel; each event-loop slice first
//! drains the channel once, and runs the settlement closure on the thread holding the realm (resolve needs &mut Js).
//!
//! Job only holds a Sender (Send + Clone), not an Rc, so the future can be handed to the global executor.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::Context;

/// The thing brought back by finished work.
#[derive(Debug)]
pub enum Payload {
    /// Raw bytes (reading files, calculating digests), which go out as an ArrayBuffer.
    Bytes(Vec<u8>),
    /// Data that is already text (JSON, paths), which goes out as a JS string and no longer goes through base64.
    Text(String),
}

impl From<Vec<u8>> for Payload {
    fn from(bytes: Vec<u8>) -> Self {
        Payload::Bytes(bytes)
    }
}

pub type Reply = Result<Payload, String>;

/// How to settle a result into a JS value after getting it. Runs on the thread that holds the realm.
pub type Settle = Box<dyn FnOnce(&mut Context, Reply)>;

#[derive(Clone)]
pub struct Pending {
    inner: Rc<Inner>,
}

struct Inner {
    next_id: Cell<u64>,
    slots: RefCell<BTreeMap<u64, Settle>>,
    sender: smol::channel::Sender<(u64, Reply)>,
    receiver: smol::channel::Receiver<(u64, Reply)>,
    /// Woken whenever a job finishes, so the event loop waits for the next result instead of
    /// asking again every millisecond.
    wake: smol::channel::Sender<()>,
    woken: smol::channel::Receiver<()>,
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}

impl Pending {
    pub fn new() -> Self {
        let (sender, receiver) = smol::channel::unbounded();
        let (wake, woken) = smol::channel::unbounded();
        Pending {
            inner: Rc::new(Inner {
                next_id: Cell::new(0),
                slots: RefCell::new(BTreeMap::new()),
                sender,
                receiver,
                wake,
                woken,
            }),
        }
    }

    /// Attach a settlement closure, returning the end that can send the result back.
    pub fn park(&self, settle: Settle) -> Job {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        self.inner.slots.borrow_mut().insert(id, settle);
        Job {
            id,
            sender: self.inner.sender.clone(),
            wake: self.inner.wake.clone(),
        }
    }

    /// Settle all results that have already come back, and return how many were settled.
    pub fn drain(&self, context: &mut Context) -> Result<usize, String> {
        let mut settled = 0;
        loop {
            let (id, reply) = match self.inner.receiver.try_recv() {
                Ok(item) => item,
                Err(_) => break,
            };
            let settle = self.inner.slots.borrow_mut().remove(&id);
            match settle {
                Some(settle) => {
                    settle(context, reply);
                    settled += 1;
                }
                None => return Err(format!("Settlement ID {id} has no corresponding closure")),
            }
        }
        Ok(settled)
    }

    /// Wait until any job finishes. Used by the event loop instead of polling on a timer.
    pub async fn wait(&self) {
        let _ = self.inner.woken.recv().await;
    }

    /// The same, for the blocking loop. Waiting on a channel that has a message already
    /// queued returns immediately, so a result that landed during the last slice is not missed.
    pub fn wait_blocking(&self) {
        let _ = self.inner.woken.recv_blocking();
    }

    /// How many things are still not done (started but the result has not come back, or it has come back but has not been settled).
    pub fn outstanding(&self) -> usize {
        self.inner.slots.borrow().len()
    }
}

/// Hand a Future to smol and give JS a promise for it.
///
/// Works are settled on the thread holding the realm, because resolving needs
/// a mutable engine; the future itself only has to be Send.
pub fn start<F>(
    scope: &mut quickjs_oxide::engine::api::core::Scope<'_>,
    pending: &Pending,
    work: F,
) -> Result<quickjs_oxide::engine::api::core::Value, quickjs_oxide::engine::api::core::RuntimeError>
where
    F: std::future::Future<Output = Reply> + Send + 'static,
{
    use quickjs_oxide::engine::api::core::{Js, JsString, NativeErrorKind, Value};
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let job = pending.park(Box::new(move |context: &mut Context, reply: Reply| {
        let value = match reply {
            Ok(Payload::Bytes(bytes)) => {
                crate::modules::bytes::give(context, &bytes).unwrap_or(Value::Undefined)
            }
            Ok(Payload::Text(text)) => match JsString::try_from_utf8(&text) {
                Ok(text) => Value::String(text),
                Err(_) => Value::Undefined,
            },
            Err(message) => match context.new_native_error(NativeErrorKind::Internal, &message) {
                Ok(value) => value,
                Err(_) => Value::Undefined,
            },
        };
        let _ = capability.resolve(context, value);
    }));
    smol::spawn(async move { job.finish(work.await) }).detach();
    Ok(Value::Object(promise))
}

/// A round-trip ticket for one piece of work. It cannot be cloned out, so one piece of work is settled only once.
pub struct Job {
    id: u64,
    sender: smol::channel::Sender<(u64, Reply)>,
    wake: smol::channel::Sender<()>,
}

impl Job {
    pub fn finish(self, reply: Reply) {
        // channel is unbounded, so try_send only fails when 'the receiving end is gone';
        // At that point the realm no longer wants this result; just drop it.
        let _ = self.sender.try_send((self.id, reply));
        // One wake-up per finished job. The channel is unbounded, so this never blocks and
        // never loses a wake-up; extra ones just make the next wait return at once.
        let _ = self.wake.try_send(());
    }
}
