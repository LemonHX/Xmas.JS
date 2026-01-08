//! Command implementations for Cotton CLI.

mod add;
mod clean;
mod create;
pub mod exec;
mod install;
mod remove;
mod run;
mod update;
mod upgrade;
mod why;

pub use add::cmd_add;
pub use clean::cmd_clean;
pub use create::cmd_create;
pub use exec::cmd_exec;
pub use install::{cmd_install, init_storage, install, join_paths, new_path};
pub use remove::cmd_remove;
pub use run::cmd_run;
pub use update::cmd_update;
pub use upgrade::cmd_upgrade;
pub use why::cmd_why;

use crate::{cli::Subcommand, Args};
use color_eyre::eyre::Result;

/// Execute the appropriate command based on CLI arguments.
pub async fn execute_command(args: &Args) -> Result<()> {
    match &args.cmd {
        Subcommand::Install => cmd_install(&args).await,
        Subcommand::Update => cmd_update(&args).await,
        Subcommand::Add { names, dev, pin } => cmd_add(&names, *dev, *pin).await,
        Subcommand::Run { name, watch } => cmd_run(&args, &name, &watch).await,
        Subcommand::Clean => cmd_clean(),
        Subcommand::Upgrade { pin } => cmd_upgrade(*pin).await,

        // TODO: fix with deno task shell
        Subcommand::Exec {
            exe,
            args: cmd_args,
        } => cmd_exec(&args, exe, cmd_args).await,
        Subcommand::Remove { names, dev } => cmd_remove(&names, *dev).await,
        Subcommand::Why { name, version } => cmd_why(&name, version.as_ref()).await,
        Subcommand::Create { name } => cmd_create(&args, &name).await,
        Subcommand::DownloadAndExec {
            name,
            args: cmd_args,
        } => exec::cmd_download_and_exec(&args, name, cmd_args).await,
    }
}
