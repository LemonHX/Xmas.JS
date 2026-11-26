//! timers. Scheduling is on the Rust side (the host's timer table); argument passing and wrapping are on the JS side.
//!
//! The old implementation (modules/src/timers.rs) used tokio's Sleep + Notify + rquickjs's
//! Persistent<Function>, and it also carried that whole async_hooks setup. Here everything is replaced with the host's
//! BTreeMap timer table: when due, the event loop brings the Context to execute it; callbacks can directly call JS.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use quickjs_oxide::engine::api::core::{Js, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::host::{Console, Level, TimerId, Timers};

const PRELUDE: &str = include_str!("../../js/timers.js");

struct State {
    next_handle: i64,
    handles: HashMap<i64, TimerId>,
}

/// A timer that has already been scheduled. If repeat is Some, it is an interval.
struct Plan {
    handle: i64,
    repeat: Option<Duration>,
    callback: Value,
    args: Value,
    next_at: Cell<Instant>,
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    driver
        .eval(PRELUDE)
        .map_err(|error| format!("timers prelude: {error}"))?;

    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let invoke = driver
        .eval("globalThis.__timer_invoke")
        .map_err(|error| format!("__timer_invoke: {error}"))?;

    let timers = driver.host.timers.clone();
    let console = driver.host.console.clone();
    let state: Rc<RefCell<State>> = Rc::new(RefCell::new(State {
        next_handle: 1,
        handles: HashMap::new(),
    }));

    // __timer_set(callback, ms, args, repeat) -> handle
    {
        let timers = timers.clone();
        let console = console.clone();
        let state = Rc::clone(&state);
        let invoke = invoke.clone();
        driver
            .context()
            .define_function(
                &global,
                "__timer_set",
                4,
                move |scope: &mut Scope<'_>,
                      callback: Value,
                      ms: f64,
                      args: Value,
                      repeat: bool| {
                    set(
                        &timers, &console, &state, scope, &invoke, callback, ms, args, repeat,
                    )
                },
            )
            .map_err(|error| format!("__timer_set: {error}"))?;
    }

    // __timer_clear(handle)
    {
        let timers = timers.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__timer_clear",
                1,
                move |_scope: &mut Scope<'_>, handle: f64| {
                    let handle = handle as i64;
                    if let Some(id) = state.borrow_mut().handles.remove(&handle) {
                        timers.cancel(id);
                        Ok::<Value, RuntimeError>(Value::Bool(true))
                    } else {
                        Ok(Value::Bool(false))
                    }
                },
            )
            .map_err(|error| format!("__timer_clear: {error}"))?;
    }

    let names = [
        "setTimeout",
        "setInterval",
        "setImmediate",
        "clearTimeout",
        "clearInterval",
        "queueMicrotask",
    ];
    let module = driver
        .eval("globalThis.__timers_module")
        .map_err(|error| format!("timers module: {error}"))?;
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "timers", exports)?;

    let promise_names = ["setTimeout", "setImmediate", "setInterval", "scheduler"];
    let promises = driver
        .eval("globalThis.__timers_promises")
        .map_err(|error| format!("timers/promises module: {error}"))?;
    let promise_exports = super::collect(driver, &promises, &promise_names)?;
    super::define_with_alias(driver, "timers/promises", promise_exports)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set(
    timers: &Timers,
    console: &Console,
    state: &Rc<RefCell<State>>,
    scope: &mut Scope<'_>,
    invoke: &Value,
    callback: Value,
    ms: f64,
    args: Value,
    repeat: bool,
) -> Result<Value, RuntimeError> {
    let Value::Object(object) = &callback else {
        return Err(RuntimeError::Invariant("timer callback is not a function"));
    };
    if scope.as_callable(object)?.is_none() {
        return Err(RuntimeError::Invariant("timer callback is not a function"));
    }
    let Value::Object(invoke_object) = invoke else {
        return Err(RuntimeError::Invariant("__timer_invoke is not a function"));
    };
    let invoke = scope
        .as_callable(invoke_object)?
        .ok_or(RuntimeError::Invariant("__timer_invoke is not a function"))?;

    let handle = {
        let mut state = state.borrow_mut();
        let handle = state.next_handle;
        state.next_handle += 1;
        handle
    };

    // setImmediate must be ordered before setTimeout(fn, 0): use a time far in the past.
    let (when, repeat) = if ms < 0.0 {
        (Instant::now() - Duration::from_secs(600), None)
    } else if repeat {
        let every = Duration::from_secs_f64(ms / 1000.0);
        (Instant::now() + every, Some(every))
    } else {
        (Instant::now() + Duration::from_secs_f64(ms / 1000.0), None)
    };

    let plan = Rc::new(Plan {
        handle,
        repeat,
        callback,
        args,
        next_at: Cell::new(when),
    });
    arm(timers, state, console, &invoke, &plan);
    Ok(Value::Float(handle as f64))
}

fn arm(
    timers: &Timers,
    state: &Rc<RefCell<State>>,
    console: &Console,
    invoke: &quickjs_oxide::engine::api::core::CallableRef,
    plan: &Rc<Plan>,
) {
    let timers_next = timers.clone();
    let state_next = Rc::clone(state);
    let console_next = console.clone();
    let invoke_next = invoke.clone();
    let plan_next = Rc::clone(plan);

    let id = timers.at(
        plan.next_at.get(),
        Box::new(move |context| {
            let outcome = context.call(
                &invoke_next,
                Value::Undefined,
                &[plan_next.callback.clone(), plan_next.args.clone()],
            );
            if let Err(error) = outcome {
                console_next.line(Level::Error, &format!("timer callback threw: {error}"));
            }
            match plan_next.repeat {
                Some(every) => {
                    // The callback may already have called clearInterval: if the handle is not in the table, don't reschedule,
                    // otherwise this rescheduling will revive the interval that was just cleared.
                    if state_next.borrow().handles.contains_key(&plan_next.handle) {
                        plan_next.next_at.set(Instant::now() + every);
                        arm(
                            &timers_next,
                            &state_next,
                            &console_next,
                            &invoke_next,
                            &plan_next,
                        );
                    }
                }
                None => {
                    state_next.borrow_mut().handles.remove(&plan_next.handle);
                }
            }
            Ok(())
        }),
    );
    state.borrow_mut().handles.insert(plan.handle, id);
}
