use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock,
    },
    time::Duration,
};

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;

pub static PROGRESS_BAR: LazyLock<ProgressBar> = LazyLock::new(|| {
    let pb = ProgressBar::new(0).with_style(
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

pub fn set_total(total: u64) {
    PROGRESS_BAR.set_length(total);
}

pub fn inc_progress() {
    PROGRESS_BAR.inc(1);
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

pub fn log_progress(text: &str) {
    PROGRESS_BAR.set_message(text.to_string());
    inc_progress();
    log_verbose(text);
}

pub fn finish_progress() {
    PROGRESS_BAR.finish_with_message("✨ Done!".green().to_string());
}
