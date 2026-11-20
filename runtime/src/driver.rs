//! Event loop: the job queue and timers alternate, one slice is 200µs.
//!
//! The numbers come from docs/event-loop.md: the poll overhead for a 200µs slice is about 0.17%,
//! Check the clock once every 32 jobs; pushing it lower only costs more poll and saves no time.
//!
//! When idle, wait for timers with smol::Timer, so the reactor is also running and the host's IO futures don't starve.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use quickjs_oxide::engine::api::core::{
    Context, HeapLimits, ObjectRef, PromiseCapability, PromiseSnapshot, Runtime, RuntimeError,
    RuntimeOptions, Scope, Value,
};

use crate::host::{Host, Level, PromiseSlot, sleep};

pub const SLICE: Duration = Duration::from_micros(200);
const CLOCK_EVERY: u64 = 32;

/// The default heap limit per realm. The default value is unlimited (usize::MAX),
/// A memory bomb in one realm can drag down the entire process, so there must be a number here.
pub const DEFAULT_HEAP: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct Slice {
    pub jobs: u64,
    pub timers: usize,
    /// Host async work (fs, etc.) settled during this slice.
    pub settled: usize,
}

pub struct Driver {
    pub host: Host,
    pub notice_us: f64,
    /// The heap cap this realm was built with, so process.memoryUsage can report it.
    pub heap_cap: usize,
    /// Heap bytes as of the last loop turn. The engine refuses to be re-entered
    /// from a host callback, so process.memoryUsage reads this instead of asking
    /// mid-call; it is stale by at most one turn of the loop.
    heap_used: Rc<Cell<usize>>,
    runtime: Runtime,
    context: Context,
}

impl Driver {
    pub fn new(host: Host) -> Result<Self, String> {
        Self::with_heap(host, DEFAULT_HEAP)
    }

    pub fn with_heap(host: Host, max_bytes: usize) -> Result<Self, String> {
        let options = RuntimeOptions {
            heap: HeapLimits { max_bytes },
            ..RuntimeOptions::default()
        };
        let runtime = Runtime::with_host(host.engine(), options);
        let context = runtime.context();
        let mut driver = Driver {
            host,
            notice_us: 0.0,
            heap_cap: max_bytes,
            heap_used: Rc::new(Cell::new(0)),
            runtime,
            context,
        };
        driver.install_globals()?;
        driver.refresh_heap();
        Ok(driver)
    }

    fn install_globals(&mut self) -> Result<(), String> {
        let global = self
            .context
            .global_object()
            .map_err(|error| format!("global: {error}"))?;

        let timers = self.host.timers.clone();
        self.context
            .define_function(
                &global,
                "__sleep",
                1,
                move |scope: &mut Scope<'_>, ms: f64| sleep(&timers, scope, ms),
            )
            .map_err(|error| format!("sleep: {error}"))?;

        let clock = self.host.clock.clone();
        self.context
            .define_function(&global, "__now_us", 0, move |_: &mut Scope<'_>| {
                Ok::<Value, RuntimeError>(Value::Float(clock.elapsed_us()))
            })
            .map_err(|error| format!("now: {error}"))?;

        let console = self.host.console.clone();
        self.context
            .define_function(
                &global,
                "__log",
                1,
                move |_: &mut Scope<'_>, text: String| {
                    console.line(Level::Log, &text);
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("log: {error}"))?;

        Ok(())
    }

    /// Attach a setTimeout-style promise to a host timer; both tests and modules/timers use it.
    pub fn sleep_ms(&mut self, scope: &mut Scope<'_>, ms: f64) -> Result<Value, RuntimeError> {
        sleep(&self.host.timers, scope, ms)
    }

    pub fn promise_timer(&self, after: Duration, capability: PromiseCapability) {
        self.host
            .timers
            .promise_after(after, PromiseSlot::new(capability));
    }

    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }

    pub fn context_ref(&self) -> &Context {
        &self.context
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The promise's state and result; if it isn't a promise, it's None.
    pub fn promise(&self, promise: &ObjectRef) -> Result<Option<PromiseSnapshot>, String> {
        self.runtime
            .promise_snapshot(promise)
            .map_err(|error| format!("{error}"))
    }

    /// Wait for one thing: the next timer to fire, or wait at most 1ms and check the reactor again.
    /// 1ms is the polling granularity in tests; production services wait directly on their own accept/IO futures,
    /// and don't go through this function.
    pub async fn wait_ready(&mut self) -> Result<(), String> {
        let ceiling = Duration::from_millis(1);
        let wait = match self.host.timers.next_deadline() {
            Some(when) => when.saturating_duration_since(Instant::now()).min(ceiling),
            None => ceiling,
        };
        smol::Timer::after(wait).await;
        Ok(())
    }

    pub fn eval(&mut self, source: &str) -> Result<Value, String> {
        match self.context.eval(source) {
            Ok(value) => Ok(value),
            Err(error) => Err(self.render(error)),
        }
    }

    /// Translate engine errors into human-readable words: for a thrown exception, read its own message,
    /// otherwise it is always "JavaScript exception", and tests can't reveal anything.
    pub fn render(&mut self, error: RuntimeError) -> String {
        match error {
            RuntimeError::Exception(value) => self.render_exception(&value),
            other => format!("{other}"),
        }
    }

    fn render_exception(&mut self, value: &Value) -> String {
        use quickjs_oxide::engine::api::core::{JsString, PropertyKey};
        let Value::Object(object) = value else {
            return match value {
                Value::String(text) => text.to_utf8_lossy(),
                other => format!("{other:?}"),
            };
        };
        let read = |context: &mut Context, name: &str| -> Option<String> {
            let key = PropertyKey::from_js_string(JsString::try_from_utf8(name).ok()?);
            match context.get_property(object, &key).ok()? {
                Value::String(text) => Some(text.to_utf8_lossy()),
                _ => None,
            }
        };
        if let Some(stack) = read(&mut self.context, "stack") {
            return stack;
        }
        let name = read(&mut self.context, "name").unwrap_or_else(|| "Error".to_string());
        let message = read(&mut self.context, "message").unwrap_or_default();
        if message.is_empty() {
            name
        } else {
            format!("{name}: {message}")
        }
    }

    /// A version without evaluation wrapping; tests need to check what is thrown themselves.
    pub fn eval_raw(&mut self, source: &str) -> Result<Value, RuntimeError> {
        self.context.eval(source)
    }

    pub fn eval_number(&mut self, source: &str) -> Result<f64, String> {
        match self.eval(source)? {
            Value::Float(value) => Ok(value),
            Value::Int(value) => Ok(value as f64),
            Value::Undefined => Ok(f64::NAN),
            other => Err(format!("expected number, got {other:?}")),
        }
    }

    /// Run within a deadline; remove the deadline once done.
    /// If this time is exceeded, the script is stopped by interrupt, and the realm remains usable.
    pub fn timed<T>(&mut self, within: Duration, body: impl FnOnce(&mut Self) -> T) -> T {
        self.host.clock.set_deadline_in(within);
        let outcome = body(self);
        self.host.clock.clear_deadline();
        outcome
    }

    /// Run one slice: jobs within the budget, plus timers that are due.
    /// Read the engine's heap report and keep it for process.memoryUsage. Only
    /// called while the realm is not entered: between turns of the loop.
    pub fn refresh_heap(&mut self) {
        if let Ok(report) = self.runtime.memory_report() {
            self.heap_used.set(report.reserved_bytes());
        }
    }

    /// A handle to the heap reading above, for the process module.
    pub fn heap_used(&self) -> Rc<Cell<usize>> {
        self.heap_used.clone()
    }

    pub fn slice(&mut self) -> Result<Slice, String> {
        self.refresh_heap();
        let started = Instant::now();
        let mut jobs = 0u64;
        while started.elapsed() < SLICE {
            if !self
                .runtime
                .is_job_pending()
                .map_err(|error| format!("{error}"))?
            {
                break;
            }
            if !self
                .runtime
                .run_pending_job()
                .map_err(|error| format!("{error}"))?
                .executed()
            {
                break;
            }
            jobs += 1;
            if jobs % CLOCK_EVERY == 0 && started.elapsed() >= SLICE {
                break;
            }
        }

        let now = Instant::now();
        if let Some(when) = self.host.timers.next_deadline() {
            if when <= now {
                let late = now.saturating_duration_since(when).as_micros() as f64;
                self.notice_us = self.notice_us.max(late);
            }
        }
        // Results coming back from host async work must be settled on this thread (resolve needs &mut Js)
        let settled = self.host.pending.drain(&mut self.context)?;
        let timers = self.host.timers.fire_due(now, &mut self.context)?;
        Ok(Slice {
            jobs,
            timers,
            settled,
        })
    }

    /// Run until there is no work left. In a real service this alternates with the socket; in the probe it just waits for timers.
    pub async fn run_until_idle(&mut self) -> Result<(), String> {
        loop {
            let slice = self.slice()?;
            let pending = self
                .runtime
                .is_job_pending()
                .map_err(|error| format!("{error}"))?;
            if pending || slice.jobs > 0 || slice.timers > 0 || slice.settled > 0 {
                continue;
            }
            match self.host.timers.next_deadline() {
                Some(when) => {
                    let wait = when.saturating_duration_since(Instant::now());
                    if wait > Duration::ZERO {
                        smol::Timer::after(wait).await;
                    }
                }
                // No timers, but host work is still in flight: wait for it to report back.
                None if self.host.pending.outstanding() > 0 => {
                    self.host.pending.wait().await;
                }
                None => return Ok(()),
            }
        }
    }

    /// The non-async version; waiting for timers relies on thread sleep. Only used in scenarios without IO futures.
    pub fn run_until_idle_blocking(&mut self) -> Result<(), String> {
        loop {
            let slice = self.slice()?;
            let pending = self
                .runtime
                .is_job_pending()
                .map_err(|error| format!("{error}"))?;
            if pending || slice.jobs > 0 || slice.timers > 0 || slice.settled > 0 {
                continue;
            }
            match self.host.timers.next_deadline() {
                Some(when) => {
                    let wait = when.saturating_duration_since(Instant::now());
                    if wait > Duration::ZERO {
                        std::thread::sleep(wait);
                    }
                }
                None if self.host.pending.outstanding() > 0 => {
                    self.host.pending.wait_blocking();
                }
                None => return Ok(()),
            }
        }
    }

    /// How many host async jobs have not landed yet.
    pub fn outstanding_io(&self) -> usize {
        self.host.pending.outstanding()
    }

    pub fn pending_jobs(&self) -> Result<bool, String> {
        self.runtime
            .is_job_pending()
            .map_err(|error| format!("{error}"))
    }
}
