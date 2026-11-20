//! Timer table. The event loop is its only executor.
//!
//! Due callbacks are taken out here, and then executed by the loop with the Context—because settling
//! a promise needs a &mut runtime entry point; not just any thread can do it whenever it wants.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use quickjs_oxide::engine::api::core::{Context, RuntimeError, Value};

/// What to do when due. The parameter is the realm held by the loop.
pub type TimerCallback = Box<dyn FnOnce(&mut Context) -> Result<(), String>>;

type Key = (Instant, u64);

struct Table {
    next_id: u64,
    entries: BTreeMap<Key, TimerCallback>,
    cancelled: HashSet<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerId(u64);

#[derive(Clone)]
pub struct Timers(Rc<RefCell<Table>>);

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

impl Timers {
    pub fn new() -> Self {
        Timers(Rc::new(RefCell::new(Table {
            next_id: 1,
            entries: BTreeMap::new(),
            cancelled: HashSet::new(),
        })))
    }

    pub fn at(&self, when: Instant, callback: TimerCallback) -> TimerId {
        let mut table = self.0.borrow_mut();
        let id = table.next_id;
        table.next_id += 1;
        table.entries.insert((when, id), callback);
        TimerId(id)
    }

    pub fn after(&self, after: Duration, callback: TimerCallback) -> TimerId {
        self.at(Instant::now() + after, callback)
    }

    pub fn cancel(&self, id: TimerId) {
        let mut table = self.0.borrow_mut();
        table.cancelled.insert(id.0);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.0.borrow().entries.keys().next().map(|(when, _)| *when)
    }

    pub fn len(&self) -> usize {
        self.0.borrow().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().entries.is_empty()
    }

    /// Settle all due ones and return how many actually ran.
    /// Move the due ones out first, then execute them; scheduling a new timer inside a callback won't run into borrow conflicts.
    pub fn fire_due(&self, now: Instant, context: &mut Context) -> Result<usize, String> {
        let due: Vec<(u64, TimerCallback)> = {
            let mut table = self.0.borrow_mut();
            let mut due = Vec::new();
            while let Some((when, id)) = table.entries.keys().next().copied() {
                if when > now {
                    break;
                }
                let callback = table.entries.remove(&(when, id)).expect("entry");
                if table.cancelled.remove(&id) {
                    continue;
                }
                due.push((id, callback));
            }
            due
        };
        let count = due.len();
        for (_, callback) in due {
            callback(context)?;
        }
        Ok(count)
    }

    /// Attach a promise to a timer. The foundation of setTimeout.
    pub fn promise_after(&self, after: Duration, capability: PromiseSlot) -> TimerId {
        self.after(
            after,
            Box::new(move |context: &mut Context| {
                capability
                    .resolve(context)
                    .map_err(|error| format!("timer resolve: {error}"))
            }),
        )
    }
}

/// The half-promise obtained outside the event loop can only be settled once.
pub struct PromiseSlot {
    capability: Option<quickjs_oxide::engine::api::core::PromiseCapability>,
}

impl PromiseSlot {
    pub fn new(capability: quickjs_oxide::engine::api::core::PromiseCapability) -> Self {
        PromiseSlot {
            capability: Some(capability),
        }
    }

    pub fn resolve(mut self, context: &mut Context) -> Result<(), String> {
        match self.capability.take() {
            Some(capability) => capability
                .resolve(context, Value::Undefined)
                .map_err(|error| format!("{error}")),
            None => Ok(()),
        }
    }
}

/// Convenience: given a number of milliseconds, create a promise from scope and attach a timer to it.
pub fn sleep(
    timers: &Timers,
    scope: &mut quickjs_oxide::engine::api::core::Scope<'_>,
    ms: f64,
) -> Result<Value, RuntimeError> {
    let capability = scope.new_promise()?;
    let promise = capability.promise.clone();
    let after = Duration::from_secs_f64(ms.max(0.0) / 1000.0);
    timers.promise_after(after, PromiseSlot::new(capability));
    Ok(Value::Object(promise))
}
