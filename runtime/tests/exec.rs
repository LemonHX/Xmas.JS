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

// ---------- the zx surface around it ----------

#[test]
fn a_preset_carries_its_defaults_and_chains() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const relaxed = $({ nothrow: true });
            const first = await relaxed`exit 1`;
            const both = relaxed({ quiet: true });
            const second = await both`exit 2`;
            globalThis.first = String(first.exitCode);
            globalThis.second = String(second.exitCode);
            globalThis.global_nothrow = String($.nothrow);
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(first)"), "1");
    assert_eq!(h.expect_text("String(second)"), "2");
    // A preset is a new $, not a change to the global one.
    assert_eq!(h.expect_text("String(global_nothrow)"), "false");
}

#[test]
fn sync_runs_the_line_before_it_returns() {
    let mut h = harness();
    h.eval(
        r#"const out = $.sync`printf synced`;
           globalThis.text = out.stdout;
           globalThis.code = out.exitCode;
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(text)"), "synced");
    assert_eq!(h.expect_number("code"), 0.0);
}

#[test]
fn sync_throws_unless_nothrow() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            let thrown = 'no throw';
            try { $.sync`exit 7`; } catch (error) { thrown = String(error.exitCode); }
            const relaxed = $({ nothrow: true });
            const kept = relaxed.sync`exit 7`;
            globalThis.thrown = thrown;
            globalThis.kept = String(kept.exitCode);
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(thrown)"), "7");
    assert_eq!(h.expect_text("String(kept)"), "7");
}

#[test]
fn a_command_gets_the_input_it_was_given() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const out = await $({ input: 'from the caller\n' })`cat`;
            globalThis.text = out.stdout;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(text)"), "from the caller\n");
}

#[test]
fn pipe_feeds_one_command_into_the_next() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const counted = await $`printf 'one two three'`.pipe`wc -w`;
            const shouted = await $`printf 'hello'`.pipe($`tr a-z A-Z`);
            globalThis.counted = counted.stdout.trim();
            globalThis.shouted = shouted.stdout;
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(counted)"), "3");
    assert_eq!(h.expect_text("String(shouted)"), "HELLO");
}

#[test]
fn cd_changes_where_the_next_command_runs_and_within_puts_it_back() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            cd('/tmp');
            const inside = (await $`pwd`).stdout.trim();
            const nested = await within(async () => {
                cd('/');
                return (await $`pwd`).stdout.trim();
            });
            const after = (await $`pwd`).stdout.trim();
            globalThis.seen = [inside.endsWith('/tmp'), nested, after === inside].join('/');
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    // /tmp is /private/tmp on macOS, so the check is on the name, not on the exact path.
    assert_eq!(h.expect_text("String(seen)"), "true///true");
}

#[test]
fn glob_finds_files_by_pattern() {
    let mut h = harness();
    // A tree of our own, so the test does not depend on what is in the repository.
    let root = std::env::temp_dir().join(format!("xmas-glob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/deep")).expect("directory");
    std::fs::write(root.join("src/a.ts"), "").expect("file");
    std::fs::write(root.join("src/deep/b.ts"), "").expect("file");
    std::fs::write(root.join("readme.md"), "").expect("file");
    h.eval(&format!("$.cwd = '{}';", root.display().to_string().replace('\\', "/")))
        .expect("cwd");
    h.eval(
        r#"(async () => {
            globalThis.top = glob.sync('*.md').join(',');
            globalThis.nested = glob.sync('src/**/*.ts').join(',');
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(top)"), "readme.md");
    assert_eq!(h.expect_text("String(nested)"), "src/a.ts,src/deep/b.ts");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn which_finds_a_program_on_the_path() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            const shell = await which('sh');
            const missing = await which('definitely-not-a-program-here', { nothrow: true });
            globalThis.found = String(shell).endsWith('/sh');
            globalThis.missing = String(missing);
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(found)"), "true");
    assert_eq!(h.expect_text("String(missing)"), "null");
}

#[test]
fn tmpdir_and_tmpfile_make_something_to_write_to() {
    let mut h = harness();
    h.eval(
        r#"const directory = tmpdir('inner');
           const file = tmpfile('note.txt', 'contents');
           globalThis.both = [directory.endsWith('/inner/'), fs.existsSync(directory), fs.readFileSync(file)].join('/');
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(both)"), "true/true/contents");
}

#[test]
fn minimist_parses_the_arguments_the_way_zx_does() {
    let mut h = harness();
    h.eval(
        r#"const parsed = minimist(['--name', 'x', '--flag', '-abc', '-n', '5', '--no-color', 'rest'], { boolean: ['flag'] });
           globalThis.seen = [parsed.name, String(parsed.flag), String(parsed.a && parsed.b && parsed.c), String(parsed.n), String(parsed.color), parsed._.join(',')].join('/');
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(seen)"), "x/true/true/5/false/rest");
}

#[test]
fn dotenv_parses_what_a_dotenv_file_says() {
    let mut h = harness();
    h.eval(
        r#"const values = dotenv.parse('# comment\nFOO=bar\nexport BAZ="quoted value"\n');
           globalThis.seen = values.FOO + '/' + values.BAZ;
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(seen)"), "bar/quoted value");
}

#[test]
fn chalk_writes_ansi_only_when_something_is_a_terminal() {
    let mut h = harness();
    h.eval(
        r#"globalThis.plain = chalk.red('x');
           globalThis.chained = chalk.red.bold('x');
           globalThis.done = 'yes';"#,
    )
    .expect("start");
    run(&mut h, "done");
    // Captured output is not a terminal, so nothing is coloured and the text is the text.
    assert_eq!(h.expect_text("String(plain)"), "x");
    assert_eq!(h.expect_text("String(chained)"), "x");
}

#[test]
fn echo_sleep_and_retry_do_what_they_say() {
    let mut h = harness();
    h.eval(
        r#"(async () => {
            echo('hello', 'world');
            echo`a ${1 + 1} b`;
            const started = Date.now();
            await sleep(20);
            const waited = Date.now() - started >= 15;
            let attempts = 0;
            const value = await retry(3, 1, () => {
                attempts += 1;
                if (attempts < 3) throw new Error('not yet');
                return 'third';
            });
            globalThis.seen = [waited, value, attempts].join('/');
            globalThis.done = 'yes';
        })();"#,
    )
    .expect("start");
    run(&mut h, "done");
    assert_eq!(h.expect_text("String(seen)"), "true/third/3");
}
