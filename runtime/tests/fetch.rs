//! fetch against a canned server on localhost: the transport, the body stream, encodings,
//! redirects and aborts, with no network and no test server crate.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

use xmas_runtime::testkit::Harness;

/// Serve one HTTP/1.1 connection with this exact response, then close.
fn canned(response: &'static str) -> (u16, JoinHandle<String>) {
    serve(response.as_bytes().to_vec())
}

/// The same, for a response built while the test runs (a redirect needs the next port).
fn serve(bytes: Vec<u8>) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let request = read_request(&mut socket);
        socket.write_all(&bytes).expect("write");
        socket.flush().expect("flush");
        request
    });
    (port, handle)
}

/// Read request headers and any body the client announced.
fn read_request(socket: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match socket.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(end) = find(&buffer, b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buffer[..end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if buffer.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buffer).to_string()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn idle(h: &mut Harness) {
    for _ in 0..50 {
        h.driver().run_until_idle_blocking().expect("pump");
    }
}

#[test]
fn fetch_reads_status_headers_and_text() {
    let (port, _handle) = canned(
        "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nX-Thing: one\r\nX-Thing: two\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    );
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/hello');
            globalThis.status = response.status;
            globalThis.statusText = response.statusText;
            globalThis.ok = response.ok;
            globalThis.thing = response.headers.get('x-thing');
            globalThis.text = await response.text();
            globalThis.used = response.bodyUsed;
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(status)"), "201");
    assert_eq!(h.expect_text("String(statusText)"), "Created");
    assert_eq!(h.expect_text("String(ok)"), "true");
    assert_eq!(h.expect_text("String(text)"), "hello");
    assert_eq!(h.expect_text("String(used)"), "true");
    assert_eq!(h.expect_text("String(thing)"), "one, two");
}

#[test]
fn fetch_posts_a_body() {
    let (port, handle) =
        canned("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/post', {{
                method: 'POST',
                headers: {{ 'content-type': 'text/plain' }},
                body: 'ping',
            }});
            globalThis.text = await response.text();
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(text)"), "ok");
    let request = handle.join().expect("server");
    assert!(request.starts_with("POST /post HTTP/1.1"), "{request}");
    assert!(
        request.to_lowercase().contains("content-type: text/plain"),
        "{request}"
    );
    assert!(request.ends_with("ping"), "{request}");
}

#[test]
fn fetch_decodes_gzip() {
    let mut gzipped = vec![
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
        0x57, 0xc8, 0x40, 0x90, 0x00, 0x80, 0x88, 0xf9, 0xe5, 0x11, 0x00, 0x00, 0x00,
    ];
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 28\r\nConnection: close\r\n\r\n"
            .to_vec();
    response.append(&mut gzipped);
    let (port, _handle) = serve(response);
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/gzip');
            globalThis.text = await response.text();
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(text)"), "hello hello hello");
}

#[test]
fn fetch_streams_a_chunked_body() {
    let (port, _handle) = canned(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nfirst\r\n6\r\nsecond\r\n0\r\n\r\n",
    );
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/chunks');
            const reader = response.body.getReader();
            const seen = [];
            for (;;) {{
                const {{ value, done }} = await reader.read();
                if (done) break;
                seen.push(new Uint8Array(value).length);
            }}
            globalThis.pieces = seen.length;
            globalThis.text = await new Response('x').text();
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert!(h.expect_number("pieces") >= 1.0);
    assert_eq!(h.expect_text("String(text)"), "x");
}

#[test]
fn fetch_follows_a_redirect_by_default() {
    let (port, _handle) =
        canned("HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nlanded");
    let target = format!("http://127.0.0.1:{port}/landed");
    let (first, _first_handle) = serve(
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes(),
    );
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{first}/start');
            globalThis.text = await response.text();
            globalThis.redirected = response.redirected;
            globalThis.url = response.url;
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(text)"), "landed");
    assert_eq!(h.expect_text("String(redirected)"), "true");
    assert_eq!(h.expect_text("String(url)"), target);
}

#[test]
fn fetch_with_a_manual_redirect_returns_the_302() {
    let (port, handle) = canned(
        "HTTP/1.1 302 Found\r\nLocation: http://example.com/next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/start', {{ redirect: 'manual' }});
            globalThis.status = response.status;
            globalThis.location = response.headers.get('location');
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(status)"), "302");
    assert_eq!(h.expect_text("String(location)"), "http://example.com/next");
    handle.join().expect("server");
}

#[test]
fn fetch_aborts_a_request_that_is_still_waiting() {
    // A server that accepts and never answers.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut buffer = [0u8; 1024];
        let _ = socket.read(&mut buffer);
        std::thread::sleep(std::time::Duration::from_millis(500));
    });

    let mut h = Harness::with_modules();
    h.eval(&format!(
        "globalThis.outcome = 'waiting';
         (async () => {{
            const controller = new AbortController();
            setTimeout(() => controller.abort(), 20);
            try {{
                await fetch('http://127.0.0.1:{port}/slow', {{ signal: controller.signal }});
                globalThis.outcome = 'no error';
            }} catch (error) {{
                globalThis.outcome = error.name;
            }}
         }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(outcome)"), "AbortError");
    server.join().expect("server");
}

#[test]
fn fetch_reports_a_connection_refused() {
    // Bind and close, so nothing is listening on that port.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            try {{
                await fetch('http://127.0.0.1:{port}/nothing');
                globalThis.outcome = 'no error';
            }} catch (error) {{
                globalThis.outcome = error.name;
            }}
         }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(outcome)"), "TypeError");
}

#[test]
fn a_bodyless_status_has_no_body() {
    let (port, _handle) = canned("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const response = await fetch('http://127.0.0.1:{port}/empty');
            globalThis.status = response.status;
            globalThis.body = String(response.body);
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(status)"), "204");
    assert_eq!(h.expect_text("String(body)"), "null");
}

#[test]
fn headers_fold_case_and_iterate_in_order() {
    let mut h = Harness::with_modules();
    h.eval(
        "const headers = new Headers({ 'X-Test': 'b' });
         headers.append('a-header', 'one');
         headers.append('A-Header', 'two');
         headers.set('x-test', 'c');
         globalThis.value = headers.get('a-HEADER');
         globalThis.names = Array.from(headers.keys()).join(',');
         globalThis.count = headers.has('X-Test');
         headers.delete('x-test');
         globalThis.after = String(headers.get('x-test'));
         globalThis.left = Array.from(headers.keys()).join(',');",
    )
    .unwrap();
    assert_eq!(h.expect_text("String(value)"), "one, two");
    assert_eq!(h.expect_text("String(names)"), "a-header,x-test");
    assert_eq!(h.expect_text("String(count)"), "true");
    assert_eq!(h.expect_text("String(after)"), "null");
    assert_eq!(h.expect_text("String(left)"), "a-header");
}

#[test]
fn headers_reject_broken_names_and_values() {
    let mut h = Harness::with_modules();
    h.eval(
        "const headers = new Headers();
         const thrown = (run) => { try { run(); return 'no error'; } catch (error) { return error.name; } };
         globalThis.badName = thrown(() => headers.append('bad name', 'x'));
         globalThis.badValue = thrown(() => headers.append('ok', 'line' + String.fromCharCode(10) + 'break'));",
    )
    .unwrap();
    assert_eq!(h.expect_text("String(badName)"), "TypeError");
    assert_eq!(h.expect_text("String(badValue)"), "TypeError");
}

#[test]
fn response_constructors_behave() {
    let mut h = Harness::with_modules();
    h.eval(
        "(async () => {
            const json = Response.json({ a: 1 });
            globalThis.type = json.headers.get('content-type');
            globalThis.parsed = (await json.json()).a;
            const redirect = Response.redirect('http://example.com/', 301);
            globalThis.redirectStatus = redirect.status;
            globalThis.location = redirect.headers.get('location');
            const copy = new Response('hello');
            const branch = copy.clone();
            globalThis.both = (await copy.text()) + '/' + (await branch.text());
            globalThis.secondRead = await copy.text().then(() => 'no error', (error) => error.name);
        })();",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(type)"), "application/json");
    assert_eq!(h.expect_text("String(parsed)"), "1");
    assert_eq!(h.expect_text("String(redirectStatus)"), "301");
    assert_eq!(h.expect_text("String(location)"), "http://example.com/");
    assert_eq!(h.expect_text("String(both)"), "hello/hello");
    assert_eq!(h.expect_text("String(secondRead)"), "TypeError");
}

#[test]
fn fetch_sends_form_data_as_multipart() {
    let (port, handle) =
        canned("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    let mut h = Harness::with_modules();
    h.eval(&format!(
        "(async () => {{
            const form = new FormData();
            form.append('name', 'xmas');
            form.append('loud', 'yes');
            const response = await fetch('http://127.0.0.1:{port}/form', {{ method: 'POST', body: form }});
            globalThis.text = await response.text();
        }})();"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(text)"), "ok");
    let request = handle.join().expect("server");
    assert!(
        request
            .to_lowercase()
            .contains("content-type: multipart/form-data; boundary="),
        "{request}"
    );
    assert!(request.contains("name=\"name\""), "{request}");
    assert!(request.contains("xmas"), "{request}");
}

#[test]
fn fetch_is_a_module_and_a_global() {
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "test-fetch",
            "import { Response, Headers } from 'node:fetch';
             export const same = Response === globalThis.Response && Headers === globalThis.Headers;
             export const ok = typeof fetch === 'function';",
        )
        .unwrap();
    let same = h.property(&namespace, "same").unwrap();
    assert_eq!(format!("{same:?}").contains("true"), true);
}
