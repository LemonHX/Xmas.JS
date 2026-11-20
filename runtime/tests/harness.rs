//! t11: Can the test harness replace the old test_async_with / call_test / ModuleEvaluator?
//! Each test case corresponds to one style in the old harness, with the shape aligned to modules/src/utils/test.rs.

use quickjs_oxide::engine::api::core::{RuntimeError, Scope, Value};
use xmas_runtime::testkit::{Harness, given_file, number_of, text_of};

#[test]
fn eval_number() {
    let mut h = Harness::new();
    assert_eq!(h.expect_number("1 + 1"), 2.0);
    assert_eq!(h.expect_text("'abc'.toUpperCase()"), "ABC");
    h.eval("globalThis.acc = 0; for (let i = 0; i < 10; i++) acc += i;").unwrap();
    assert_eq!(h.expect_number("acc"), 45.0);
}

#[test]
fn captures_thrown_error() {
    let mut h = Harness::new();
    let thrown = h.eval_err("throw new TypeError('boom')");
    assert_eq!(thrown.name, "TypeError");
    assert_eq!(thrown.message, "boom");
}

#[test]
fn rust_module_by_name() {
    // The spot in the old harness for ModuleEvaluator::eval_rust::<M>: a module built by Rust, imported by name.
    let mut h = Harness::new();
    let add = h.rust_function("add", 2, |_scope: &mut Scope<'_>, a: f64, b: f64| {
        Ok::<Value, RuntimeError>(Value::Float(a + b))
    });
    h.define_module("adder", vec![("add".to_string(), add)]);
    let namespace = h
        .module("main", "import { add } from 'adder'; export function test() { return add(2, 3); }")
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    assert_eq!(number_of(&value), Some(5.0));
}

#[test]
fn tsx_module_imports_tsx() {
    // Module table + loader: one tsx module imports another tsx module.
    let mut h = Harness::new();
    h.module(
        "util.tsx",
        "export interface P { n: number }\nexport const doubled = (p: P): number => p.n * 2;\n",
    )
    .unwrap();
    let namespace = h
        .module(
            "main.tsx",
            "import { doubled } from 'util.tsx';\nexport function test(): number { return doubled({ n: 21 }); }\n",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    assert_eq!(number_of(&value), Some(42.0));
}

#[test]
fn jsx_gets_transformed() {
    let mut h = Harness::new();
    h.eval(
        "globalThis.Fragment = 'Fragment';
         globalThis.h = (tag, props, ...kids) => tag + '[' + kids.join('|') + ']';",
    )
    .unwrap();
    let namespace = h
        .module(
            "view.entry.tsx",
            "export function test(): string { const name: string = 'x'; return <b class=\"c\">{name}</b>; }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    assert_eq!(text_of(&value), Some("b[x]".to_string()));
}

#[test]
fn async_test_with_timer() {
    // That into_future::<T> call in the old harness's call_test: get the result after the promise finishes.
    let mut h = Harness::new();
    let namespace = h
        .module(
            "timers_test",
            "export async function test() { await __sleep(2); return 'timeout'; }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(text_of(&settled), Some("timeout".to_string()));
}

#[test]
fn promise_chain_settles() {
    let mut h = Harness::new();
    let value = h
        .eval("Promise.resolve(1).then(v => v + 1).then(v => v * 10)")
        .unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(number_of(&settled), Some(20.0));
}

#[test]
fn error_from_module_call() {
    let mut h = Harness::new();
    let namespace = h
        .module("failing", "export function test() { throw new TypeError('bad input'); }")
        .unwrap();
    let thrown = h.call_err(&namespace, "test", &[]);
    assert_eq!(thrown.name, "TypeError");
    assert_eq!(thrown.message, "bad input");
}

#[test]
fn broken_module_fails_to_compile() {
    let mut h = Harness::new();
    let error = h.module("broken.ts", "export function f(: number { return 1; }").unwrap_err();
    assert!(error.contains("broken.ts"), "The error should include the module name: {error}");
}

#[test]
fn given_file_writes_a_file() {
    let path = given_file("hello");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    let mut h = Harness::new();
    let source = format!("export const content = {:?};", path.to_string_lossy());
    let namespace = h.module("file_test.ts", &source).unwrap();
    let value = h.property(&namespace, "content").unwrap();
    assert!(text_of(&value).unwrap().contains("xmas-test-"));
}

#[test]
fn host_timers_are_drained_by_the_loop() {
    // Host timer + promise: that setTimeout style in the old harness relies on tokio's timer,
    // Here it relies on the driver's timer table.
    let mut h = Harness::new();
    h.eval(
        "globalThis.order = [];
         __sleep(1).then(() => order.push('a'));
         __sleep(3).then(() => order.push('b'));
         __sleep(2).then(() => order.push('c'));",
    )
    .unwrap();
    smol::block_on(async { h.driver().run_until_idle().await }).unwrap();
    assert_eq!(h.expect_text("order.join(',')"), "a,c,b");
}
