//! Remove command implementation.

use color_eyre::eyre::{eyre, ContextCompat, Result};
use compact_str::CompactString;
use serde_json::Value;

use crate::progress::{log_progress, PROGRESS_BAR};
use crate::util::{read_package_or_default, save_package};

/// Execute the remove command.
pub async fn cmd_remove(names: &[CompactString], dev: bool) -> Result<()> {
    if names.is_empty() {
        PROGRESS_BAR.suspend(|| println!("Note: no packages specified"));
    }

    let mut package: Value = read_package_or_default().await?;
    let dependencies = package
        .as_object_mut()
        .wrap_err("`package.json` is invalid")?
        .entry(if dev {
            "devDependencies"
        } else {
            "dependencies"
        })
        .or_insert(Value::Object(Default::default()))
        .as_object_mut()
        .wrap_err("`package.json` contains non-object dependencies field")?;

    for name in names {
        dependencies
            .remove(&name.to_string())
            .wrap_err(eyre!("Package `{name}` is not specified in `package.json`"))?;
    }

    log_progress(&format!("Removed {} dependencies", names.len()));

    save_package(&package).await?;

    Ok(())
}
