//! `xmas test`: find the test files, and run each in a realm of its own.
//!
//! A realm per file keeps one file's globals, mocks and module state out of the next. The
//! framework itself is js/test.js (see modules::testing); this side loads a file, starts the run
//! it collected and reads the summary the framework leaves behind.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::host::Host;

#[derive(Clone, Debug)]
pub struct Options {
    pub timeout_ms: u64,
    pub bail: bool,
    pub name_pattern: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options { timeout_ms: 5000, bail: false, name_pattern: None }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileResult {
    pub pass: u64,
    pub fail: u64,
    pub skip: u64,
    pub todo: u64,
    pub expects: u64,
    /// The file could not be loaded, or the run never finished.
    pub error: Option<String>,
}

const SUFFIXES: [&str; 8] = ["ts", "tsx", "js", "jsx", "mjs", "mts", "cjs", "cts"];

fn is_test_file(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else { return false };
    SUFFIXES.contains(&extension)
        && ["test", "spec"].iter().any(|word| {
            stem.ends_with(&format!(".{word}")) || stem.ends_with(&format!("_{word}"))
        })
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            walk(&path, out);
        } else if is_test_file(&name) {
            out.push(path);
        }
    }
}

/// The test files under root. Filters are substrings of the path; a filter that names a file is
/// taken as that file, whatever it is called.
pub fn find_tests(root: &Path, filters: &[String]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, &mut found);
    found.sort();
    if filters.is_empty() {
        return found;
    }
    let mut chosen: Vec<PathBuf> = found
        .into_iter()
        .filter(|path| {
            let shown = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
            filters.iter().any(|filter| shown.contains(filter.trim_start_matches("./")))
        })
        .collect();
    for filter in filters {
        let direct = root.join(filter);
        if direct.is_file() && !chosen.contains(&direct) {
            chosen.push(direct);
        }
    }
    chosen
}

/// Run one file. Everything it prints goes to host's console.
pub fn run_file(path: &Path, options: &Options, host: Host) -> FileResult {
    match try_run(path, options, host) {
        Ok(result) => result,
        Err(error) => FileResult { error: Some(error), ..FileResult::default() },
    }
}

fn try_run(path: &Path, options: &Options, host: Host) -> Result<FileResult, String> {
    let absolute = std::fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut driver = crate::entry::realm(host, "")?;
    crate::modules::testing::install(&mut driver)?;
    driver.eval("__xmas_test_globals()")?;
    crate::entry::evaluate_file(&mut driver, &absolute)?;
    let settings = serde_json::json!({
        "timeout": options.timeout_ms,
        "bail": options.bail,
        "pattern": options.name_pattern,
    });
    let quoted = serde_json::to_string(&settings.to_string()).map_err(|error| error.to_string())?;
    driver.eval(&format!("__xmas_test_start({quoted})"))?;
    wait_for_result(&mut driver)?;
    let value = driver.eval("globalThis.__xmas_test_result")?;
    let Some(text) = crate::testkit::text_of(&value) else {
        return Err("the tests did not finish: something never settled".into());
    };
    let summary: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if let Some(error) = summary.get("error").and_then(|e| e.as_str()) {
        return Err(error.to_string());
    }
    let count = |key: &str| summary.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(FileResult {
        pass: count("pass"),
        fail: count("fail"),
        skip: count("skip"),
        todo: count("todo"),
        expects: count("expects"),
        error: None,
    })
}

/// Find and run the tests under root, printing a bun-style report. True when nothing failed.
pub fn run(root: &Path, filters: &[String], options: &Options) -> bool {
    let files = find_tests(root, filters);
    if files.is_empty() {
        eprintln!("xmas test: no test files found");
        return false;
    }
    let started = Instant::now();
    let mut total = FileResult::default();
    let mut broken = 0u64;
    for path in &files {
        println!("\n{}:", path.strip_prefix(root).unwrap_or(path).display());
        let result = run_file(path, options, Host::new());
        if let Some(error) = &result.error {
            println!("error: {error}");
            broken += 1;
        }
        total.pass += result.pass;
        total.fail += result.fail;
        total.skip += result.skip;
        total.todo += result.todo;
        total.expects += result.expects;
        if options.bail && (result.fail > 0 || result.error.is_some()) {
            break;
        }
    }
    println!();
    println!(" {} pass", total.pass);
    if total.skip > 0 { println!(" {} skip", total.skip); }
    if total.todo > 0 { println!(" {} todo", total.todo); }
    println!(" {} fail", total.fail + broken);
    println!(" {} expect() calls", total.expects);
    println!(
        "Ran {} tests across {} files. [{:.2}s]",
        total.pass + total.fail + total.skip + total.todo,
        files.len(),
        started.elapsed().as_secs_f64()
    );
    total.fail == 0 && broken == 0
}

/// Turn the realm until the framework has left its summary. Not until the realm is idle: a test's
/// timeout timer that was cleared can still sit in the timer table, and waiting for it to drain
/// would hold every file for the length of the timeout.
fn wait_for_result(driver: &mut crate::driver::Driver) -> Result<(), String> {
    use std::time::{Duration, Instant};
    loop {
        let slice = driver.slice()?;
        let value = driver.eval("globalThis.__xmas_test_result")?;
        if crate::testkit::text_of(&value).is_some() {
            return Ok(());
        }
        let pending = driver.runtime().is_job_pending().map_err(|error| format!("{error}"))?;
        if pending || slice.jobs > 0 || slice.timers > 0 || slice.settled > 0 {
            continue;
        }
        match driver.host.timers.next_deadline() {
            Some(when) => {
                let wait = when.saturating_duration_since(Instant::now());
                if wait > Duration::ZERO {
                    std::thread::sleep(wait.min(Duration::from_millis(5)));
                }
            }
            None if driver.host.pending.outstanding() > 0 => driver.host.pending.wait_blocking(),
            None => return Ok(()),
        }
    }
}
