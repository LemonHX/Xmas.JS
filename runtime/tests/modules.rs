//! console and timers, carried over from the probe.
//! Each case mirrors behaviour the old modules/ implementation had.

use xmas_runtime::testkit::{Harness, number_of, text_of};

fn idle(h: &mut Harness) {
    smol::block_on(async { h.driver().run_until_idle().await }).unwrap();
}

#[test]
fn console_writes_to_the_host_sink() {
    let mut h = Harness::with_modules();
    h.eval("console.log('a', 1, { b: 2 }); console.warn('w'); console.assert(false, 'boom');")
        .unwrap();
    let out = h.host().console.take();
    assert!(out.contains("[log] a 1 {\"b\":2}"), "{out}");
    assert!(out.contains("[warn] w"), "{out}");
    assert!(out.contains("[error] Assertion failed: boom"), "{out}");
}

#[test]
fn console_time_and_time_end() {
    let mut h = Harness::with_modules();
    h.eval("console.time('t'); console.timeEnd('t');").unwrap();
    let out = h.host().console.take();
    assert!(out.contains("t: "), "{out}");
    assert!(out.contains("ms"), "{out}");
}

#[test]
fn set_timeout_orders_by_deadline_and_passes_args() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout((x, y) => order.push(x + y), 3, 'a', 'b');
         setTimeout(() => order.push('first'), 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "first,ab");
}

#[test]
fn set_immediate_runs_before_timeout_zero() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout(() => order.push('timeout'), 0);
         setImmediate(() => order.push('immediate'));",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "immediate,timeout");
}

#[test]
fn clear_timeout_cancels() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.fired = 0;
         const handle = setTimeout(() => { fired++; }, 0);
         clearTimeout(handle);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("fired"), 0.0);
}

#[test]
fn set_interval_repeats_until_cleared() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.n = 0;
         const handle = setInterval(() => { n++; if (n === 3) clearInterval(handle); }, 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("n"), 3.0);
}

#[test]
fn timer_callback_throwing_does_not_kill_the_loop() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.after = 0;
         setTimeout(() => { throw new Error('boom'); }, 0);
         setTimeout(() => { after = 1; }, 1);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("after"), 1.0);
    let out = h.host().console.take();
    assert!(out.contains("timer callback threw"), "{out}");
}

#[test]
fn timers_module_by_name() {
    // The shape of test_timers in the old modules/src/timers.rs: import { setTimeout }, await.
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "timers_test.mjs",
            "import { setTimeout } from 'timers';
             export async function test() {
               return new Promise((resolve) => setTimeout(() => resolve('timeout'), 2));
             }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(text_of(&settled), Some("timeout".to_string()));
}

#[test]
fn timers_promises_module_by_name() {
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "timers_promises_test.mjs",
            "import { setTimeout } from 'timers/promises';
             export async function test() { return await setTimeout(2, 'later'); }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(text_of(&settled), Some("later".to_string()));
}

#[test]
fn queue_microtask_runs_before_timers() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.order = [];
         setTimeout(() => order.push('timer'), 0);
         queueMicrotask(() => order.push('micro'));",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "micro,timer");
}

#[test]
fn timer_handle_is_a_number() {
    let mut h = Harness::with_modules();
    let handle = h.eval("setTimeout(() => {}, 1000)").unwrap();
    assert!(number_of(&handle).is_some(), "the handle must be a number: {handle:?}");
    h.eval("clearTimeout(0)").unwrap();
}

#[test]
fn event_target_add_remove_once() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.seen = [];
         const target = new EventTarget();
         const onPing = (event) => seen.push('ping:' + event.type);
         target.addEventListener('ping', onPing);
         target.addEventListener('once', () => seen.push('once'), { once: true });
         target.dispatchEvent(new Event('ping'));
         target.removeEventListener('ping', onPing);
         target.dispatchEvent(new Event('ping'));
         target.dispatchEvent(new Event('once'));
         target.dispatchEvent(new Event('once'));",
    )
    .unwrap();
    assert_eq!(h.expect_text("seen.join(',')"), "ping:ping,once");
}

#[test]
fn event_prevent_default_and_immediate_stop() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.log = [];
         const target = new EventTarget();
         target.addEventListener('go', (event) => { log.push('a'); event.preventDefault(); event.stopImmediatePropagation(); });
         target.addEventListener('go', () => log.push('b'));
         const event = new Event('go', { cancelable: true });
         globalThis.notCancelled = target.dispatchEvent(event);
         globalThis.defaultPrevented = event.defaultPrevented;",
    )
    .unwrap();
    assert_eq!(h.expect_text("log.join(',')"), "a");
    assert_eq!(h.expect_number("notCancelled ? 1 : 0"), 0.0);
    assert_eq!(h.expect_number("defaultPrevented ? 1 : 0"), 1.0);
}

#[test]
fn custom_event_carries_detail() {
    let mut h = Harness::with_modules();
    h.eval(
        "const target = new EventTarget();
         globalThis.detail = null;
         target.addEventListener('custom', (event) => { globalThis.detail = event.detail; });
         target.dispatchEvent(new CustomEvent('custom', { detail: { n: 7 } }));",
    )
    .unwrap();
    assert_eq!(h.expect_number("detail.n"), 7.0);
}

#[test]
fn event_emitter_round_trip() {
    let mut h = Harness::with_modules();
    h.eval(
        "const { EventEmitter } = __events_module;
         const emitter = new EventEmitter();
         globalThis.trace = [];
         emitter.on('data', (a, b) => trace.push('on:' + a + b));
         emitter.once('data', () => trace.push('once'));
         emitter.emit('data', 1, 2);
         emitter.emit('data', 3, 4);
         globalThis.count = emitter.listenerCount('data');",
    )
    .unwrap();
    assert_eq!(h.expect_text("trace.join(',')"), "on:12,once,on:34");
    assert_eq!(h.expect_number("count"), 1.0);
}

#[test]
fn event_emitter_error_without_listener_throws() {
    let mut h = Harness::with_modules();
    let error = h
        .eval_err("const { EventEmitter } = __events_module; new EventEmitter().emit('error', new Error('boom'));");
    assert!(error.text.contains("boom"), "{error:?}");
}

#[test]
fn abort_controller_fires_abort_event() {
    let mut h = Harness::with_modules();
    h.eval(
        "const controller = new AbortController();
         globalThis.order = [];
         controller.signal.addEventListener('abort', () => order.push('listener'));
         controller.signal.onabort = () => order.push('onabort');
         globalThis.before = controller.signal.aborted;
         controller.abort();
         globalThis.after = controller.signal.aborted;
         globalThis.reasonName = controller.signal.reason.name;",
    )
    .unwrap();
    assert_eq!(h.expect_number("before ? 1 : 0"), 0.0);
    assert_eq!(h.expect_number("after ? 1 : 0"), 1.0);
    assert_eq!(h.expect_text("order.join(',')"), "listener,onabort");
    assert_eq!(h.expect_text("reasonName"), "AbortError");
}

#[test]
fn abort_signal_throw_if_aborted() {
    let mut h = Harness::with_modules();
    let error = h
        .eval_err("AbortSignal.abort(new Error('nope')).throwIfAborted();");
    assert!(error.text.contains("nope"), "{error:?}");
}

#[test]
fn abort_signal_any_and_timeout() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.which = null;
         const first = new AbortController();
         const second = new AbortController();
         const both = AbortSignal.any([first.signal, second.signal]);
         both.addEventListener('abort', () => { which = 'both'; });
         second.abort();
         globalThis.timeoutFired = null;
         const timed = AbortSignal.timeout(2);
         timed.addEventListener('abort', () => { timeoutFired = timed.reason.name; });",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("which"), "both");
    assert_eq!(h.expect_text("timeoutFired"), "AbortError");
}

fn url_parses_absolute_and_relative() {
    let mut h = Harness::with_modules();
    h.eval(
        "const url = new URL('https://user:pw@example.com:8443/a/b?x=1&y=2#frag');
         globalThis.parts = [url.protocol, url.username, url.password, url.hostname, url.port,
                             url.pathname, url.search, url.hash, url.origin].join('|');
         globalThis.relative = new URL('../c?z=3', 'https://example.com/a/b/').href;
         globalThis.bad = URL.canParse('not a url');",
    )
    .unwrap();
    assert_eq!(
        h.expect_text("parts"),
        "https:|user|pw|example.com|8443|/a/b|?x=1&y=2|#frag|https://example.com:8443"
    );
    assert_eq!(h.expect_text("relative"), "https://example.com/a/c?z=3");
    assert_eq!(h.expect_number("bad ? 1 : 0"), 0.0);
}

#[test]
fn url_setters_rewrite_href() {
    let mut h = Harness::with_modules();
    h.eval(
        "const url = new URL('https://example.com:8443/a?x=1#frag');
         url.hash = '';
         url.search = '?y=2';
         globalThis.step1 = url.href;
         url.port = '';
         url.pathname = '/z';
         globalThis.step2 = url.href;
         url.protocol = 'http:';
         globalThis.step3 = url.href;",
    )
    .unwrap();
    assert_eq!(h.expect_text("step1"), "https://example.com:8443/a?y=2");
    assert_eq!(h.expect_text("step2"), "https://example.com/z?y=2");
    assert_eq!(h.expect_text("step3"), "http://example.com/z?y=2");
}

#[test]
fn url_search_params_reads_and_writes_back() {
    let mut h = Harness::with_modules();
    h.eval(
        "const url = new URL('https://example.com/?a=1&b=2&a=3');
         globalThis.initial = [url.searchParams.get('a'), url.searchParams.getAll('a').join(','), url.searchParams.size].join('|');
         url.searchParams.append('c', '4 5');
         globalThis.afterAppend = url.search;
         url.searchParams.delete('b');
         url.searchParams.sort();
         globalThis.afterSort = url.search;
         globalThis.text = url.searchParams.toString();",
    )
    .unwrap();
    assert_eq!(h.expect_text("initial"), "1|1,3|3");
    assert_eq!(h.expect_text("afterAppend"), "?a=1&b=2&a=3&c=4+5");
    assert_eq!(h.expect_text("afterSort"), "?a=1&a=3&c=4+5");
    assert_eq!(h.expect_text("text"), "a=1&a=3&c=4+5");
}

#[test]
fn url_search_params_iterates_and_accepts_inputs() {
    let mut h = Harness::with_modules();
    h.eval(
        "const fromText = new URLSearchParams('a=1&b=two+words');
         globalThis.fromString = [...fromText].map((pair) => pair.join('=')).join(',');
         globalThis.fromObject = new URLSearchParams({ k: 'v' }).toString();
         globalThis.keys = [...fromText.keys()].join(',');",
    )
    .unwrap();
    assert_eq!(h.expect_text("fromString"), "a=1,b=two words");
    assert_eq!(h.expect_text("fromObject"), "k=v");
    assert_eq!(h.expect_text("keys"), "a,b");
}

#[test]
fn url_module_by_name() {
    let mut h = Harness::with_modules();
    let namespace = h
        .module(
            "url_test.mjs",
            "import { URL } from 'url';
             export function test() { return new URL('/x', 'https://example.com/base/').href; }",
        )
        .unwrap();
    let value = h.call(&namespace, "test", &[]).unwrap();
    assert_eq!(text_of(&value), Some("https://example.com/x".to_string()));
}

#[test]
fn buffer_string_round_trip_in_encodings() {
    let mut h = Harness::with_modules();
    h.eval(
        "const utf8 = Buffer.from('\\u4e2d\\u6587 ok');
         globalThis.hex = Buffer.from('hi').toString('hex');
         globalThis.back = utf8.toString('utf8');
         globalThis.size = utf8.length;
         globalThis.b64 = Buffer.from('hi').toString('base64');
         globalThis.fromB64 = Buffer.from('aGk=', 'base64').toString('utf8');
         globalThis.latin = Buffer.from('ff', 'hex').toString('latin1').charCodeAt(0);",
    )
    .unwrap();
    assert_eq!(h.expect_text("hex"), "6869");
    assert_eq!(h.expect_text("back"), "\u{4e2d}\u{6587} ok");
    assert_eq!(h.expect_number("size"), 9.0);
    assert_eq!(h.expect_text("b64"), "aGk=");
    assert_eq!(h.expect_text("fromB64"), "hi");
    assert_eq!(h.expect_number("latin"), 255.0);
}

#[test]
fn buffer_is_a_uint8_array() {
    let mut h = Harness::with_modules();
    h.eval(
        "const buf = Buffer.alloc(4, 7);
         globalThis.isView = buf instanceof Uint8Array;
         globalThis.sum = Array.from(buf).reduce((a, b) => a + b, 0);
         globalThis.sliceCtor = buf.subarray(1, 3).constructor.name;
         globalThis.sliceIsBuffer = Buffer.isBuffer(buf.subarray(1, 3));
         globalThis.concat = Buffer.concat([Buffer.from('ab'), Buffer.from('cd')]).toString();
         globalThis.byteLength = Buffer.byteLength('abc');",
    )
    .unwrap();
    assert_eq!(h.expect_number("isView ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("sum"), 28.0);
    assert_eq!(h.expect_text("sliceCtor"), "Buffer");
    assert_eq!(h.expect_number("sliceIsBuffer ? 1 : 0"), 1.0);
    assert_eq!(h.expect_text("concat"), "abcd");
    assert_eq!(h.expect_number("byteLength"), 3.0);
}

#[test]
fn atob_and_btoa_round_trip() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.encoded = btoa('hello');
         globalThis.decoded = atob(encoded);",
    )
    .unwrap();
    assert_eq!(h.expect_text("encoded"), "aGVsbG8=");
    assert_eq!(h.expect_text("decoded"), "hello");
}

#[test]
fn blob_reads_back_as_text_and_bytes() {
    let mut h = Harness::with_modules();
    h.eval(
        "const blob = new Blob(['ab', Buffer.from('cd')], { type: 'text/plain' });
         globalThis.size = blob.size;
         globalThis.type = blob.type;
         globalThis.promise = blob.text();
         globalThis.position = 0;
         promise.then((text) => { globalThis.body = text; globalThis.position = 1; });",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("size"), 4.0);
    assert_eq!(h.expect_text("type"), "text/plain");
    assert_eq!(h.expect_text("body"), "abcd");
}

#[test]
fn blob_slice_keeps_type() {
    let mut h = Harness::with_modules();
    h.eval(
        "const blob = new Blob(['abcdef'], { type: 'text/plain' });
         const cut = blob.slice(1, 4);
         globalThis.cutSize = cut.size;
         globalThis.cutType = cut.type;
         globalThis.cutPromise = cut.text();
         cutPromise.then((text) => { globalThis.cutBody = text; });",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("cutSize"), 3.0);
    assert_eq!(h.expect_text("cutType"), "text/plain");
    assert_eq!(h.expect_text("cutBody"), "bcd");
}

#[test]
fn fs_write_read_round_trip() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-roundtrip");
    let file = format!("{dir}/note.txt");
    h.eval(&format!(
        "globalThis.done = null;
         fs.writeFile({file:?}, 'hello fs')
           .then(() => fs.readFile({file:?}, 'utf8'))
           .then((text) => {{ globalThis.text = text; globalThis.done = true; }})
           .catch((error) => {{ globalThis.failure = String(error); globalThis.done = false; }});"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("done ? 1 : 0"), 1.0, "{:?}", h.eval("String(globalThis.failure)").ok());
    assert_eq!(h.expect_text("text"), "hello fs");
}

#[test]
fn fs_read_without_encoding_gives_buffer() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-buffer");
    let file = format!("{dir}/raw.bin");
    h.eval(&format!(
        "globalThis.step = 0;
         fs.writeFile({file:?}, Buffer.from('abcd'))
           .then(() => fs.readFile({file:?}))
           .then((buf) => {{ globalThis.isBuffer = Buffer.isBuffer(buf); globalThis.hex = buf.toString('hex'); globalThis.step = 1; }});"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("step"), 1.0);
    assert_eq!(h.expect_number("isBuffer ? 1 : 0"), 1.0);
    assert_eq!(h.expect_text("hex"), "61626364");
}

#[test]
fn fs_append_and_stat() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-append");
    let file = format!("{dir}/log.txt");
    h.eval(&format!(
        "globalThis.step = 0;
         globalThis.failure = null;
         fs.writeFile({file:?}, 'one')
           .then(() => fs.appendFile({file:?}, '+two'))
           .then(() => fs.stat({file:?}))
           .then((info) => {{
             globalThis.size = info.size;
             globalThis.isFile = info.isFile();
             globalThis.isDir = info.isDirectory();
             return fs.readFile({file:?}, 'utf8');
           }})
           .then((text) => {{ globalThis.text = text; globalThis.step = 1; }})
           .catch((error) => {{ globalThis.failure = String(error && error.stack || error); }});"
    ))
    .unwrap();
    idle(&mut h);
    let failure = h.expect_text("String(globalThis.failure || '')");
    assert_eq!(h.expect_number("step"), 1.0, "failure={failure}");
    assert_eq!(h.expect_text("text"), "one+two");
    assert_eq!(h.expect_number("size"), 7.0);
    assert_eq!(h.expect_number("isFile ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("isDir ? 1 : 0"), 0.0);
}

#[test]
fn fs_mkdir_readdir_rm() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-tree");
    let nested = format!("{dir}/a/b");
    h.eval(&format!(
        "globalThis.step = 0;
         fs.mkdir({nested:?}, {{ recursive: true }})
           .then(() => fs.writeFile({nested:?} + '/f.txt', 'x'))
           .then(() => fs.readdir({dir:?}, {{ withFileTypes: true }}))
           .catch((error) => {{ globalThis.failure = String(error); throw error; }})
           .then((items) => {{ globalThis.names = items.map((item) => item.name + (item.isDirectory() ? '/' : '')).join(','); }})
           .then(() => fs.readdir({nested:?}))
           .then((names) => {{ globalThis.inner = names.join(','); }})
           .then(() => fs.rm({dir:?}, {{ recursive: true }}))
           .then(() => fs.exists({dir:?}))
           .then((there) => {{ globalThis.stillThere = there; globalThis.step = 1; }});"
    ))
    .unwrap();
    idle(&mut h);
    let failure = h.expect_text("String(globalThis.failure || '')");
    assert_eq!(h.expect_number("step"), 1.0, "failure={failure}");
    assert_eq!(h.expect_text("names"), "a/");
    assert_eq!(h.expect_text("inner"), "f.txt");
    assert_eq!(h.expect_number("stillThere ? 1 : 0"), 0.0);
}

#[test]
fn fs_missing_file_rejects_with_enoent() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-missing");
    let file = format!("{dir}/nope.txt");
    h.eval(&format!(
        "globalThis.step = 0;
         fs.readFile({file:?})
           .then(() => {{ globalThis.step = 2; }})
           .catch((error) => {{ globalThis.code = error.code; globalThis.message = error.message; globalThis.step = 1; }});"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("step"), 1.0);
    assert_eq!(h.expect_text("code"), "ENOENT");
    assert!(h.expect_text("message").contains("No such file") || h.expect_text("message").contains("os error"));
}

#[test]
fn fs_rename_and_copy() {
    let mut h = Harness::with_modules();
    let dir = temp_dir("fs-rename");
    let from = format!("{dir}/one.txt");
    let to = format!("{dir}/two.txt");
    let copy = format!("{dir}/three.txt");
    h.eval(&format!(
        "globalThis.step = 0;
         fs.writeFile({from:?}, 'body')
           .then(() => fs.rename({from:?}, {to:?}))
           .then(() => fs.copyFile({to:?}, {copy:?}))
           .then(() => Promise.all([fs.readFile({to:?}, 'utf8'), fs.readFile({copy:?}, 'utf8')]))
           .then((pair) => {{ globalThis.pair = pair.join('|'); globalThis.step = 1; }});"
    ))
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("step"), 1.0);
    assert_eq!(h.expect_text("pair"), "body|body");
}

/// Each test case has its own temporary directory; don't worry about it after running (the system will clean it up).
fn temp_dir(tag: &str) -> String {
    let path = std::env::temp_dir().join(format!("xmas-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("create temporary directory");
    path.to_string_lossy().into_owned()
}

#[test]
fn crypto_hash_matches_known_digests() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.hex = ['sha1', 'sha256', 'sha384', 'sha512', 'md5']
           .map((alg) => crypto.createHash(alg).update('abc').digest('hex')).join('|');",
    )
    .unwrap();
    assert_eq!(
        h.expect_text("hex"),
        concat!(
            "a9993e364706816aba3e25717850c26c9cd0d89d|",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad|",
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7|",
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f|",
            "900150983cd24fb0d6963f7d28e17f72"
        )
    );
}

#[test]
fn crypto_hash_streams_and_copies() {
    let mut h = Harness::with_modules();
    h.eval(
        "const base = crypto.createHash('sha256').update('a');
         const copy = base.copy();
         base.update('bc');
         copy.update('bd');
         globalThis.streamed = base.digest('hex');
         globalThis.copied = copy.digest('hex');
         globalThis.bufferOut = Buffer.isBuffer(crypto.createHash('md5').update('abc').digest());
         globalThis.once = crypto.hash('sha256', 'abc', 'hex');",
    )
    .unwrap();
    assert_eq!(h.expect_text("streamed"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    // the one after copy only consumed 'a' + 'bd'
    assert_eq!(h.expect_text("copied"), "a52d159f262b2c6ddb724a61840befc36eb30c88877a4030b65cbe86298449c9");
    assert_eq!(h.expect_number("bufferOut ? 1 : 0"), 1.0);
    assert_eq!(h.expect_text("once"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn crypto_hmac_matches_known_digests() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.hex = crypto.createHmac('sha256', 'key').update('abc').digest('hex');
         globalThis.b64 = crypto.createHmac('sha256', 'key').update('abc').digest('base64');",
    )
    .unwrap();
    assert_eq!(
        h.expect_text("hex"),
        "9c196e32dc0175f86f4b1cb89289d6619de6bee699e4c378e68309ed97a1a6ab"
    );
    assert_eq!(h.expect_text("b64"), "nBluMtwBdfhvSxy4konWYZ3mvuaZ5MN45oMJ7Zehpqs=");
}

#[test]
fn crypto_random_bytes_and_uuid() {
    let mut h = Harness::with_modules();
    h.eval(
        "const a = crypto.randomBytes(16);
         const b = crypto.randomBytes(16);
         globalThis.sized = a.length;
         globalThis.isBuffer = Buffer.isBuffer(a);
         globalThis.differs = Buffer.compare(a, b) !== 0;
         globalThis.ordered = Buffer.compare(Buffer.from('ab'), Buffer.from('abc'));
         globalThis.uuid = crypto.randomUUID();
         globalThis.uuidShape = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(globalThis.uuid);
         const view = new Uint8Array(8);
         crypto.getRandomValues(view);
         globalThis.filled = view.some((byte) => byte !== 0);
         globalThis.int = crypto.randomInt(5, 10);
         globalThis.intInRange = globalThis.int >= 5 && globalThis.int < 10;",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(h.expect_number("sized"), 16.0);
    assert_eq!(h.expect_number("isBuffer ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("differs ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("ordered"), -1.0);
    assert_eq!(h.expect_number("uuidShape ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("filled ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("intInRange ? 1 : 0"), 1.0);
}

#[test]
fn crypto_timing_safe_equal() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.same = crypto.timingSafeEqual(Buffer.from('abcd'), Buffer.from('abcd'));
         try { crypto.timingSafeEqual(Buffer.from('abcd'), Buffer.from('abc')); globalThis.threw = false; }
         catch (error) { globalThis.threw = error instanceof RangeError; }",
    )
    .unwrap();
    assert_eq!(h.expect_number("same ? 1 : 0"), 1.0);
    assert_eq!(h.expect_number("threw ? 1 : 0"), 1.0);
}

