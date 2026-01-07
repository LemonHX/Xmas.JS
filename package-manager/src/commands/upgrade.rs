//! Upgrade command implementation.

use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::commands::add::add_packages;
use crate::util::read_package;

/// Execute the upgrade command.
pub async fn cmd_upgrade(pin: bool) -> Result<()> {
    let package = read_package().await?;
    add_packages(
        &package.dependencies.keys().cloned().collect_vec(),
        false,
        pin,
    )
    .await?;
    add_packages(
        &package.dev_dependencies.keys().cloned().collect_vec(),
        true,
        pin,
    )
    .await?;
    Ok(())
}
