//! The package manager, as a part of the runtime.
//!
//! It was a crate of its own with a command line of its own, which put the same subcommands in two
//! parsers and left the runtime unable to install what an import asked for without crossing a
//! crate boundary. It is a module now: the binary owns the only parser (`src/main.rs` in the
//! workspace root), the commands below are plain functions, and an import that names a package
//! nobody installed calls into here directly (see `crate::deps`).
//!
//! Named `package` rather than `pm` because that is what it manages; `metadata` is where a
//! package.json is read.

pub mod autoload;
pub mod cache;
pub mod clone;
pub mod commands;
pub mod config;
pub mod metadata;
pub mod net;
pub mod npm;
pub mod plan;
pub mod pool;
pub mod progress;
pub mod resolve;
pub mod scoped_path;
pub mod util;
pub mod watch;

pub use progress::PROGRESS_BAR;

/// The two switches every command shares.
///
/// What the command line used to say with `Args`: the parser belongs to the binary, so what travels
/// down here is only what a command reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Say what is happening, as log lines above the progress bar.
    pub verbose: bool,
    /// Refuse to touch the lockfile.
    pub immutable: bool,
}

impl Options {
    /// Nothing said, nothing fixed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Say what is happening.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Refuse to touch the lockfile.
    pub fn immutable(mut self, immutable: bool) -> Self {
        self.immutable = immutable;
        self
    }
}

/// Take over the error reporting: a package manager's errors come with a report worth reading,
/// and the hooks behind it are process wide, so the binary asks for them once, before any command.
pub fn install_error_reporter() -> Result<(), String> {
    color_eyre::install().map_err(|error| format!("{error}"))
}

/// Finish what a command started: the trace, if one was asked for, and the progress bars.
pub fn finish() {
    net::trace_report();
    PROGRESS_BAR.finish_and_clear();
}
