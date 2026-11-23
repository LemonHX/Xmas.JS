//! Remote imports, end to end: what a URL specifier turns into.
//!
//! Nothing here needs a socket. A data URL goes through the same fetch path a real one does --
//! the branch that differs is only where the bytes come from -- so these cover the loader end of
//! docs/remote.md without making the suite depend on unpkg being up.

mod common;

use common::{start_file, tree, Client};

fn answered(files: &[(&str, &str)]) -> String {
    let dir = tree(files);
    let server = start_file(&dir.join("app.ts"));
    let mut client = Client::to(&server);
    client.send(REQUEST);
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}{body}");
    body
}

const REQUEST: &str = "GET / HTTP/1.1\r\nHost: t\r\n\r\n";

fn app(expression: &str) -> String {
    format!("export default {{ fetch() {{ return new Response(String({expression})); }} }};\n")
}

#[test]
fn a_data_url_is_a_module() {
    let entry = format!(
        "import {{ hi }} from 'data:text/javascript,export const hi = () => 42';\n{}",
        app("hi()")
    );
    assert_eq!(answered(&[("app.ts", &entry)]), "42");
}

#[test]
fn a_data_url_with_base64_and_a_media_type() {
    // 'export const x = 7' in base64. The media type carries a charset parameter, which is a
    // parameter rather than part of the type.
    let entry = format!(
        "import {{ x }} from 'data:text/javascript;charset=utf-8;base64,ZXhwb3J0IGNvbnN0IHggPSA3';\n{}",
        app("x * 6")
    );
    assert_eq!(answered(&[("app.ts", &entry)]), "42");
}

#[test]
fn a_data_url_can_be_imported_by_another() {
    let entry = format!(
        "import {{ two }} from 'data:text/javascript,export const two = 2';\n{}",
        app("two * 21")
    );
    assert_eq!(answered(&[("app.ts", &entry)]), "42");
}

#[test]
fn a_data_url_says_what_it_is() {
    let fetched = xmas_runtime::remote::body("data:text/javascript,export const x = 1").unwrap();
    assert_eq!(fetched.media_type, "text/javascript");
    assert_eq!(
        String::from_utf8(fetched.bytes).unwrap(),
        "export const x = 1"
    );
}

#[test]
fn a_failure_is_not_cached() {
    // The runtime must not refuse a URL that starts working, so nothing about a failed fetch is
    // remembered. A closed port on loopback fails without needing the internet to be reachable.
    let first = xmas_runtime::remote::body("https://127.0.0.1:1/nothing.js");
    assert!(first.is_err(), "a refused connection should fail");
    let reason = first.unwrap_err();
    assert!(reason.contains("127.0.0.1:1"), "{reason}");

    // Still a failure the second time: nothing was remembered to make it look otherwise.
    assert!(xmas_runtime::remote::body("https://127.0.0.1:1/nothing.js").is_err());
}

#[test]
fn a_data_url_is_not_written_to_the_cache() {
    // A data URL has no network and no identity worth caching: whatever the working directory is,
    // the call leaves no file behind.
    let before = std::fs::read_dir(".xmas/remote").map(|d| d.count()).unwrap_or(0);
    xmas_runtime::remote::body("data:text/javascript,export const y = 2").unwrap();
    let after = std::fs::read_dir(".xmas/remote").map(|d| d.count()).unwrap_or(0);
    assert_eq!(before, after);
}
