//! xmas:realm: a source string as a module, evaluated on demand.

use std::time::Duration;

use quickjs_oxide::engine::api::core::Value;
use xmas_runtime::testkit::{Harness, text_of};

/// Evaluate `body` with the realm prelude in scope, and read back what it answers.
///
/// The prelude installs `xmas:realm` on the global, so a test reaches it without importing a
/// module: `Harness::module` runs a module to completion but cannot wait for what the module
/// started, and `realm.evaluate` answers a promise.
fn run(h: &mut Harness, body: &str) -> String {
    let source = format!(
        "(function(){{ const {{ createRealm }} = globalThis.__realm_module; {body} }})()"
    );
    match h.eval(&source) {
        Ok(value) => text_of(&value).unwrap_or_else(|| format!("{value:?}")),
        Err(error) => panic!("{body} failed: {error}"),
    }
}

/// Run `h`'s jobs until `globalThis.__settled` is set, or until there is nothing left to run.
///
/// An evaluation is promise-driven: the import the realm starts settles over several turns of the
/// job queue, and how many depends on what the module itself awaited.
fn settle(h: &mut Harness) {
    for _ in 0..64 {
        let slice = h
            .driver()
            .slice()
            .unwrap_or_else(|error| panic!("slice: {error}"));
        if h.eval("globalThis.__settled !== undefined").unwrap_or(Value::Undefined)
            == Value::Bool(true)
        {
            return;
        }
        if slice.jobs == 0 && slice.timers == 0 {
            return;
        }
    }
}

/// Evaluate `promise` in the realm, wait for it, and render what it answered.
///
/// A realm call answers a promise, so the answer is read back the way a caller reads one: through
/// `then`, once the jobs behind it have run.
fn settled(h: &mut Harness, promise: &str) -> String {
    h.eval("globalThis.__settled = undefined;").unwrap();
    h.eval(&format!(
        "{promise}.then(
           function (r) {{
             globalThis.__settled = r.value === undefined ? 'value:undefined' : 'value:' + JSON.stringify(r.value);
           }},
           function (e) {{ globalThis.__settled = 'error:' + e.name + ':' + e.message; }});
         undefined"
    ))
    .unwrap_or_else(|error| panic!("{promise}: {error}"));
    settle(h);
    run(h, "return String(globalThis.__settled);")
}

fn realm(h: &mut Harness) {
    run(h, "globalThis.__r = createRealm({ name: 'agent' }); return 'ok';");
}

#[test]
fn a_source_string_runs_as_a_module() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    assert_eq!(
        settled(&mut h, "globalThis.__r.evaluate('export default 1 + 1;')"),
        "value:2"
    );
}

#[test]
fn top_level_await_is_waited_for() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    assert_eq!(
        settled(
            &mut h,
            "globalThis.__r.evaluate('const x = await Promise.resolve(5); export default x * 2;')"
        ),
        "value:10"
    );
}

#[test]
fn an_await_inside_the_module_does_not_change_the_answer() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    // The await makes the module asynchronous, which is the path where the evaluation settles from
    // a job rather than from the call itself.
    assert_eq!(
        settled(
            &mut h,
            r#"globalThis.__r.evaluate('await null; await null; export default "deep";')"#
        ),
        "value:\"deep\""
    );
}

#[test]
fn a_module_without_a_default_export_answers_undefined() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    assert_eq!(
        settled(&mut h, "globalThis.__r.evaluate('globalThis.__side = 1;')"),
        "value:undefined"
    );
    assert_eq!(h.expect_number("globalThis.__side"), 1.0);
}

#[test]
fn the_same_identifier_runs_again_as_a_new_module() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    // A module record is handed back as it is, so a second evaluation under one name would answer
    // the first run's namespace and never run. The run number in the name is what stops that.
    let source = "globalThis.__n = (globalThis.__n || 0) + 1; export default globalThis.__n;";
    assert_eq!(
        settled(
            &mut h,
            &format!("globalThis.__r.evaluate({source:?}, {{ identifier: 'esm.js' }})")
        ),
        "value:1"
    );
    assert_eq!(
        settled(
            &mut h,
            &format!("globalThis.__r.evaluate({source:?}, {{ identifier: 'esm.js' }})")
        ),
        "value:2"
    );
}

#[test]
fn a_syntax_error_reaches_the_caller() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    let out = settled(&mut h, "globalThis.__r.evaluate('export default ;')");
    assert!(out.starts_with("error:"), "{out}");
    assert!(out.contains("SyntaxError"), "{out}");
}

#[test]
fn a_throw_reaches_the_caller() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    let out = settled(&mut h, "globalThis.__r.evaluate('throw new Error(\\'boom\\');')");
    assert!(out.starts_with("error:"), "{out}");
    assert!(out.contains("boom"), "{out}");
}

#[test]
fn a_killed_realm_refuses_to_evaluate() {
    let mut h = Harness::with_modules();
    run(
        &mut h,
        "globalThis.__r = createRealm({ name: 'gone' }); globalThis.__r.kill(); return 'ok';",
    );
    assert_eq!(h.expect_number("globalThis.__r.killed ? 1 : 0"), 1.0);
    let out = run(
        &mut h,
        "try { globalThis.__r.evaluate('export default 1;'); return 'no throw'; } \
         catch (e) { return e.message; }",
    );
    assert!(out.contains("gone"), "{out}");
}

#[test]
fn evaluate_refuses_a_source_that_is_not_a_string() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    let out = run(
        &mut h,
        "try { globalThis.__r.evaluate(42); return 'no throw'; } catch (e) { return e.name; }",
    );
    assert_eq!(out, "TypeError");
}

/// The promise `evaluate` answered with is rejected when the realm is killed under it, rather
/// than left waiting for a module the engine threw out of.
#[test]
fn kill_rejects_a_waiting_evaluation() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    h.eval(
        "globalThis.__state = 'pending';
         globalThis.__r.evaluate('await new Promise(function () {})').then(
           function () { globalThis.__state = 'resolved'; },
           function (e) { globalThis.__state = 'rejected:' + e.message; });
         undefined",
    )
    .unwrap();
    for _ in 0..8 {
        let _ = h.driver().slice();
    }
    run(&mut h, "globalThis.__r.kill(); return 'killed';");
    for _ in 0..8 {
        let _ = h.driver().slice();
    }
    let state = run(&mut h, "return String(globalThis.__state);");
    assert!(state.contains("rejected"), "the evaluation was not rejected: {state}");
}

/// A loop the realm is running is interrupted by its kill switch, from another thread -- which is
/// the only way it can be, because JS gets no turn while the loop spins.
#[test]
fn kill_stops_a_running_loop() {
    let mut h = Harness::with_modules();
    realm(&mut h);
    h.eval(
        "globalThis.__state = 'pending';
         globalThis.__r.evaluate('globalThis.__ticks = 0; while (true) { globalThis.__ticks = globalThis.__ticks + 1; }').then(
           function () { globalThis.__state = 'resolved'; },
           function (e) { globalThis.__state = 'rejected:' + e.message; });
         undefined",
    )
    .unwrap();

    let switch = h.driver().host.realms.switch(0).expect("realm 0");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        switch.kill();
    });

    let mut stopped = false;
    for _ in 0..50 {
        if h
            .driver()
            .timed(Duration::from_millis(400), |d| d.slice())
            .is_err()
        {
            stopped = true;
            break;
        }
    }
    for _ in 0..40 {
        let _ = h.driver().slice();
    }

    assert!(stopped, "the loop was never interrupted");
    let state = run(&mut h, "return String(globalThis.__state);");
    assert!(state.contains("rejected"), "the evaluation was not rejected: {state}");
    assert_eq!(run(&mut h, "return String(globalThis.__r.killed);"), "true");
    // The driver is not left poisoned by the kill: a later loop still runs.
    assert_eq!(run(&mut h, "for (let i = 0; i < 1000; i++) {} return String(1 + 1);"), "2");
    let out = run(
        &mut h,
        "try { globalThis.__r.evaluate('1'); return 'no throw'; } catch (e) { return e.message; }",
    );
    assert!(out.contains("was killed"), "{out}");
}


#[test]
fn provide_makes_a_name_importable() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const r = createRealm({ name: 'a' });
         r.provide('host:thing', { answer: 41, default: function () { return 'd'; } });
         r.evaluate(\"import thing, { answer } from 'host:thing'; globalThis.__got = [answer, thing()];\").then(
           () => { globalThis.__settled = true; },
           (e) => { globalThis.__error = String(e); globalThis.__settled = true; },
         );
         return 'started';",
    );
    assert_eq!(out, "started");
    settle(&mut h);
    assert_eq!(
        run(&mut h, "return String(globalThis.__error);"),
        "undefined",
        "the import failed"
    );
    assert_eq!(run(&mut h, "return String(globalThis.__got);"), "41,d");
}

#[test]
fn provide_replaces_a_name_it_was_given_before() {
    let mut h = Harness::with_modules();
    run(
        &mut h,
        "const r = createRealm({ name: 'a' });
         r.provide('host:n', { v: 1 });
         r.evaluate(\"import { v } from 'host:n'; globalThis.__first = v;\").then(
           () => { globalThis.__settled = true; }, () => { globalThis.__settled = true; });
         return 'started';",
    );
    settle(&mut h);
    assert_eq!(run(&mut h, "return String(globalThis.__first);"), "1");

    run(
        &mut h,
        "globalThis.__settled = undefined;
         globalThis.__r2 = createRealm({ name: 'b' });
         globalThis.__r2.provide('host:n', { v: 2 });
         globalThis.__r2.evaluate(\"import { v } from 'host:n'; globalThis.__second = v;\").then(
           () => { globalThis.__settled = true; }, () => { globalThis.__settled = true; });
         return 'started';",
    );
    settle(&mut h);
    assert_eq!(run(&mut h, "return String(globalThis.__second);"), "2");
}

#[test]
fn provide_rejects_a_name_that_is_not_a_module() {
    let mut h = Harness::with_modules();
    let out = run(
        &mut h,
        "const r = createRealm({ name: 'a' });
         try { r.provide('', {}); } catch (e) { return e.message; }
         return 'no throw';",
    );
    assert!(out.contains("needs a name"), "{out}");
}
