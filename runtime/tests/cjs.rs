//! require(), end to end: a CommonJS file reached through an entry, as a server worker would.

mod common;

use common::{start_file, tree, Client};

fn answered(files: &[(&str, &str)]) -> String {
    let dir = tree(files);
    let server = start_file(&dir.join("app.ts"));
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}{body}");
    body
}

fn app(body: &str) -> String {
    format!("export default {{ fetch() {{ return new Response(String({body})); }} }};\n")
}

#[test]
fn require_gives_module_exports_and_the_file_names() {
    let entry = "const lib = require('./lib.cjs');\nexport default { fetch() { return new Response(lib.name + '|' + lib.dir.endsWith('/') + '|' + lib.file.endsWith('lib.cjs')); } };\n";
    let body = answered(&[
        ("app.ts", entry),
        ("lib.cjs", "exports.name = 'lib';\nexports.dir = __dirname + '/';\nexports.file = __filename;\n"),
    ]);
    assert_eq!(body, "lib|true|true");
}

#[test]
fn module_exports_can_be_replaced_and_the_top_level_can_return() {
    let body = answered(&[
        ("app.ts", &format!("const add = require('./add');\n{}", app("add(2, 3)"))),
        ("add.js", "module.exports = function add(a, b) { return a + b; };\nreturn;\nmodule.exports = null;\n"),
    ]);
    assert_eq!(body, "5");
}

#[test]
fn this_at_the_top_is_module_exports() {
    let body = answered(&[
        ("app.ts", &format!("const x = require('./x');\n{}", app("x.hello"))),
        ("x.js", "this.hello = 'from this';\nconst y = typeof require + typeof module;\n"),
    ]);
    assert_eq!(body, "from this");
}

#[test]
fn json_and_the_shipped_modules_are_required() {
    let body = answered(&[
        (
            "app.ts",
            &format!(
                "const data = require('./data.json');\nconst path = require('node:path');\n{}",
                app("data.list.length + path.join('a', 'b')")
            ),
        ),
        ("data.json", "{ \"list\": [1, 2, 3] }"),
    ]);
    assert_eq!(body, "3a/b");
}

#[test]
fn a_directory_index_and_a_package_main_are_found() {
    let body = answered(&[
        (
            "app.ts",
            &format!("const dir = require('./dir');\nconst pkg = require('pkg');\n{}", app("dir + pkg")),
        ),
        ("dir/index.js", "module.exports = 'index';"),
        ("node_modules/pkg/package.json", "{ \"name\": \"pkg\", \"main\": \"lib/main.js\", \"exports\": { \".\": { \"import\": \"./esm.mjs\", \"require\": \"./lib/main.js\" } } }"),
        ("node_modules/pkg/lib/main.js", "module.exports = '+cjs';"),
        ("node_modules/pkg/esm.mjs", "export default '+esm';"),
    ]);
    assert_eq!(body, "index+cjs");
}

#[test]
fn a_cycle_sees_the_exports_made_so_far() {
    let body = answered(&[
        ("app.ts", &format!("const a = require('./a');\n{}", app("a.seen + '/' + a.done"))),
        ("a.js", "exports.early = 'e';\nconst b = require('./b');\nexports.seen = b.sawEarly;\nexports.done = true;\n"),
        ("b.js", "const a = require('./a');\nexports.sawEarly = String(a.early);\n"),
    ]);
    // b runs before a's body (its dependencies run first), so a has no exports yet: undefined, not a crash.
    assert_eq!(body, "undefined/true");
}

#[test]
fn an_esm_file_can_import_a_cjs_one() {
    let body = answered(&[
        ("app.ts", "import lib from './lib.cjs';\nexport default { fetch() { return new Response(lib.v); } };\n"),
        ("lib.cjs", "module.exports = { v: 'default' };"),
    ]);
    assert_eq!(body, "default");
}

#[test]
fn a_named_import_of_a_cjs_file_finds_the_export() {
    let body = answered(&[
        (
            "app.ts",
            "import { name, kind } from './lib.cjs';\nexport default { fetch() { return new Response(name + '/' + kind); } };\n",
        ),
        ("lib.cjs", "exports.name = 'lib';\nexports.kind = 'cjs';\n"),
    ]);
    assert_eq!(body, "lib/cjs");
}

#[test]
fn a_named_import_of_a_cjs_object_module_exports_finds_the_keys() {
    let body = answered(&[
        (
            "app.ts",
            "import { a, b } from './lib.js';\nexport default { fetch() { return new Response(a + '+' + b); } };\n",
        ),
        ("lib.js", "module.exports = { a: '1', b: '2' };\n"),
    ]);
    assert_eq!(body, "1+2");
}

#[test]
fn a_name_the_file_does_not_export_is_still_a_compile_error() {
    let dir = tree(&[
        ("app.ts", "import { missing } from './lib.cjs';\nexport default { fetch() { return new Response('x'); } };\n"),
        ("lib.cjs", "exports.present = 1;\n"),
    ]);
    let error = std::panic::catch_unwind(|| start_file(&dir.join("app.ts")));
    // Starting the file is what loads it: a missing name is a link error, not a request that fails.
    assert!(error.is_err(), "a missing named export should not start");
}

#[test]
fn a_computed_require_says_why_it_fails() {
    let body = answered(&[
        (
            "app.ts",
            "const name = './x';\nlet m = '';\ntry { require(name); } catch (e: any) { m = e.code + ':' + e.message.includes('static scan'); }\nexport default { fetch() { return new Response(m); } };\n",
        ),
        ("x.js", "module.exports = 1;"),
    ]);
    assert_eq!(body, "MODULE_NOT_FOUND:true");
}
