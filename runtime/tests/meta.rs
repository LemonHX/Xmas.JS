//! import.meta, from a module on disk all the way to an answer a request can read.
//!
//! The properties are rewritten into the source before the engine sees it, so what these check is
//! both halves: that the value is the right one, and that reaching it did not disturb the code
//! around it.

mod common;

use common::{start_file, tree, Client};

const REQUEST: &str = "GET / HTTP/1.1\r\nHost: t\r\n\r\n";

fn answered_files(files: &[(&str, &str)]) -> String {
    let dir = tree(files);
    let server = start_file(&dir.join("app.ts"));
    let mut client = Client::to(&server);
    client.send(REQUEST);
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}{body}");
    body
}

fn answered(source: &str) -> String {
    answered_files(&[("app.ts", source)])
}

/// A module whose fetch returns whatever the expression evaluates to.
fn app(expression: &str) -> String {
    format!("export default {{ fetch() {{ return new Response(String({expression})); }} }};\n")
}

/// Top-level await used to cost a module its import.meta: the source was parsed as a script,
/// where await is a syntax error, so the tree was unusable and the rewrite was skipped. Every
/// script in eta awaits at the top level, which made this the common case rather than an edge.
#[test]
fn a_module_with_top_level_await_still_knows_its_url() {
    let body = answered(
        "const name = await Promise.resolve('a');\n\
         export default { fetch() { return new Response(import.meta.url + ':' + name); } };\n",
    );
    assert!(body.starts_with("file://"), "{body}");
    assert!(body.ends_with("app.ts:a"), "{body}");
}

#[test]
fn a_module_knows_whether_it_is_the_entry() {
    let source = format!(
        "import {{ fromDep }} from './dep.ts';\n{}",
        app("[import.meta.main, fromDep].join()")
    );
    let body = answered_files(&[
        ("app.ts", &source),
        ("dep.ts", "export const fromDep = import.meta.main;\n"),
    ]);
    // The entry is main; the module it imports, evaluated as part of the same run, is not.
    assert_eq!(body, "true,false");
}

#[test]
fn the_environment_is_readable_as_an_object() {
    // Whatever the process was started with, these two are always present and always agree.
    assert_eq!(
        answered(&app("[typeof import.meta.env, typeof import.meta.env.PROD].join()")),
        "object,boolean"
    );
}

#[test]
fn the_release_flags_follow_node_env() {
    // DEV and PROD partition the same value; MODE is that value. All three come from NODE_ENV, so a
    // realm started without one is a development realm.
    assert_eq!(
        answered(&app(
            "[import.meta.env.DEV, import.meta.env.PROD, import.meta.env.MODE].join()"
        )),
        "true,false,development"
    );
}

#[test]
fn the_environment_has_no_prototype_and_cannot_be_written() {
    // A program that writes to it would otherwise make every other module disagree with it.
    let source = format!(
        "const before = import.meta.env.PROD;\ntry {{ import.meta.env.PROD = !before; }} catch {{}}\n{}",
        app("[import.meta.env.PROD === before, typeof import.meta.env.toString].join()")
    );
    assert_eq!(answered(&source), "true,undefined");
}

#[test]
fn a_property_nothing_implements_stops_the_build() {
    // Silently answering undefined would be a program that runs and does the wrong thing later.
    let dir = tree(&[("app.ts", "export const r = import.meta.require;\n")]);
    let error = xmas_runtime::entry::serve_app("127.0.0.1:0", 1, &dir.join("app.ts"))
        .err()
        .expect("an unimplemented property is a compile error");
    assert!(error.contains("import.meta.require"), "{error}");
}
