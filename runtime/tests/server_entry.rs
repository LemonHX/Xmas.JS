//! Serving a module from disk: the two entry shapes, plus the two ways an entry goes wrong.

mod common;

use std::time::Duration;

use common::{start_file, written, Client};

#[test]
fn an_entry_that_exports_a_default_fetch_is_served() {
    let path = written(
        "app.ts",
        "export default {\n  fetch(request: Request): Response {\n    return new Response('default ' + request.method);\n  },\n};\n",
    );
    let server = start_file(&path);
    let mut client = Client::to(&server);
    client.send("GET /x HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "default GET");
}

#[test]
fn an_entry_that_exports_fetch_itself_is_served() {
    let path = written(
        "app.js",
        "export function fetch(request) { return new Response('named ' + request.url); }\n",
    );
    let server = start_file(&path);
    let mut client = Client::to(&server);
    client.send("GET /named?q=1 HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body, "named http://t/named?q=1");
}

#[test]
fn an_entry_that_calls_serve_keeps_its_own_handler() {
    let path = written(
        "app.ts",
        "Xmas.serve(function (request: Request) {\n  return new Response('from serve');\n});\nexport function fetch() {\n  return new Response('from the export');\n}\n",
    );
    let server = start_file(&path);
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_head, body) = client.response();
    assert_eq!(body, "from serve");
}

#[test]
fn an_entry_with_no_handler_refuses_to_start() {
    // A worker that cannot resolve a handler would answer 500 to everything, so the server does not
    // start instead of pretending it can serve.
    let path = written("app.ts", "export const nothing = 1;\n");
    let error = xmas_runtime::entry::serve_app("127.0.0.1:0", 1, &path)
        .err()
        .expect("an entry with no handler cannot serve");
    assert!(error.contains("no fetch handler"), "{error}");
}

#[test]
fn an_answer_that_is_not_a_response_is_a_500() {
    let path = written(
        "app.ts",
        "export default {\n  fetch(): unknown {\n    return 42;\n  },\n};\n",
    );
    let server = start_file(&path);
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 500"), "{head}");
    assert!(body.contains("not a Response"), "{body}");
}

#[test]
fn an_entry_that_throws_while_it_is_evaluated_says_so() {
    // Module evaluation is a promise, so a top-level throw would otherwise look like an entry that
    // simply has no handler.
    let path = written(
        "app.ts",
        "throw new Error('boom at the top level');
export default {
  fetch(): Response {
    return new Response('x');
  },
};
",
    );
    let error = xmas_runtime::entry::serve_app("127.0.0.1:0", 1, &path)
        .err()
        .expect("the entry throws, so it cannot serve");
    assert!(error.contains("boom at the top level"), "{error}");
}

#[test]
fn a_tsx_entry_is_compiled_before_it_runs() {
    let path = written(
        "app.tsx",
        "type Answer = { text: string };\nconst make = (text: string): Answer => ({ text });\nexport default {\n  fetch(): Response {\n    return new Response(make('compiled').text);\n  },\n};\n",
    );
    let server = start_file(&path);
    let mut client = Client::to(&server);
    client.send("GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_head, body) = client.response();
    assert_eq!(body, "compiled");
    std::thread::sleep(Duration::from_millis(10));
}
