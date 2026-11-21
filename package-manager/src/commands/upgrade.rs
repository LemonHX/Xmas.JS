//! Upgrade command implementation.

use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::commands::add::add_packages;
use crate::util::read_package;

/// Execute the upgrade command.
pub fn cmd_upgrade(pin: bool) -> Result<()> {
    let package = read_package()?;
    add_packages(
        &package.dependencies.keys().cloned().collect_vec(),
        false,
        pin,
    )?;
    add_packages(
        &package.dev_dependencies.keys().cloned().collect_vec(),
        true,
        pin,
    )?;
    Ok(())
}
