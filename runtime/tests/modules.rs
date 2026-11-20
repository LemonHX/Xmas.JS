//! console and timers, carried over from the probe.
//! Each case mirrors behaviour the old modules/ implementation had.

use xmas_runtime::testkit::{Harness, number_of, text_of};

fn idle(h: &mut Harness) {
    smol::block_on(async { h.driver().run_until_idle().await }).unwrap();
}

#[test]
fn console_writes_to_the_host_sink() {
    let mut h = Harness::with_modules();
    h.eval("console.log('a', 1, { b: 2 }); console.warn('w'); console.assert(false, 'boom');")
        .unwrap();
    let out = h.host().console.take();
    assert!(out.contains("[log] a 1 {\"b\":2}"), "{out}");
    assert!(out.contains("[warn] w"), "{out}");
    assert!(out.contains("[error] Assertion failed: boom"), "{out}");
}

#[test]
fn console_time_and_time_end() {
    let mut h = Harness::with_modules();
    h.eval("console.time('t'); console.timeEnd('t');").unwrap();
    let out = h.host().console.take();
    assert!(out.contains("t: "), "{out}");
    assert!(out.contains("ms"), "{out}");
}

#[test]
fn set_timeout_orders_by_deadline_and_passes_args() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout((x, y) => order.push(x + y), 3, 'a', 'b');
         setTimeout(() => order.push('first'), 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "first,ab");
}

#[test]
fn set_immediate_runs_before_timeout_zero() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout(() => order.push('timeout'), 0);
         setImmediate(() => order.push('immediate'));",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "immediate,timeout");
}

#[test]
fn clear_timeout_cancels() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.fired = 0;
         const handle = setTimeout(() => { fired++; }, 0);
         clearTimeout(handle);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("fired"), 0.0);
}

#[test]
fn set_interval_repeats_until_cleared() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.n = 0;
         const handle = setInterval(() => { n++; if (n === 3) clearInterval(handle); }, 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("n"), 3.0);
}

#[test]
fn timer_callback_throwing_does_not_kill_the_loop() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.after = 0;
         setTimeout(() => { throw new Error('boom'); }, 0);
         setTimeout(() => { after = 1; }, 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("after"), 1.0);
    let out = h.host().console.take();
    assert!(out.contains("timer callback threw"), "{out}");
}

#[test]
fn timers_module_by_name() {
    // The shape of test_timers in the old modules/src/timers.rs: import { setTimeout }, await.
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "timers_test.mjs",
            "import { setTimeout } from 'timers';
             export async function test() {
               return new Promise((resolve) => setTimeout(() => resolve('timeout'), 2));
             }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(text_of(&settled), Some("timeout".to_string()));
}

#[test]
fn timers_promises_module_by_name() {
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "timers_promises_test.mjs",
            "import { setTimeout } from 'timers/promises';
             export async function test() { return await setTimeout(2, 'later'); }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(text_of(&settled), Some("later".to_string()));
}

#[test]
fn queue_microtask_runs_before_timers() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout(() => order.push('timer'), 0);
         queueMicrotask(() => order.push('micro'));",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "micro,timer");
}

#[test]
fn timer_handle_is_a_number() {
    let mut h = Harness::with_modules();
    let handle = h.eval("setTimeout(() => {}, 1000)").unwrap();
    assert!(number_of(&handle).is_some(), "the handle must be a number: {handle:?}");
    h.eval("clearTimeout(0)").unwrap();
}

#[test]
fn event_target_add_remove_once() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.seen = [];
         const target = new EventTarget();
         const onPing = (event) => seen.push('ping:' + event.type);
         target.addEventListener('ping', onPing);
         target.addEventListener('once', () => seen.push('once'), { once: true });
         target.dispatchEvent(new Event('ping'));
         target.removeEventListener('ping', onPing);
         target.dispatchEvent(new Event('ping'));
         target.dispatchEvent(new Event('once'));
         target.dispatchEvent(new Event('once'));",
    )
    .unwrap();
    assert_eq!(h.expect_text("seen.join(',')"), "ping:ping,once");
}

#[test]
fn event_prevent_default_and_immediate_stop() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.log = [];
         const target = new EventTarget();
         target.addEventListener('go', (event) => { log.push('a'); event.preventDefault(); event.stopImmediatePropagation(); });
         target.addEventListener('go', () => log.push('b'));
         const event = new Event('go', { cancelable: true });
         globalThis.notCancelled = target.dispatchEvent(event);
         globalThis.defaultPrevented = event.defaultPrevented;",
    )
    .unwrap();
    assert_eq!(h.expect_text("log.join(',')"), "a");
    assert_eq!(h.expect_number("notCancelled ? 1 : 0"), 0.0);
    assert_eq!(h.expect_number("defaultPrevented ? 1 : 0"), 1.0);
}

#[test]
fn custom_event_carries_detail() {
    let mut h = Harness::with_modules();
    h.eval(
        "const target = new EventTarget();
         globalThis.detail = null;
         target.addEventListener('custom', (event) => { globalThis.detail = event.detail; });
         target.dispatchEvent(new CustomEvent('custom', { detail: { n: 7 } }));",
    )
    .unwrap();
    assert_eq!(h.expect_number("detail.n"), 7.0);
}

#[test]
fn event_emitter_round_trip() {
    let mut h = Harness::with_modules();
    h.eval(
        "const { EventEmitter } = __events_module;
         const emitter = new EventEmitter();
         globalThis.trace = [];
         emitter.on('data', (a, b) => trace.push('on:' + a + b));
         emitter.once('data', () => trace.push('once'));
         emitter.emit('data', 1, 2);
         emitter.emit('data', 3, 4);
         globalThis.count = emitter.listenerCount('data');",
    )
    .unwrap();
    assert_eq!(h.expect_text("trace.join(',')"), "on:12,once,on:34");
    assert_eq!(h.expect_number("count"), 1.0);
}

#[test]
fn event_emitter_error_without_listener_throws() {
    let mut h = Harness::with_modules();
    let error = h
        .eval_err("const { EventEmitter } = __events_module; new EventEmitter().emit('error', new Error('boom'));");
    assert!(error.text.contains("boom"), "{error:?}");
}

#[test]
fn abort_controller_fires_abort_event() {
    let mut h = Harness::with_modules();
    h.eval(
        "const controller = new AbortController();
         globalThis.order = [];
         controller.signal.addEventListener('abort', () => order.push('listener'));
         controller.signal.onabort = () => order.push('onabort');
         globalThis.before = controller.signal.aborted;
         controller.abort();
         globalThis.after = controller.signal.aborted;
         globalThis.reasonName = controller.signal.reason.name;",
    )
    .unwrap();
    assert_eq!(h.expect_number("before ? 1 : 0"), 0.0);
    assert_eq!(h.expect_number("after ? 1 : 0"), 1.0);
    assert_eq!(h.expect_text("order.join(',')"), "listener,onabort");
    assert_eq!(h.expect_text("reasonName"), "AbortError");
}

#[test]
fn abort_signal_throw_if_aborted() {
    let mut h = Harness::with_modules();
    let error = h
        .eval_err("AbortSignal.abort(new Error('nope')).throwIfAborted();");
    assert!(error.text.contains("nope"), "{error:?}");
}

#[test]
fn abort_signal_any_and_timeout() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.which = null;
         const first = new AbortController();
         const second = new AbortController();
         const both = AbortSignal.any([first.signal, second.signal]);
         both.addEventListener('abort', () => { which = 'both'; });
         second.abort();
         globalThis.timeoutFired = null;
         const timed = AbortSignal.timeout(2);
         timed.addEventListener('abort', () => { timeoutFired = timed.reason.name; });",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("which"), "both");
    assert_eq!(h.expect_text("timeoutFired"), "AbortError");
}
