//! WebAssembly: the namespace, the three error classes, and compiling bytes.
//!
//! The engine is WNJR; what is under test is the seam. A compiled module has to
//! be a WebAssembly.Module, bad bytes have to throw a CompileError rather than a
//! TypeError, and validate has to answer instead of throwing.

use xmas_runtime::testkit::{text_of, Harness};

fn eval(script: &str) -> String {
    let mut h = Harness::with_modules();
    match h.eval(script) {
        Ok(value) => text_of(&value).unwrap_or_default(),
        Err(error) => panic!("{script}\n{error}"),
    }
}

fn throws(script: &str) -> (String, String) {
    let mut h = Harness::with_modules();
    let error = h.eval_err(script);
    (error.name, error.message)
}

/// The eight bytes of an empty module: the magic, the version, no section.
const EMPTY: &str = "[0x00,0x61,0x73,0x6d,0x01,0x00,0x00,0x00]";

#[test]
fn webassembly_is_a_global_with_the_standard_shape() {
    let answer = eval(
        r#"JSON.stringify([
            typeof WebAssembly,
            typeof WebAssembly.validate,
            typeof WebAssembly.compile,
            typeof WebAssembly.compileStreaming,
            typeof WebAssembly.Module,
            Object.prototype.toString.call(WebAssembly),
            Object.prototype.toString.call(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]))),
        ])"#,
    );
    assert_eq!(
        answer,
        r#"["object","function","function","function","function","[object WebAssembly]","[object WebAssembly.Module]"]"#
    );
}

#[test]
fn the_three_error_classes_are_error_subclasses() {
    let answer = eval(
        r#"JSON.stringify([
            [WebAssembly.CompileError, WebAssembly.LinkError, WebAssembly.RuntimeError]
              .every((C) => C.prototype instanceof Error),
            [WebAssembly.CompileError, WebAssembly.LinkError, WebAssembly.RuntimeError]
              .map((C) => new C('x').name),
            new WebAssembly.LinkError('gone').message,
        ])"#,
    );
    assert_eq!(
        answer,
        r#"[true,["CompileError","LinkError","RuntimeError"],"gone"]"#
    );
}

#[test]
fn validate_says_yes_to_an_empty_module() {
    assert_eq!(
        eval(&format!("String(WebAssembly.validate(new Uint8Array({EMPTY})))")),
        "true"
    );
}

#[test]
fn validate_says_no_rather_than_throwing() {
    // The magic is wrong, which is a compile error, not a TypeError.
    assert_eq!(
        eval("String(WebAssembly.validate(new Uint8Array([1,2,3,4,5,6,7,8])))"),
        "false"
    );
    // A truncated header.
    assert_eq!(
        eval("String(WebAssembly.validate(new Uint8Array([0,97,115])))"),
        "false"
    );
}

#[test]
fn validate_rejects_what_is_not_bytes() {
    let (name, _) = throws("WebAssembly.validate('not bytes')");
    assert_eq!(name, "TypeError");
}

#[test]
fn a_module_compiles_and_reports_its_class() {
    let answer = eval(&format!(
        "(function () {{
            var m = new WebAssembly.Module(new Uint8Array({EMPTY}));
            return JSON.stringify([m instanceof WebAssembly.Module, m[Symbol.toStringTag]]);
        }})()"
    ));
    assert_eq!(answer, r#"[true,"WebAssembly.Module"]"#);
}

#[test]
fn bad_bytes_throw_a_compile_error() {
    let (name, message) = throws("new WebAssembly.Module(new Uint8Array([1,2,3,4,5,6,7,8]))");
    assert_eq!(name, "CompileError");
    assert!(!message.is_empty(), "CompileError carries the engine's reason");
}

/// Settles `expression` (a promise) and returns what it resolved or rejected
/// with, as a string. The harness has no async loop, so the job is drained by
/// handing control back to the driver until the answer is there.
fn settled(expression: &str) -> String {
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "globalThis.__settled = 'pending';
         {expression}.then(
            (value) => {{ globalThis.__settled = 'resolved:' + String(value); }},
            (error) => {{ globalThis.__settled = 'rejected:' + error.name; }}
         );"
    ))
    .unwrap_or_else(|error| panic!("{expression}\n{error}"));
    // The harness has no event loop: run the promise jobs by hand.
    for _ in 0..16 {
        let waiting = h.driver().runtime().is_job_pending().unwrap_or(false);
        if waiting {
            h.driver().runtime().run_pending_job().unwrap();
        }
        let seen = h.expect_text("__settled");
        if seen != "pending" {
            return seen;
        }
        if !waiting {
            break;
        }
    }
    panic!("{expression} never settled");
}

#[test]
fn compile_resolves_with_a_module_and_rejects_with_a_compile_error() {
    let resolved = settled(&format!(
        "WebAssembly.compile(new Uint8Array({EMPTY})).then((m) => m instanceof WebAssembly.Module)"
    ));
    assert_eq!(resolved, "resolved:true");

    let rejected = settled("WebAssembly.compile(new Uint8Array([1,2,3,4,5,6,7,8]))");
    assert_eq!(rejected, "rejected:CompileError");
}

#[test]
fn compile_streaming_checks_the_content_type() {
    let wrong = settled(
        r#"WebAssembly.compileStreaming(new Response('x', { headers: { 'content-type': 'text/plain' } }))"#,
    );
    assert_eq!(
        wrong, "rejected:TypeError",
        "a wrong MIME type is a TypeError, per spec"
    );
}
