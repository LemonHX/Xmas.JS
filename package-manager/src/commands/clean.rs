//! Clean command implementation.

use color_eyre::eyre::Result;
use std::fs::remove_dir_all;
use std::io::ErrorKind;

/// Execute the clean command.
pub fn cmd_clean() -> Result<()> {
    for dir in ["node_modules", ".xmas"] {
        match remove_dir_all(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            r => r?,
        }
    }
    Ok(())
}
