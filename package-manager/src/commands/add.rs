//! Add command implementation.

use color_eyre::eyre::{ContextCompat, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::CompactString;
use serde_json::Value;

use crate::npm::fetch_package;
use crate::progress::PROGRESS_BAR;
use crate::util::{read_package_or_default, save_package};

/// Execute the add command.
pub fn cmd_add(names: &[CompactString], dev: bool, pin: bool) -> Result<()> {
    if names.is_empty() {
        PROGRESS_BAR.suspend(|| println!("Note: no packages specified"));
    }

    add_packages(names, dev, pin)
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
