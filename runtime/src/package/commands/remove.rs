//! Remove command implementation.

use color_eyre::eyre::{eyre, ContextCompat, Result};
use compact_str::CompactString;
use serde_json::Value;

use super::install;
use crate::package::progress::{log_progress, PROGRESS_BAR};
use crate::package::util::{read_package_or_default, save_package};
use crate::package::Options;

/// Execute the remove command.
///
/// Like `add`, this is npm's `remove`: taking a name out of package.json leaves whatever it
/// brought in on disk, so the tree is reinstalled to drop what nothing asks for any more.
pub fn cmd_remove(names: &[CompactString], dev: bool, options: &Options) -> Result<()> {
    if names.is_empty() {
        PROGRESS_BAR.suspend(|| println!("Note: no packages specified"));
    }

    let mut package: Value = read_package_or_default()?;
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

    save_package(&package)?;

    if names.is_empty() {
        return Ok(());
    }
    install::install(options)
}
