//! Create command implementation.

use color_eyre::eyre::Result;
use compact_str::CompactString;
use std::ffi::OsStr;

use crate::commands::exec::{exec_with_args, install_bin_temp};

/// Execute the create command.
pub async fn cmd_create(args: &crate::Args, name: &CompactString) -> Result<()> {
    let name = format!("create-{name}");
    install_bin_temp(args, &name).await?;
    exec_with_args(OsStr::new(&name), &[])
}
