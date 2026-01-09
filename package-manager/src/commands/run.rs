//! Run command implementation.

use color_eyre::eyre::{ContextCompat, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::CompactString;
use deno_task_shell::KillSignal;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::exit;

use crate::commands::exec::shell;
use crate::commands::{install, join_paths, new_path};
use crate::progress::PROGRESS_BAR;
use crate::util::read_package;
use crate::watch::async_watch;

/// Execute the run command.
pub async fn cmd_run(arg: &crate::Args, name: &CompactString, watch: &[PathBuf]) -> Result<()> {
    join_paths()?;

    loop {
        let finish = async {
            let event = async_watch(watch.iter().map(|x| x.as_ref())).await?;
            PROGRESS_BAR.suspend(|| {
                println!(
                    "{} File modified: {}",
                    " WATCH ".on_purple(),
                    event.paths[0].to_string_lossy()
                )
            });
            PROGRESS_BAR.finish_and_clear();

            Ok(()) as Result<_>
        };

        let install = async {
            let package = read_package().await?;

            let script = package
                .scripts
                .get(name)
                .wrap_err(format!("Script `{name}` is not defined"))?
                .as_str()
                .wrap_err(format!("Script `{name}` is not a string"))?;

            install(arg).await?;
            let cwd = std::env::current_dir()?;
            let mut new_env = HashMap::new();
            new_env.insert(OsString::from("PATH"), new_path()?);
            let exit_code = shell(script, cwd, new_env, KillSignal::default()).await?;

            if exit_code != 0 {
                exit(exit_code);
            }

            Ok(()) as Result<_>
        };

        tokio::select! {
            res = finish => {
                res?;
            }
            res = install => {
                res?;
            }
        }
    }
}
