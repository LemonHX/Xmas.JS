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

/// `(module (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))`,
/// which the tests need as a module that exports something callable.
/// One page of memory exported as \`mem\`, with \`store(addr, val)\` and \`load()\`.
const MEMORY: &str = "[0x00,0x61,0x73,0x6d,0x01,0x00,0x00,0x00,0x01,0x0a,0x02,0x60,0x02,0x7f,0x7f,0x00,0x60,0x00,0x01,0x7f,0x03,0x03,0x02,0x00,0x01,0x05,0x04,0x01,0x01,0x01,0x04,0x07,0x16,0x03,0x03,0x6d,0x65,0x6d,0x02,0x00,0x05,0x73,0x74,0x6f,0x72,0x65,0x00,0x00,0x04,0x6c,0x6f,0x61,0x64,0x00,0x01,0x0a,0x13,0x02,0x09,0x00,0x20,0x00,0x20,0x01,0x36,0x02,0x00,0x0b,0x07,0x00,0x41,0x00,0x28,0x02,0x00,0x0b]";

/// `(module (import "env" "f" (func (param i32) (result i32))) (func (export "call") ...))`;
const IMPORTED: &str = "[0x00,0x61,0x73,0x6d,0x01,0x00,0x00,0x00,0x01,0x06,0x01,0x60,0x01,0x7f,0x01,0x7f,0x02,0x09,0x01,0x03,0x65,0x6e,0x76,0x01,0x66,0x00,0x00,0x03,0x02,0x01,0x00,0x07,0x08,0x01,0x04,0x63,0x61,0x6c,0x6c,0x00,0x01,0x0a,0x08,0x01,0x06,0x00,0x20,0x00,0x10,0x00,0x0b]";

const ADD: &str = "[0x00,0x61,0x73,0x6d,0x01,0x00,0x00,0x00,0x01,0x07,0x01,0x60,0x02,0x7f,0x7f,0x01,0x7f,0x03,0x02,0x01,0x00,0x07,0x07,0x01,0x03,0x61,0x64,0x64,0x00,0x00,0x0a,0x09,0x01,0x07,0x00,0x20,0x00,0x20,0x01,0x6a,0x0b]";

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

#[test]
fn an_instance_has_the_class_and_an_exports_object() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({ADD})));
            return JSON.stringify([
                i instanceof WebAssembly.Instance,
                i[Symbol.toStringTag],
                Object.keys(i.exports),
                typeof i.exports.add,
                i.exports.add.name,
            ]);
        }})()"
    ));
    assert_eq!(
        answer,
        r#"[true,"WebAssembly.Instance",["add"],"function","add"]"#
    );
}

#[test]
fn an_exported_function_takes_and_returns_numbers() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({ADD})));
            return String(i.exports.add(2, 40));
        }})()"
    ));
    assert_eq!(answer, "42");
}

#[test]
fn instantiate_resolves_with_an_instance() {
    let resolved = settled(&format!(
        "WebAssembly.instantiate(new Uint8Array({ADD})).then((r) => r.instance instanceof WebAssembly.Instance && r.module instanceof WebAssembly.Module)"
    ));
    assert_eq!(resolved, "resolved:true");
}

#[test]
fn instantiating_bad_bytes_rejects_with_a_compile_error() {
    assert_eq!(
        settled("WebAssembly.instantiate(new Uint8Array([1,2,3,4,5,6,7,8]))"),
        "rejected:CompileError"
    );
}

#[test]
fn new_instance_wants_a_module() {
    let (name, _) = throws("new WebAssembly.Instance({})");
    assert_eq!(name, "TypeError");
}


#[test]
fn an_imported_function_is_called_from_wasm() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(
                new WebAssembly.Module(new Uint8Array({IMPORTED})),
                {{ env: {{ f: (x) => x + 1 }} }},
            );
            return String(i.exports.call(41));
        }})()"
    ));
    assert_eq!(answer, "42");
}

#[test]
fn a_missing_import_is_a_link_error() {
    let (name, message) = throws(&format!(
        "new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({IMPORTED})), {{}})"
    ));
    assert_eq!(name, "LinkError");
    assert!(
        message.contains("env"),
        "the message names what is missing: {message}"
    );
}

#[test]
fn an_import_that_is_not_a_function_is_a_link_error() {
    let (name, _) = throws(&format!(
        "new WebAssembly.Instance(
            new WebAssembly.Module(new Uint8Array({IMPORTED})),
            {{ env: {{ f: 7 }} }},
        )"
    ));
    assert_eq!(name, "LinkError");
}

#[test]
fn a_module_that_imports_nothing_ignores_the_object() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({ADD})), {{ nothing: true }});
            return String(i.exports.add(20, 22));
        }})()"
    ));
    assert_eq!(answer, "42");
}

#[test]
fn a_memory_export_is_one_page_of_bytes() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({MEMORY})));
            return JSON.stringify([
                i.exports.mem instanceof WebAssembly.Memory,
                i.exports.mem[Symbol.toStringTag],
                i.exports.mem.buffer.byteLength,
                i.exports.mem.length,
            ]);
        }})()"
    ));
    assert_eq!(
        answer,
        r#"[true,"WebAssembly.Memory",65536,1]"#
    );
}

#[test]
fn writing_through_the_buffer_is_visible_to_wasm_and_back() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({MEMORY})));
            var bytes = new Uint8Array(i.exports.mem.buffer);
            bytes[0] = 7;
            var fromWasm = i.exports.load();
            i.exports.store(0, 99);
            return String(fromWasm) + ',' + String(bytes[0]);
        }})()"
    ));
    assert_eq!(answer, "7,99");
}

#[test]
fn growing_detaches_the_old_buffer_and_makes_a_longer_one() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({MEMORY})));
            var before = i.exports.mem.buffer;
            var previous = i.exports.mem.grow(2);
            var detached;
            try {{ new Uint8Array(before); detached = false; }}
            catch (e) {{ detached = e instanceof TypeError; }}
            return JSON.stringify([previous, i.exports.mem.length, detached, i.exports.mem.buffer.byteLength]);
        }})()"
    ));
    assert_eq!(answer, r"[1,3,true,196608]");
}

#[test]
fn a_grow_keeps_the_bytes_that_were_written() {
    let answer = eval(&format!(
        "(function () {{
            var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({MEMORY})));
            i.exports.store(0, 1234);
            i.exports.mem.grow(1);
            return String(i.exports.load());
        }})()"
    ));
    assert_eq!(answer, "1234");
}
