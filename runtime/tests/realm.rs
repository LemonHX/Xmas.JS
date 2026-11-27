//! xmas:realm: a source string as a module, evaluated on demand.

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
