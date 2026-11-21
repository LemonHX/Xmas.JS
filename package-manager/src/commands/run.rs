//! Run command implementation.

use color_eyre::eyre::{ContextCompat, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::CompactString;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::exit;

use crate::commands::exec::shell;
use crate::commands::{install, join_paths, new_path};
use crate::progress::PROGRESS_BAR;
use crate::util::read_package;
use crate::watch::watch;

/// Run a script from package.json.
///
/// Without --watch this is one run. With it, the script runs again each time something under the
/// watched paths changes, and the exit code of a run only ends this process.
pub fn cmd_run(arg: &crate::Args, name: &CompactString, watch_paths: &[PathBuf]) -> Result<()> {
    join_paths()?;

    loop {
        let package = read_package()?;

        let script = package
            .scripts
            .get(name)
            .wrap_err(format!("Script `{name}` is not defined"))?
            .as_str()
            .wrap_err(format!("Script `{name}` is not a string"))?;

        install(arg)?;
        let cwd = std::env::current_dir()?;
        let mut new_env = HashMap::new();
        new_env.insert(OsString::from("PATH"), new_path()?);
        let exit_code = shell(script, cwd, new_env)?;

        if exit_code != 0 {
            exit(exit_code);
        }

        if watch_paths.is_empty() {
            return Ok(());
        }

        let watcher = watch(watch_paths.iter().map(|path| path.as_ref()))?;
        let event = watcher.next();
        PROGRESS_BAR.suspend(|| {
            println!(
                "{} File modified: {}",
                " WATCH ".on_purple(),
                event.paths[0].to_string_lossy()
            )
        });
        PROGRESS_BAR.finish_and_clear();
    }
}
