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
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}

impl Pending {
    pub fn new() -> Self {
        let (sender, receiver) = smol::channel::unbounded();
        Pending {
            inner: Rc::new(Inner {
                next_id: Cell::new(0),
                slots: RefCell::new(BTreeMap::new()),
                sender,
                receiver,
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

    /// How many things are still not done (started but the result has not come back, or it has come back but has not been settled).
    pub fn outstanding(&self) -> usize {
        self.inner.slots.borrow().len()
    }
}

/// A round-trip ticket for one piece of work. It cannot be cloned out, so one piece of work is settled only once.
pub struct Job {
    id: u64,
    sender: smol::channel::Sender<(u64, Reply)>,
}

impl Job {
    pub fn finish(self, reply: Reply) {
        // channel is unbounded, so try_send only fails when 'the receiving end is gone';
        // At that point the realm no longer wants this result; just drop it.
        let _ = self.sender.try_send((self.id, reply));
    }
}
