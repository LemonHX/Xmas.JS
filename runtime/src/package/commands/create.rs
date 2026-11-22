//! Create command implementation.

use color_eyre::eyre::Result;
use compact_str::CompactString;
use std::ffi::{OsStr, OsString};

use crate::package::commands::exec::{exec_with_args, install_bin_temp};
use crate::package::commands::new_path;

/// Execute the create command.
pub fn cmd_create(args: &crate::package::Options, name: &CompactString) -> Result<()> {
    let name = format!("create-{name}");
    install_bin_temp(args, &name)?;
    exec_with_args(
        OsStr::new(&name),
        &[],
        vec![(OsString::from("PATH"), new_path()?)],
    )
}
