//! `$` against real commands: output, exit codes, quoting, stderr, a timeout, and a command
//! whose output is bigger than the pipe backlog.

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
    // Quiet, so the captured console holds only what a test prints itself.
    h.eval("$.quiet = true;").expect("quiet");
    h
}

#[test]
fn a_command_answers_with_its_output() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $`printf 'hi\nthere'`;
            globalThis.text = out.stdout;
            globalThis.code = out.exitCode;
            globalThis.ok = String(out.ok);
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(text)"), "hi\nthere");
    assert_eq!(h.expect_number("code"), 0.0);
    assert_eq!(h.expect_text("String(ok)"), "true");
}

#[test]
fn a_failing_command_throws_with_its_code() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            try {
                await $`exit 3`;
                globalThis.result = 'no throw';
            } catch (error) {
                globalThis.result = error.name + ':' + error.exitCode;
            }
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "ProcessError:3");
}

#[test]
fn nothrow_hands_back_the_output_of_a_failing_command() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $`printf gone; exit 2`.nothrow();
            globalThis.result = out.stdout + '/' + out.exitCode;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "gone/2");
}

#[test]
fn a_value_is_quoted_for_the_shell() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const dangerous = String.fromCharCode(39) + "a b; rm -rf /" + String.fromCharCode(39);
            const out = await $`printf %s ${dangerous}`;
            globalThis.result = out.stdout;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "'a b; rm -rf /'");
}

#[test]
fn stderr_is_kept_apart() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $`printf oops 1>&2`;
            globalThis.result = out.stdout + '/' + out.stderr;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "/oops");
}

#[test]
fn a_timeout_kills_the_command() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const started = Date.now();
            try {
                await $`sleep 30`.timeout(150);
                globalThis.result = 'no throw';
            } catch (error) {
                globalThis.result = error.message.indexOf('timed out') >= 0 ? 'timed out' : error.message;
            }
            globalThis.elapsed = Date.now() - started;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "timed out");
    assert!(
        h.expect_number("elapsed") < 3_000.0,
        "the timeout did not stop the command"
    );
}

#[test]
fn a_command_runs_where_it_was_told_to() {
    let directory = std::env::temp_dir();
    let quoted = format!("'{}'", directory.to_string_lossy().replace('\'', "\\'"));
    let mut h = harness();
    h.eval(&format!(
        r#"(async () => {{
            const out = await $`pwd`.cwd({quoted});
            globalThis.result = out.stdout.trim();
            globalThis.done = 'yes';
        }})();"#
    ))
    .expect("start");
    run(&mut h, "done");
    let seen = std::fs::canonicalize(h.expect_text("String(result)")).expect("canonical");
    let expected = std::fs::canonicalize(&directory).expect("canonical");
    assert_eq!(seen, expected);
}

#[test]
fn output_larger_than_the_pipe_backlog_arrives_whole() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $`seq 1 50000`;
            globalThis.lines = out.stdout.split('\n').filter((line) => line !== '').length;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_number("lines"), 50_000.0);
}

#[test]
fn an_interpolated_number_goes_in_as_a_number() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $`printf %s ${7}`;
            globalThis.result = out.stdout;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(result)"), "7");
}
