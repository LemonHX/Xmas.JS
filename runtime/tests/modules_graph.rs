//! Import resolution, end to end: an entry that imports its neighbours, its packages, or nothing
//! that exists.

mod common;

use common::{start_file, tree, Client};

/// Serve `app.ts` written among `files` and answer one request to it.
fn answered(files: &[(&str, &str)]) -> String {
    answered_at("app.ts", files)
}

fn answered_at(entry: &str, files: &[(&str, &str)]) -> String {
    let dir = tree(files);
    let server = start_file(&dir.join(entry));
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}{body}");
    body
}

/// Why `app.ts` written among `files` refuses to start.
fn refused(files: &[(&str, &str)]) -> String {
    let dir = tree(files);
    xmas_runtime::entry::serve_app("127.0.0.1:0", 1, &dir.join("app.ts"))
        .err()
        .expect("the entry cannot start")
}

#[test]
fn a_relative_import_with_its_extension_is_found() {
    let body = answered(&[
        (
            "app.ts",
            "import { greet } from './lib/greet.ts';\nexport default {\n  fetch(): Response {\n    return new Response(greet('world'));\n  },\n};\n",
        ),
        (
            "lib/greet.ts",
            "export function greet(who: string): string {\n  return 'hello ' + who;\n}\n",
        ),
    ]);
    assert_eq!(body, "hello world");
}

#[test]
fn a_specifier_without_an_extension_is_probed_with_ts_first() {
    let body = answered(&[
        (
            "app.ts",
            "import { answer } from './lib/answer';\nexport default {\n  fetch(): Response {\n    return new Response(String(answer));\n  },\n};\n",
        ),
        ("lib/answer.js", "export const answer = 'the js one';\n"),
        ("lib/answer.ts", "export const answer: number = 42;\n"),
    ]);
    assert_eq!(body, "42");
}

#[test]
fn a_nested_import_is_resolved_against_the_file_that_wrote_it() {
    let body = answered(&[
        (
            "app.ts",
            "import { one } from './lib/one.ts';\nexport default {\n  fetch(): Response {\n    return new Response(one());\n  },\n};\n",
        ),
        (
            "lib/one.ts",
            "import { two } from '../shared/two';\nexport function one(): string {\n  return 'one+' + two();\n}\n",
        ),
        ("shared/two.ts", "export function two(): string {\n  return 'two';\n}\n"),
    ]);
    assert_eq!(body, "one+two");
}

#[test]
fn a_js_specifier_finds_the_ts_source_it_was_written_as() {
    // What a project written in types spells when it wants the compiled name.
    let body = answered(&[
        (
            "app.ts",
            "import { name } from './lib/name.js';\nexport default {\n  fetch(): Response {\n    return new Response(name);\n  },\n};\n",
        ),
        ("lib/name.ts", "export const name: string = 'from ts';\n"),
    ]);
    assert_eq!(body, "from ts");
}

#[test]
fn a_bare_import_comes_from_node_modules() {
    let body = answered(&[
        (
            "app.ts",
            "import { shout } from 'shout';\nexport default {\n  fetch(): Response {\n    return new Response(shout());\n  },\n};\n",
        ),
        ("node_modules/shout/package.json", "{\"name\":\"shout\",\"main\":\"lib/main.js\"}\n"),
        ("node_modules/shout/lib/main.js", "export function shout() { return 'from a package'; }\n"),
    ]);
    assert_eq!(body, "from a package");
}

#[test]
fn a_bare_import_comes_from_node_modules_further_up() {
    let body = answered_at(
        "src/deep/app.ts",
        &[
        (
            "src/deep/app.ts",
            "import { shout } from 'shout';\nexport default {\n  fetch(): Response {\n    return new Response(shout());\n  },\n};\n",
        ),
        ("node_modules/shout/index.ts", "export function shout(): string { return 'up here'; }\n"),
        ],
    );
    assert_eq!(body, "up here");
}

#[test]
fn a_package_with_exports_is_read_through_them() {
    let body = answered(&[
        (
            "app.ts",
            "import { sub } from 'pkg/sub';\nexport default {\n  fetch(): Response {\n    return new Response(sub);\n  },\n};\n",
        ),
        (
            "node_modules/pkg/package.json",
            "{\"name\":\"pkg\",\"exports\":{\".\":{\"import\":\"./main.js\"},\"./sub\":{\"import\":\"./src/sub.js\"}}}\n",
        ),
        ("node_modules/pkg/main.js", "export const root = 'root';\n"),
        ("node_modules/pkg/src/sub.js", "export const sub = 'from exports';\n"),
    ]);
    assert_eq!(body, "from exports");
}

#[test]
fn a_missing_relative_import_says_which_file_asked() {
    let error = refused(&[
        (
            "app.ts",
            "import './nope.ts';\nexport default {\n  fetch(): Response {\n    return new Response('x');\n  },\n};\n",
        ),
    ]);
    assert!(error.contains("cannot find './nope.ts'"), "{error}");
    assert!(error.contains("app.ts"), "{error}");
}

#[test]
fn a_missing_package_says_so() {
    let error = refused(&[
        (
            "app.ts",
            "import 'nowhere';\nexport default {\n  fetch(): Response {\n    return new Response('x');\n  },\n};\n",
        ),
    ]);
    assert!(error.contains("cannot find package 'nowhere'"), "{error}");
}

#[test]
fn a_module_this_runtime_ships_is_imported_by_its_node_name() {
    // The modules answer to both spellings from the table, so resolution never reaches the disk.
    let body = answered(&[(
        "app.ts",
        "import { basename } from 'node:path';\nexport default {\n  fetch(): Response {\n    return new Response(basename('/a/b/c.txt'));\n  },\n};\n",
    )]);
    assert_eq!(body, "c.txt");
}

#[test]
fn a_node_module_this_runtime_does_not_ship_says_so() {
    // The binding has to be used: an unused import is dropped before anything resolves it, so an
    // import nobody reads says nothing about whether it could be found.
    let error = refused(&[(
        "app.ts",
        "import http from 'node:http';\nexport default {\n  fetch(): Response {\n    return new Response(String(http));\n  },\n};\n",
    )]);
    assert!(error.contains("'node:http' is not a module this runtime ships"), "{error}");
}

#[test]
fn an_import_for_its_side_effect_alone_still_runs() {
    let body = answered(&[
        (
            "app.ts",
            "import './setup.ts';\nexport default {\n  fetch(): Response {\n    return new Response(String((globalThis as any).ready));\n  },\n};\n",
        ),
        ("setup.ts", "(globalThis as any).ready = 'set up first';\n"),
    ]);
    assert_eq!(body, "set up first");
}
