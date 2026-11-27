//! Time, deadline, and cross-thread cancellation.
//!
//! The three are grouped together because they are all inputs to whether this round can keep running:
//! interrupt only looks at this one check; the event loop uses the same clock to compute timeouts and timer.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

struct Inner {
    start: Instant,
    deadline: Cell<Option<Instant>>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct Clock(Rc<Inner>);

/// A cancellation switch that can be handed off to other threads (connection dropped, client gone).
#[derive(Clone)]
pub struct CancelHandle(Arc<AtomicBool>);

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Clock(Rc::new(Inner {
            start: Instant::now(),
            deadline: Cell::new(None),
            cancelled: Arc::new(AtomicBool::new(false)),
        }))
    }

    pub fn start(&self) -> Instant {
        self.0.start
    }

    /// Microseconds since the clock was created. The same order of magnitude as performance.now() seen from the JS side.
    pub fn elapsed_us(&self) -> f64 {
        self.0.start.elapsed().as_micros() as f64
    }

    /// Nanoseconds since the clock was created: one f64, exact for 104 days.
    pub fn elapsed_ns(&self) -> f64 {
        self.0.start.elapsed().as_nanos() as f64
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.0.deadline.get()
    }

    pub fn set_deadline_in(&self, after: Duration) {
        self.0.deadline.set(Some(Instant::now() + after));
    }

    pub fn set_deadline_at(&self, at: Instant) {
        self.0.deadline.set(Some(at));
    }

    pub fn clear_deadline(&self) {
        self.0.deadline.set(None);
    }

    /// The statement called inside interrupt: either a timeout or an external stop request counts as stopping.
    pub fn stop_requested(&self) -> bool {
        self.cancelled()
            || self
                .0
                .deadline
                .get()
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle(Arc::clone(&self.0.cancelled))
    }
}

impl CancelHandle {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Puts the switch back, once whatever the cancel was stopping has unwound.
    ///
    /// A cancel is a latch: interrupt keeps answering yes until it is cleared, so a driver that
    /// cancelled one run would refuse every loop after it. Clearing is what makes the realm
    /// usable again after a kill.
    pub fn revoke(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
