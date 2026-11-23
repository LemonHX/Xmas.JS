//! Add command implementation.

use color_eyre::eyre::{ContextCompat, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::CompactString;
use serde_json::Value;

use super::install;
use crate::package::npm::fetch_package;
use crate::package::progress::PROGRESS_BAR;
use crate::package::util::{read_package_or_default, save_package};
use crate::package::Options;

/// Execute the add command.
///
/// Adding a package means putting it on disk, not only naming it in package.json: a manifest that
/// lists something node_modules does not have is a broken tree, and asking for it twice is the
/// kind of thing nobody does. So this is npm's `add`: write the field, then install.
pub fn cmd_add(names: &[CompactString], dev: bool, pin: bool, options: &Options) -> Result<()> {
    if names.is_empty() {
        PROGRESS_BAR.suspend(|| println!("Note: no packages specified"));
    }

    add_packages(names, dev, pin)?;
    if names.is_empty() {
        return Ok(());
    }
    install::install(options)
}

/// Add packages to package.json.
pub fn add_packages(names: &[CompactString], dev: bool, pin: bool) -> Result<()> {
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

    PROGRESS_BAR.set_message("Resolving packages".to_string());
    PROGRESS_BAR.set_length(names.len() as u64);

    for name in names.iter() {
        let res = smol::block_on(fetch_package(name))?;
        PROGRESS_BAR.inc(1);
        PROGRESS_BAR.set_message(format!("Resolved {name}"));

        let latest = res
            .dist_tags
            .get("latest")
            .wrap_err("Package `latest` tag not specified")?;

        let version = if pin {
            latest.to_string()
        } else {
            format!("^{latest}")
        };

        dependencies.insert(name.to_string(), Value::String(version.to_string()));

        PROGRESS_BAR.suspend(|| println!("Added {} {}", name.yellow(), version.yellow()));
    }

    PROGRESS_BAR.finish_and_clear();
    save_package(&package)?;

    Ok(())
}
