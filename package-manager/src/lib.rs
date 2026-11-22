//! Cotton - A fast JavaScript/Node.js package manager.
//!
//! This library provides the core functionality for managing npm packages,
//! resolving dependencies, and executing package scripts.

pub mod autoload;
pub mod cache;
pub mod cli;
pub mod commands;
pub mod config;
pub mod net;
pub mod npm;
pub mod package;
pub mod plan;
pub mod pool;
pub mod progress;
pub mod resolve;
pub mod scoped_path;
pub mod util;
pub mod watch;

pub use cli::{Args, Subcommand};
pub use commands::execute_command;
pub use progress::PROGRESS_BAR;

// ---

use color_eyre::eyre::Result;
use std::env::set_current_dir;

pub fn package_manager(args: &Args) -> Result<()> {
    color_eyre::install()?;
    if let Some(cwd) = &args.working_dir {
        set_current_dir(cwd)?;
    }
    progress::set_verbose(args.verbose);
    execute_command(&args)?;
    net::trace_report();
    PROGRESS_BAR.finish_and_clear();
    Ok(())
}
