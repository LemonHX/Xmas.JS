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
    // console.log formats like Node: util.format, so an object is inspected.
    assert!(out.contains("[log] a 1 { b: 2 }"), "{out}");
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

#[test]
fn readable_pushes_data_then_end() {
    let mut h = Harness::with_modules();
    h.eval(
        "const r = new stream.Readable();
         globalThis.seen = [];
         r.on('data', (chunk) => seen.push(String(chunk)));
         r.on('end', () => seen.push('end'));
         r.push('a');
         r.push(Buffer.from('b'));
         r.push(null);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("seen.join('|')"), "a|b|end");
}

#[test]
fn readable_read_and_async_iteration() {
    let mut h = Harness::with_modules();
    h.eval(
        "const r = stream.Readable.from(['x', 'y', 'z']);
         globalThis.collected = null;
         (async () => { const parts = []; for await (const item of r) parts.push(item); globalThis.collected = parts.join(''); })();",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("collected"), "xyz");
}

#[test]
fn readable_read_pulls_bytes() {
    let mut h = Harness::with_modules();
    h.eval(
        "const r = new stream.Readable({ read() {} });
         r.push(Buffer.from('hello'));
         globalThis.first = String(r.read());
         globalThis.second = r.read();",
    )
    .unwrap();
    assert_eq!(h.expect_text("first"), "hello");
    assert_eq!(h.expect_text("String(second)"), "null");
}

#[test]
fn pipe_feeds_a_writable_and_finishes() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.parts = [];
         const sink = new stream.Writable({ write(chunk, enc, done) { parts.push(String(chunk)); done(); } });
         globalThis.finishedNow = false;
         sink.on('finish', () => { globalThis.finishedNow = true; });
         const source = new stream.Readable();
         source.pipe(sink);
         source.push('one ');
         source.push('two');
         source.push(null);",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("parts.join('')"), "one two");
    assert_eq!(h.expect_text("String(finishedNow)"), "true");
}

#[test]
fn transform_and_pipeline() {
    let mut h = Harness::with_modules();
    h.eval(
        "const upper = new stream.Transform({ transform(chunk, enc, done) { done(null, String(chunk).toUpperCase()); } });
         globalThis.piped = null;
         globalThis.pipelineDone = false;
         stream.pipeline(
           stream.Readable.from(['ab', 'cd']),
           upper,
           new stream.Writable({ write(chunk, enc, done) { globalThis.piped = (globalThis.piped || '') + String(chunk); done(); } }),
         ).then(() => { globalThis.pipelineDone = true; });",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("piped"), "ABCD");
    assert_eq!(h.expect_text("String(pipelineDone)"), "true");
}

#[test]
fn pass_through_keeps_bytes() {
    let mut h = Harness::with_modules();
    h.eval(
        "const pass = new stream.PassThrough();
         globalThis.out = [];
         pass.on('data', (chunk) => out.push(String(chunk)));
         pass.write('keep');
         pass.end();",
    )
    .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("out.join('')"), "keep");
}

#[test]
fn write_returns_false_over_high_water_mark_and_drains() {
    let mut h = Harness::with_modules();
    h.eval(
        "globalThis.calls = 0;
         globalThis.drains = 0;
         globalThis.pending = [];
         const sink = new stream.Writable({
           highWaterMark: 4,
           write(chunk, enc, done) { calls += 1; pending.push(done); },
         });
         sink.on('drain', () => { drains += 1; });
         globalThis.firstWrite = sink.write('abcdef');
         globalThis.secondWrite = sink.write('gh');",
    )
    .unwrap();
    assert_eq!(h.expect_text("String(firstWrite)"), "false");
    assert_eq!(h.expect_text("String(calls)"), "1");
    // Release the first write: pending drops, so drain should fire
    h.eval("pending.shift()();").unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("String(calls)"), "2");
    assert_eq!(h.expect_text("String(drains >= 1)"), "true");
}


#[test]
fn process_exposes_the_run() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("String(Array.isArray(process.argv))"), "true");
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    assert_eq!(h.expect_text("process.platform"), platform);
    assert_eq!(h.expect_text("String(process.pid > 0)"), "true");
    assert_eq!(h.expect_text("String(process.pid === process.pid)"), "true");
    assert_eq!(h.expect_text("typeof process.version"), "string");
    assert_eq!(h.expect_text("typeof process.cwd()"), "string");
    assert_eq!(h.expect_text("String(process.uptime() >= 0)"), "true");
}

#[test]
fn process_env_reads_without_a_host_call() {
    let mut h = Harness::with_modules();
    h.eval("process.env.XMAS_TEST_TOKEN = 'abc'").unwrap();
    assert_eq!(h.expect_text("process.env.XMAS_TEST_TOKEN"), "abc");
    // The write reached the native side, which is where a child process reads it.
    assert_eq!(h.host().env.get("XMAS_TEST_TOKEN").as_deref(), Some("abc"));
    h.eval("delete process.env.XMAS_TEST_TOKEN").unwrap();
    assert_eq!(h.expect_text("String(process.env.XMAS_TEST_TOKEN)"), "undefined");
    assert!(h.host().env.get("XMAS_TEST_TOKEN").is_none());
}

#[test]
fn process_chdir_moves_the_working_directory() {
    let mut h = Harness::with_modules();
    let before = h.expect_text("process.cwd()");
    let target = std::env::temp_dir();
    h.eval(&format!("process.chdir({:?})", target.to_string_lossy())).unwrap();
    let after = h.expect_text("process.cwd()");
    assert_ne!(before, after);
    h.eval(&format!("process.chdir({:?})", before)).unwrap();
    assert_eq!(h.expect_text("process.cwd()"), before);
}

#[test]
fn process_next_tick_runs_before_a_settled_promise() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.order = []; process.nextTick(() => order.push('tick')); Promise.resolve().then(() => order.push('promise'));")
        .unwrap();
    idle(&mut h);
    assert_eq!(h.expect_text("order.join(',')"), "tick,promise");
}

#[test]
fn process_next_tick_takes_arguments() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.seen = null; process.nextTick((a, b) => { seen = a + b; }, 2, 3);").unwrap();
    idle(&mut h);
    assert_eq!(h.expect_number("seen"), 5.0);
}

#[test]
fn process_hrtime_counts_forward() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.first = process.hrtime();")
        .unwrap();
    let seconds = h.expect_number("first[0]");
    let nanos = h.expect_number("first[1]");
    assert!(seconds >= 0.0 && nanos >= 0.0 && nanos < 1e9);
    assert_eq!(
        h.expect_text("String(typeof process.hrtime.bigint())"),
        "bigint"
    );
}

#[test]
fn process_memory_usage_reports_the_heap() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.usage = process.memoryUsage()").unwrap();
    assert!(h.expect_number("usage.heapUsed") > 0.0);
    assert_eq!(h.expect_number("usage.heapTotal"), 64.0 * 1024.0 * 1024.0);
    assert!(h.expect_number("usage.rss") > 0.0);
}

#[test]
fn process_exit_raises_under_a_capturing_host() {
    let mut h = Harness::with_modules();
    h.eval("process.on('exit', (code) => { globalThis.exitCode = code; })").unwrap();
    let thrown = h.eval_err("process.exit(3)");
    assert!(format!("{thrown:?}").contains("process.exit(3)"), "{thrown:?}");
    assert_eq!(h.expect_number("exitCode"), 3.0);
}

#[test]
fn process_stdout_writes_through_the_console() {
    let mut h = Harness::with_modules();
    h.eval("process.stdout.write('a'); process.stderr.write('b');")
        .unwrap();
    let out = h.host().console.take();
    assert_eq!(out, "ab");
    assert_eq!(h.expect_text("String(process.stdout.fd)"), "1");
}

#[test]
fn process_is_a_module_too() {
    let mut h = Harness::with_modules();
    let namespace = h.module("node:process", "").unwrap();
    assert_eq!(h.expect_text("typeof process.nextTick"), "function");
    drop(namespace);
}

#[test]
fn os_reports_the_machine() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("typeof os.platform()"), "string");
    assert_eq!(h.expect_text("String(os.cpus().length > 0)"), "true");
    assert_eq!(h.expect_text("String(os.totalmem() > 0)"), "true");
    assert_eq!(h.expect_text("String(os.uptime() > 0)"), "true");
    assert_eq!(h.expect_text("String(os.loadavg().length)"), "3");
    assert_eq!(h.expect_text("os.EOL"), "\n");
    assert_eq!(h.expect_text("String(os.homedir().length > 0)"), "true");
    assert_eq!(h.expect_text("String(os.tmpdir().length > 0)"), "true");
    assert_eq!(h.expect_text("typeof os.userInfo().username"), "string");
}

#[test]
fn os_is_a_module_too() {
    let mut h = Harness::with_modules();
    let namespace = h.module(
        "probe",
        "import { platform, EOL } from 'node:os'; export const values = [platform(), EOL];",
    );
    assert!(namespace.is_ok(), "{namespace:?}");
}

#[test]
fn path_strings_behave_like_node() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("path.dirname('/usr/local/bin')"), "/usr/local");
    assert_eq!(h.expect_text("path.dirname('/')"), "/");
    assert_eq!(h.expect_text("path.dirname('')"), ".");
    assert_eq!(h.expect_text("path.basename('/usr/local/bin.txt')"), "bin.txt");
    assert_eq!(h.expect_text("path.basename('/usr/local/bin.txt', '.txt')"), "bin");
    assert_eq!(h.expect_text("path.extname('/usr/local/bin.txt')"), ".txt");
    assert_eq!(h.expect_text("path.extname('.bashrc')"), "");
    assert_eq!(h.expect_text("path.join('/usr', 'local', 'bin')"), "/usr/local/bin");
    assert_eq!(h.expect_text("path.join('/usr', '..', 'local')"), "/local");
    assert_eq!(h.expect_text("path.join('a', '/b')"), "a/b");
    assert_eq!(h.expect_text("path.normalize('/foo//bar/../baz')"), "/foo/baz");
    assert_eq!(h.expect_text("path.relative('/a/b/c', '/a/d')"), "../../d");
    assert_eq!(h.expect_text("String(path.isAbsolute('/usr'))"), "true");
    assert_eq!(h.expect_text("String(path.isAbsolute('usr'))"), "false");
    assert_eq!(h.expect_text("path.sep"), "/");
    assert_eq!(h.expect_text("path.delimiter"), ":");
}

#[test]
fn path_resolve_walks_from_the_working_directory() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.dir = process.cwd()").unwrap();
    assert_eq!(h.expect_text("path.resolve('foo', 'bar')"), format!("{}/foo/bar", h.expect_text("dir")));
    assert_eq!(h.expect_text("path.resolve('/a', 'b', '..', 'c')"), "/a/c");
}

#[test]
fn path_parse_and_format_round_trip() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.parts = path.parse('/home/user/app.ts')").unwrap();
    assert_eq!(h.expect_text("parts.dir"), "/home/user");
    assert_eq!(h.expect_text("parts.base"), "app.ts");
    assert_eq!(h.expect_text("parts.ext"), ".ts");
    assert_eq!(h.expect_text("parts.name"), "app");
    assert_eq!(h.expect_text("path.format(parts)"), "/home/user/app.ts");
    assert_eq!(
        h.expect_text("path.format({ dir: '/a', name: 'b', ext: '.js' })"),
        "/a/b.js"
    );
}

#[test]
fn path_is_a_module_too() {
    let mut h = Harness::with_modules();
    let namespace = h.module(
        "probe",
        "import { join, sep } from 'node:path'; export const values = [join('/a', 'b'), sep];",
    );
    assert!(namespace.is_ok(), "{namespace:?}");
}

#[test]
fn inspect_writes_values_the_way_node_does() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("util.inspect(1)"), "1");
    assert_eq!(h.expect_text("util.inspect('text')"), "text");
    assert_eq!(h.expect_text("util.inspect([1, 'a', true])"), "[ 1, 'a', true ]");
    assert_eq!(h.expect_text("util.inspect({ a: 1, b: 'x' })"), "{ a: 1, b: 'x' }");
    assert_eq!(h.expect_text("util.inspect({ a: { b: { c: 1 } } })"), "{ a: { b: { c: 1 } } }");
    assert_eq!(h.expect_text("util.inspect(function named() {})"), "[function: named]");
    assert_eq!(h.expect_text("util.inspect(class Thing {})"), "[class: Thing]");
    assert_eq!(h.expect_text("util.inspect(new Map())"), "Map {}");
    assert_eq!(h.expect_text("util.inspect(new Date(0))"), "1970-01-01T00:00:00.000Z");
    assert_eq!(h.expect_text("util.inspect(/ab+/gi)"), "/ab+/gi");
    // An Error prints its stack, as Node does; only the first line is fixed.
    let error = h.expect_text("util.inspect(new TypeError('bad'))");
    assert!(error.starts_with("TypeError: bad\n  at "), "{error}");
}

#[test]
fn inspect_marks_a_cycle_and_stops_at_the_depth_limit() {
    let mut h = Harness::with_modules();
    assert_eq!(
        h.expect_text("const loop = { name: 'x' }; loop.self = loop; util.inspect(loop)"),
        "{ name: 'x', self: [Circular] }"
    );
    assert_eq!(
        h.expect_text("util.inspect({ a: { b: { c: { d: { e: 1 } } } } })"),
        "{ a: { b: { c: { d: [Object] } } } }"
    );
}

#[test]
fn format_handles_every_spec() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("util.format('%s is %d', 'age', 30)"), "age is 30");
    assert_eq!(h.expect_text("util.format('%d', '42')"), "42");
    assert_eq!(h.expect_text("util.format('%i', '42px')"), "42");
    assert_eq!(h.expect_text("util.format('%f', '1.5x')"), "1.5");
    assert_eq!(h.expect_text("util.format('%j', { a: 1 })"), "{\"a\":1}");
    assert_eq!(h.expect_text("util.format('%o', [1, 2])"), "[ 1, 2 ]");
    assert_eq!(h.expect_text("util.format('100%%')"), "100%%");
    assert_eq!(h.expect_text("util.format(1, 'two', [3])"), "1 two [ 3 ]");
    assert_eq!(h.expect_text("util.format('%s %s', 'only')"), "only %s");
    assert_eq!(h.expect_text("util.format('%c', 'color: red')"), "");
}

#[test]
fn style_text_wraps_and_nests() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("util.styleText('red', 'hi')"), "\u{1b}[31mhi\u{1b}[39m");
    assert_eq!(
        h.expect_text("util.styleText(['bold', 'red'], 'hi')"),
        "\u{1b}[1m\u{1b}[31mhi\u{1b}[39m\u{1b}[22m"
    );
    assert!(h.eval("util.styleText('nope', 'hi')").is_err());
}

#[test]
fn promisify_and_callbackify_turn_callbacks_around() {
    let mut h = Harness::with_modules();
    h.eval("globalThis.readish = (value, callback) => callback(null, value * 2)").unwrap();
    h.eval("globalThis.readishAsync = util.promisify(readish)").unwrap();
    let value = h.eval("readishAsync(21)").unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(number_of(&settled), Some(42.0));

    h.eval("globalThis.writeish = async (value) => value + 1").unwrap();
    h.eval("globalThis.writeishCb = util.callbackify(writeish)").unwrap();
    let value = h
        .eval("new Promise((resolve) => writeishCb(41, (error, value) => resolve(error ? 'error' : value)))")
        .unwrap();
    let settled = smol::block_on(h.settle(value)).unwrap();
    assert_eq!(number_of(&settled), Some(42.0));

    h.eval("globalThis.failing = (callback) => callback(new Error('nope'))").unwrap();
    let value = h.eval("util.promisify(failing)()").unwrap();
    assert!(smol::block_on(h.settle(value)).is_err());
}

#[test]
fn util_predicates_and_deep_equality() {
    let mut h = Harness::with_modules();
    assert_eq!(h.expect_text("String(util.types.isPromise(Promise.resolve()))"), "true");
    assert_eq!(h.expect_text("String(util.types.isDate(new Date()))"), "true");
    assert_eq!(h.expect_text("String(util.types.isTypedArray(new Uint8Array(2)))"), "true");
    assert_eq!(h.expect_text("String(util.isDeepStrictEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }))"), "true");
    assert_eq!(h.expect_text("String(util.isDeepStrictEqual({ a: 1 }, { a: 2 }))"), "false");
    assert_eq!(h.expect_text("String(util.isDeepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2])))"), "true");
    assert_eq!(h.expect_text("String(util.isDeepStrictEqual(NaN, NaN))"), "true");
    assert_eq!(h.expect_text("String(util.isDeepStrictEqual({}, Object.create(null)))"), "false");
}

#[test]
fn assert_ok_and_strict_equal() {
    let mut h = Harness::with_modules();
    assert!(h.eval("assert(true)").is_ok());
    assert!(h.eval("assert(1)").is_ok());
    assert!(h.eval("assert('text')").is_ok());
    assert!(h.eval("assert(false)").is_err());
    assert!(h.eval("assert(0)").is_err());
    assert!(h.eval("assert('')").is_err());
    let message = h.eval_err("assert.ok(0, 'needs a value')");
    assert!(message.message.contains("needs a value"), "{:?}", message);
    assert!(h.eval("assert.strictEqual(1, 1)").is_ok());
    assert!(h.eval("assert.strictEqual(1, 2)").is_err());
    assert!(h.eval("assert.strictEqual(0, -0)").is_err());
    assert!(h.eval("assert.equal('1', 1)").is_ok());
    assert!(h.eval("assert.deepStrictEqual({ a: [1] }, { a: [1] })").is_ok());
    assert!(h.eval("assert.deepStrictEqual({ a: [1] }, { a: [2] })").is_err());
    assert!(h.eval("assert.deepEqual({ a: 1 }, { a: '1' })").is_ok());
    assert!(h.eval("assert.notStrictEqual(1, 2)").is_ok());
}

#[test]
fn assert_throws_and_rejects() {
    let mut h = Harness::with_modules();
    assert!(h
        .eval("assert.throws(() => { throw new TypeError('bad'); }, TypeError)")
        .is_ok());
    assert!(h
        .eval("assert.throws(() => { throw new TypeError('bad'); }, /bad/)")
        .is_ok());
    assert!(h.eval("assert.throws(() => {}, TypeError)").is_err());
    assert!(h.eval("assert.throws(() => { throw new Error('x'); }, TypeError)").is_err());
    assert!(h.eval("assert.doesNotThrow(() => 1)").is_ok());
    assert!(h.eval("assert.doesNotThrow(() => { throw new Error('x'); })").is_err());
    assert!(h.eval("assert.match('hello', /ell/)").is_ok());
    assert!(h.eval("assert.match('hello', /xyz/)").is_err());
    assert!(h.eval("assert.ifError(null)").is_ok());
    assert!(h.eval("assert.ifError(new Error('x'))").is_err());

    let value = h
        .eval("assert.rejects(Promise.reject(new Error('no')), /no/)")
        .unwrap();
    assert!(smol::block_on(h.settle(value)).is_ok());
    let value = h.eval("assert.rejects(Promise.resolve())").unwrap();
    assert!(smol::block_on(h.settle(value)).is_err());
    let value = h
        .eval("assert.rejects(Promise.reject(new Error('no')), TypeError)")
        .unwrap();
    assert!(smol::block_on(h.settle(value)).is_err());
    assert_eq!(
        h.expect_text("new assert.AssertionError({ message: 'm' }).name"),
        "AssertionError"
    );
}

#[test]
fn assert_is_a_module_too() {
    let mut h = Harness::with_modules();
    assert!(h
        .module(
            "probe",
            "import assert, { deepStrictEqual } from 'node:assert'; deepStrictEqual({ a: 1 }, { a: 1 }); export const value = typeof assert;",
        )
        .is_ok());
    assert!(h
        .module("probe2", "import { strictEqual } from 'assert/strict'; strictEqual(1, 1); export const value = 1;")
        .is_ok());
}

// ---------- string_decoder ----------

fn is_true(value: &quickjs_oxide::engine::api::core::Value) -> bool {
    matches!(value, quickjs_oxide::engine::api::core::Value::Bool(true))
}

fn text_at(
    harness: &mut Harness,
    namespace: &quickjs_oxide::engine::api::core::Value,
    name: &str,
) -> String {
    let value = harness.property(namespace, name).unwrap();
    text_of(&value).unwrap_or_default()
}

#[test]
fn string_decoder_keeps_half_a_utf8_character() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "sd-utf8",
            "import { StringDecoder } from 'node:string_decoder';
             const d = new StringDecoder('utf8');
             export const encoding = d.encoding;
             export const first = d.write(Buffer.from([0xe4, 0xb8]));
             export const second = d.write(Buffer.from([0xad]));
             export const rest = d.write(Buffer.from(' tail'));
             export const done = d.end();",
        )
        .unwrap();
    assert_eq!(text_at(&mut h, &ns, "encoding"), "utf8");
    // Half of a three-byte character produces nothing until the rest arrives.
    assert_eq!(text_at(&mut h, &ns, "first"), "");
    assert_eq!(text_at(&mut h, &ns, "second"), "\u{4e2d}");
    assert_eq!(text_at(&mut h, &ns, "rest"), " tail");
    assert_eq!(text_at(&mut h, &ns, "done"), "");
}

#[test]
fn string_decoder_flushes_incomplete_bytes_as_replacement() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "sd-flush",
            "import { StringDecoder } from 'string_decoder';
             const d = new StringDecoder('utf-8');
             export const first = d.write(Buffer.from([0xe4]));
             export const done = d.end();
             export const after = d.write(Buffer.from('ok'));",
        )
        .unwrap();
    assert_eq!(text_at(&mut h, &ns, "first"), "");
    assert_eq!(text_at(&mut h, &ns, "done"), "\u{fffd}");
    // end() empties the state, so the next chunk starts clean.
    assert_eq!(text_at(&mut h, &ns, "after"), "ok");
}

#[test]
fn string_decoder_handles_utf16_surrogates_and_base64_groups() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "sd-more",
            "import { StringDecoder } from 'node:string_decoder';
             const wide = new StringDecoder('utf16le');
             const all = Buffer.from('ab\u{1F600}', 'utf16le');
             export const wideFirst = wide.write(all.subarray(0, 3));
             export const wideRest = wide.end(all.subarray(3));
             const b64 = new StringDecoder('base64');
             export const b64First = b64.write(Buffer.from([1]));
             export const b64Rest = b64.write(Buffer.from([2, 3]));
             const hex = new StringDecoder('hex');
             export const hexText = hex.write(Buffer.from([0xde, 0xad]));
             const latin = new StringDecoder('binary');
             export const latinLabel = latin.encoding;
             export const latinText = latin.write(Buffer.from([0xe9]));",
        )
        .unwrap();
    assert_eq!(text_at(&mut h, &ns, "wideFirst"), "a");
    assert_eq!(text_at(&mut h, &ns, "wideRest"), "b\u{1F600}");
    // A base64 decoder works on groups of three bytes, like Node's.
    assert_eq!(text_at(&mut h, &ns, "b64First"), "");
    assert_eq!(text_at(&mut h, &ns, "b64Rest"), "AQID");
    assert_eq!(text_at(&mut h, &ns, "hexText"), "dead");
    assert_eq!(text_at(&mut h, &ns, "latinLabel"), "latin1");
    assert_eq!(text_at(&mut h, &ns, "latinText"), "\u{e9}");
}

#[test]
fn string_decoder_rejects_an_unknown_encoding() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "sd-bad",
            "import { StringDecoder } from 'string_decoder';
             export const attempt = () => new StringDecoder('bogus');",
        )
        .unwrap();
    let thrown = h.call_err(&ns, "attempt", &[]);
    assert!(thrown.name.contains("TypeError"), "{thrown:?}");
    assert!(thrown.message.contains("Unknown encoding: bogus"), "{thrown:?}");
}

// ---------- zlib ----------

#[test]
fn zlib_round_trips_gzip_and_deflate() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "zl-round",
            "import zlib from 'node:zlib';
             const text = 'hello hello hello hello hello';
             const source = Buffer.from(text);
             const gz = zlib.gzipSync(source);
             const zl = zlib.deflateSync(source);
             const raw = zlib.deflateRawSync(source);
             export const gzipMagic = gz[0].toString(16) + gz[1].toString(16);
             // zlib starts with 78, raw deflate has no header at all
             export const zlibHeader = zl[0].toString(16);
             export const rawHeader = raw[0].toString(16);
             export const gzipBack = zlib.gunzipSync(gz).toString();
             export const zlibBack = zlib.inflateSync(zl).toString();
             export const rawBack = zlib.inflateRawSync(raw).toString();",
        )
        .unwrap();
    assert_eq!(text_at(&mut h, &ns, "gzipMagic"), "1f8b");
    assert_eq!(text_at(&mut h, &ns, "zlibHeader"), "78");
    assert!(text_at(&mut h, &ns, "rawHeader") != "78");
    for name in ["gzipBack", "zlibBack", "rawBack"] {
        assert_eq!(text_at(&mut h, &ns, name), "hello hello hello hello hello", "{name}");
    }
}

#[test]
fn zlib_round_trips_brotli_and_computes_crc32() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "zl-brotli",
            "import { brotliCompressSync, brotliDecompressSync, crc32 } from 'zlib';
             const source = Buffer.from('brotli payload '.repeat(20));
             const packed = brotliCompressSync(source);
             export const back = brotliDecompressSync(packed).toString();
             export const smaller = packed.length < source.length;
             export const checksum = crc32(Buffer.from('hello'));
             export const seeded = crc32(Buffer.from(''), crc32(Buffer.from('hello')));",
        )
        .unwrap();
    assert_eq!(text_at(&mut h, &ns, "back"), "brotli payload ".repeat(20));
    assert!(is_true(&h.property(&ns, "smaller").unwrap()));
    // zlib.crc32('hello') is 0x3610a686, the value Node prints.
    let checksum = h.property(&ns, "checksum").unwrap();
    assert_eq!(number_of(&checksum), Some(907060870.0));
}

#[test]
fn zlib_stops_at_max_output_length() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "zl-limit",
            "import { gzipSync, gunzipSync } from 'node:zlib';
             const packed = gzipSync(Buffer.from('x'.repeat(1000)));
             export const attempt = () => {
               try {
                 gunzipSync(packed, { maxOutputLength: 10 });
                 return 'no error';
               } catch (error) {
                 return error.name + '|' + error.message;
               }
             };",
        )
        .unwrap();
    let value = h.call(&ns, "attempt", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("RangeError|Cannot create a Buffer larger than 10 bytes"));
}

#[test]
fn zlib_reports_a_bad_payload_as_a_zlib_error() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "zl-bad",
            "import { gunzipSync } from 'node:zlib';
             export const attempt = () => {
               try {
                 gunzipSync(Buffer.from([1, 2, 3, 4]));
                 return 'no error';
               } catch (error) {
                 return error.code + '|' + (error instanceof Error);
               }
             };",
        )
        .unwrap();
    let value = h.call(&ns, "attempt", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("Z_DATA_ERROR|true"));
}

#[test]
fn zlib_callback_form_runs_off_the_realm_thread() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "zl-async",
            "import zlib from 'zlib';
             export const packed = () => new Promise((resolve, reject) => {
               zlib.gzip(Buffer.from('async payload'), (error, output) => {
                 if (error) reject(error); else resolve(output);
               });
             });
             export const same = async () => {
               const gz = await packed();
               return new Promise((resolve, reject) => {
                 zlib.gunzip(gz, (error, output) => {
                   if (error) reject(error); else resolve(output.toString());
                 });
               });
             };",
        )
        .unwrap();
    let promise = h.call(&ns, "same", &[]).unwrap();
    let settled = smol::block_on(h.settle(promise)).unwrap();
    assert_eq!(text_of(&settled).as_deref(), Some("async payload"));
}

// ---------- perf_hooks ----------

#[test]
fn perf_hooks_clocks_and_measures() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "ph-entries",
            "import { performance } from 'node:perf_hooks';
             export const monotonic = () => {
               const first = performance.now();
               let sum = 0;
               for (let i = 0; i < 1000; i += 1) sum += i;
               return performance.now() >= first && sum > 0;
             };
             export const originIsEpoch = () => performance.timeOrigin > 1600000000000;
             export const measured = () => {
               performance.mark('start');
               performance.mark('finish');
               const entry = performance.measure('between', 'start', 'finish');
               return entry.entryType + '|' + (entry.duration >= 0) + '|' + performance.getEntriesByType('measure').length;
             };
             export const cleared = () => {
               performance.clearMarks();
               performance.clearMeasures();
               return performance.getEntries().length;
             };
             export const sameObject = () => performance.now === globalThis.performance.now;
             export const missingMark = () => {
               try {
                 performance.measure('nope', 'not-there');
                 return 'no error';
               } catch (error) {
                 return error.name + '|' + error.message.includes('does not exist');
               }
             };",
        )
        .unwrap();
    let value = h.call(&ns, "monotonic", &[]).unwrap();
    assert!(is_true(&value));
    let value = h.call(&ns, "originIsEpoch", &[]).unwrap();
    assert!(is_true(&value));
    let value = h.call(&ns, "measured", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("measure|true|1"));
    let value = h.call(&ns, "cleared", &[]).unwrap();
    assert_eq!(number_of(&value), Some(0.0));
    let value = h.call(&ns, "sameObject", &[]).unwrap();
    assert!(is_true(&value));
    let value = h.call(&ns, "missingMark", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("Error|true"));
}

#[test]
fn perf_hooks_observer_sees_buffered_marks() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "ph-observer",
            "import { performance, PerformanceObserver } from 'node:perf_hooks';
             export const watch = () => new Promise((resolve) => {
               performance.mark('early');
               const observer = new PerformanceObserver((list) => {
                 observer.disconnect();
                 resolve(list.getEntries().map((entry) => entry.name).join(','));
               });
               observer.observe({ type: 'mark', buffered: true });
             });",
        )
        .unwrap();
    let promise = h.call(&ns, "watch", &[]).unwrap();
    let settled = smol::block_on(h.settle(promise)).unwrap();
    assert_eq!(text_of(&settled).as_deref(), Some("early"));
}

// ---------- tty ----------

#[test]
fn tty_reports_descriptors_and_streams() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "tty-shape",
            "import tty, { isatty, WriteStream, ReadStream } from 'node:tty';
             export const kinds = () => typeof isatty(1) + '|' + isatty(-1) + '|' + typeof tty.isatty;
             export const out = () => {
               const stream = new WriteStream(2);
               return typeof stream.isTTY + '|' + typeof stream.columns + '|' + stream.fd;
             };
             export const terminal = () => {
               const stream = new WriteStream(2);
               return typeof stream.getColorDepth() + '|' + stream.hasColors(16);
             };
             export const input = () => {
               const stream = new ReadStream(0);
               return stream.isTTY === isatty(0) && stream.isRaw === false;
             };",
        )
        .unwrap();
    for (name, want) in [
        ("kinds", "boolean|false|function"),
        ("out", "boolean|number|2"),
        ("terminal", "number|true"),
    ] {
        let value = h.call(&ns, name, &[]).unwrap();
        assert_eq!(text_of(&value).as_deref(), Some(want), "{name}");
    }
    let value = h.call(&ns, "input", &[]).unwrap();
    assert!(is_true(&value));
}

// ---------- async_hooks ----------

#[test]
fn async_hooks_stores_travel_through_run_and_bind() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "ah-store",
            "import { AsyncLocalStorage, AsyncResource, executionAsyncId, createHook } from 'node:async_hooks';
             const storage = new AsyncLocalStorage();
             export const inside = () => storage.run('request-1', () => storage.getStore());
             export const outside = () => storage.getStore() === undefined;
             export const nested = () => storage.run('a', () => storage.run('b', () => storage.getStore()));
             export const bound = () => {
               const carried = storage.run('carried', () => storage.bind(() => storage.getStore()));
               return storage.run('other', () => carried());
             };
             export const leaving = () => {
               storage.enterWith('entered');
               const before = storage.getStore();
               const after = storage.exit(() => storage.getStore());
               return before + '|' + (after === undefined) + '|' + storage.getStore();
             };
             export const resource = () => new AsyncResource('test').runInAsyncScope(() => 'ran');
             export const hookShape = () => {
               const hook = createHook({ init() {} });
               hook.enable();
               const wasEnabled = hook.enabled;
               hook.disable();
               return wasEnabled + '|' + hook.enabled + '|' + executionAsyncId();
             };",
        )
        .unwrap();
    let value = h.call(&ns, "inside", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("request-1"));
    let value = h.call(&ns, "outside", &[]).unwrap();
    assert!(is_true(&value));
    let value = h.call(&ns, "nested", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("b"));
    let value = h.call(&ns, "bound", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("carried"));
    let value = h.call(&ns, "leaving", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("entered|true|entered"));
    let value = h.call(&ns, "resource", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("ran"));
    let value = h.call(&ns, "hookShape", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("true|false|0"));
}

// ---------- DOMException ----------

#[test]
fn dom_exception_matches_the_webidl_table() {
    let mut h = Harness::with_modules();
    let ns = h
        .module(
            "dom-exception",
            "export const aborted = () => {
               const error = new DOMException('stopped', 'AbortError');
               return error.name + '|' + error.code + '|' + error.message + '|' + (error instanceof Error);
             };
             export const quota = () => {
               const error = new QuotaExceededError('too big');
               return (error instanceof DOMException) + '|' + error.name + '|' + error.code;
             };
             export const tagged = () => Object.prototype.toString.call(new DOMException('', 'NotFoundError'));
             export const unnamed = () => new DOMException().name + '|' + new DOMException().code;",
        )
        .unwrap();
    let value = h.call(&ns, "aborted", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("AbortError|20|stopped|true"));
    let value = h.call(&ns, "quota", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("true|QuotaExceededError|22"));
    let value = h.call(&ns, "tagged", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("[object DOMException]"));
    let value = h.call(&ns, "unnamed", &[]).unwrap();
    assert_eq!(text_of(&value).as_deref(), Some("Error|0"));
}
