use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use owo_colors::OwoColorize;

/// The bar stays hidden for this long, so a command that finishes at once draws nothing.
const SHOW_AFTER: Duration = Duration::from_millis(500);

/// How many in-flight packages the status line names before it says "+N".
const NAMED: usize = 3;

static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);
static SHOWN: AtomicBool = AtomicBool::new(false);

pub static PROGRESS_BAR: LazyLock<ProgressBar> = LazyLock::new(|| {
    LazyLock::force(&STARTED);
    // Hidden until SHOW_AFTER has passed; a non-terminal stderr stays hidden for good.
    let pb = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::hidden()).with_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} {wide_msg}\n{bar:40.green/dim} {pos}/{len} packages {elapsed_precise:.dim}"
        )
        .unwrap()
        .progress_chars("━━╾─")
        .tick_chars("◐◓◑◒"),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
});

#[derive(Default)]
struct Status {
    /// What the command is doing when no package is in flight.
    phase: String,
    /// The packages being downloaded or installed right now, oldest first.
    in_flight: Vec<String>,
}

static STATUS: LazyLock<Mutex<Status>> = LazyLock::new(Default::default);

/// Reveal the bar once the command has run long enough, then redraw the status line.
fn refresh() {
    if !SHOWN.load(Ordering::Relaxed)
        && STARTED.elapsed() >= SHOW_AFTER
        && !SHOWN.swap(true, Ordering::Relaxed)
    {
        PROGRESS_BAR.set_draw_target(ProgressDrawTarget::stderr_with_hz(15));
    }

    let status = STATUS.lock().unwrap();
    let message = if status.in_flight.is_empty() {
        status.phase.clone()
    } else {
        let names = status.in_flight.iter().take(NAMED).cloned().collect::<Vec<_>>().join(", ");
        match status.in_flight.len().saturating_sub(NAMED) {
            0 => names,
            more => format!("{names} +{more}"),
        }
    };
    PROGRESS_BAR.set_message(message);
}

pub fn set_total(total: u64) {
    PROGRESS_BAR.set_length(total);
}

pub fn inc_progress() {
    PROGRESS_BAR.inc(1);
    refresh();
}

/// A package's work starts: it shows on the status line until `end`.
pub fn begin(label: &str) {
    STATUS.lock().unwrap().in_flight.push(label.to_string());
    refresh();
}

/// A package's work is done: it leaves the status line and counts as one step.
pub fn end(label: &str) {
    {
        let mut status = STATUS.lock().unwrap();
        if let Some(at) = status.in_flight.iter().position(|x| x == label) {
            status.in_flight.remove(at);
        }
    }
    log_verbose(&format!("Done {label}"));
    inc_progress();
}

static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Turn verbose logging on, from the command line flag.
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

pub fn log_verbose(text: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        PROGRESS_BAR.suspend(|| println!("{} {}", " VERBOSE ".on_white(), text));
    }
}

pub fn log_warning(text: &str) {
    PROGRESS_BAR.suspend(|| println!("{} {}", " WARNING ".on_yellow(), text));
}

/// Name the current phase. It does not count as a step.
pub fn log_progress(text: &str) {
    STATUS.lock().unwrap().phase = text.to_string();
    refresh();
    log_verbose(text);
}

pub fn finish_progress() {
    PROGRESS_BAR.finish_with_message("✨ Done!".green().to_string());
}
