//! node:child_process against real commands: the streams spawn hands back, the callbacks exec and
//! execFile take, the blocking calls, a kill, and the input a child reads.

use xmas_runtime::testkit::Harness;

/// Drive the loop until this global reads "yes".
fn run(h: &mut Harness, flag: &str) {
    for _ in 0..600 {
        h.driver().run_until_idle_blocking().expect("pump");
        if h.expect_text(&format!("String(globalThis.{flag})")) == "yes" {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("timed out waiting for {flag}");
}

fn harness() -> Harness {
    let mut h = Harness::with_modules();
    h.module(
        "app",
        r#"import { spawn, exec, execFile, spawnSync, execSync, execFileSync } from 'node:child_process';
           Object.assign(globalThis, { spawn, exec, execFile, spawnSync, execSync, execFileSync });"#,
    )
    .expect("module");
    h
}

#[test]
fn spawn_with_inherit_hands_the_output_to_this_process() {
    let mut h = harness();
    // The child writes to the descriptors this process already has, so nothing comes back on a
    // stream: the two readers report end at once, which is what a caller waiting on them sees.
    h.eval(
        r#"(async () => {
            const child = spawn('printf', ['inherited'], { stdio: 'inherit' });
            const chunks = [];
            child.stdout.on('data', (chunk) => chunks.push(chunk));
            await new Promise((done) => child.on('close', done));
            globalThis.result = chunks.length;
            globalThis.code = child.exitCode;
            globalThis.done = 'yes';
        })()"#,
    )
    .expect("eval");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(globalThis.result)"), "0");
    assert_eq!(h.expect_text("String(globalThis.code)"), "0");
}

#[test]
fn spawn_pipes_when_asked_for_pipes() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const child = spawn('printf', ['piped'], { stdio: ['ignore', 'pipe', 'pipe'] });
            const chunks = [];
            child.stdout.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
            await new Promise((done) => child.on('close', done));
            globalThis.result = chunks.map((chunk) => chunk.toString()).join('');
            globalThis.done = 'yes';
        })()"#,
    )
    .expect("eval");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(globalThis.result)"), "piped");
}

#[test]
fn spawn_hands_the_output_back_as_a_stream() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const child = spawn('printf', ['hi there']);
            const chunks = [];
            child.stdout.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
            await new Promise((done) => child.on('close', done));
            globalThis.result = chunks.map((chunk) => chunk.toString()).join('');
            globalThis.code = child.exitCode;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "hi there");
    assert_eq!(h.expect_number("code"), 0.0);
}

#[test]
fn exec_collects_the_output_and_calls_back() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            await new Promise((done) => {
                exec('printf one; printf two 1>&2', { encoding: 'utf8' }, (error, stdout, stderr) => {
                    globalThis.result = (error === null ? 'ok' : error.message) + '/' + stdout + '/' + stderr;
                    done();
                });
            });
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "ok/one/two");
}

#[test]
fn exec_hands_back_the_error_of_a_failing_command() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            await new Promise((done) => {
                exec('exit 4', { encoding: 'utf8' }, (error, stdout, stderr) => {
                    globalThis.result = (error === null ? 'no error' : String(error.code)) + '/' + typeof stdout;
                    done();
                });
            });
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "4/string");
}

#[test]
fn exec_file_does_not_go_through_a_shell() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            await new Promise((done) => {
                execFile('printf', ['%s', 'a b; rm -rf /'], { encoding: 'utf8' }, (error, stdout) => {
                    globalThis.result = error === null ? stdout : error.message;
                    done();
                });
            });
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "a b; rm -rf /");
}

#[test]
fn exec_sync_returns_the_output() {
    let mut h = harness();
    h.eval(
        r#"globalThis.result = execSync('printf synced', { encoding: 'utf8' });
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "synced");
}

#[test]
fn exec_sync_throws_with_the_status_and_the_output() {
    let mut h = harness();
    h.eval(
        r#"try {
               execSync('printf left; exit 5', { encoding: 'utf8' });
               globalThis.result = 'no throw';
           } catch (error) {
               globalThis.result = error.status + '/' + error.stdout;
           }
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "5/left");
}

#[test]
fn spawn_sync_reports_the_status_and_the_output() {
    let mut h = harness();
    h.eval(
        r#"const spawned = spawnSync('printf', ['out'], { encoding: 'utf8' });
           globalThis.result = spawned.status + '/' + spawned.stdout;
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "0/out");
}

#[test]
fn a_child_reads_what_is_written_to_it() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const child = spawn('cat');
            const chunks = [];
            child.stdout.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
            child.stdin.write('hello');
            child.stdin.end();
            await new Promise((done) => child.on('close', done));
            globalThis.result = chunks.map((chunk) => chunk.toString()).join('');
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "hello");
}

#[test]
fn a_child_can_be_killed() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const child = spawn('sleep', ['30']);
            const closed = new Promise((done) => child.on('close', (code, signal) => done({ code, signal })));
            child.kill('SIGKILL');
            const result = await closed;
            globalThis.result = String(result.code) + '/' + String(result.signal);
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "null/SIGKILL");
}

#[test]
fn an_unknown_signal_is_refused_and_a_known_one_is_sent() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const child = spawn('sleep', ['30']);
            try {
                child.kill('SIGTERM');
                try { child.kill('SIGNOPE'); globalThis.result = 'sent'; } catch (error) { globalThis.result = 'refused'; }
            } catch (error) {
                globalThis.result = error.message;
            }
            child.kill();
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "refused");
}

#[test]
fn max_buffer_stops_a_command_that_says_too_much() {
    let mut h = harness();
    h.eval(
        r#"try {
               execSync('printf 0123456789', { maxBuffer: 4, encoding: 'utf8' });
               globalThis.result = 'no throw';
           } catch (error) {
               globalThis.result = error.code;
           }
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(
        h.expect_text("String(result)"),
        "ERR_CHILD_PROCESS_STDIO_MAXBUFFER"
    );
}
