//! Bundle a TypeScript/JavaScript project into a single `main.js` using rolldown.
//!
//! This module only exists when the `compile` feature is enabled. It drives rolldown
//! synchronously: a tokio runtime is created here, used for the bundle, then dropped.

use std::path::{Path, PathBuf};

use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat};

/// Bundle the entry module at `entry` and write `main.js` to `out_dir`.
/// Returns the path to the written file and its content as bytes.
///
/// Local imports are resolved and inlined. Specifiers starting with `node:` or
/// `xmas:` are marked external so the runtime resolves them at run time.
pub fn bundle(entry: &Path, out_dir: &Path) -> Result<(PathBuf, Vec<u8>), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async_bundle(entry, out_dir))
}

async fn async_bundle(entry: &Path, out_dir: &Path) -> Result<(PathBuf, Vec<u8>), String> {
    let entry_str = entry
        .canonicalize()
        .unwrap_or_else(|_| entry.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let cwd = entry
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));

    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;

    let external_patterns: Vec<String> = vec![
        "node:*".to_string(),
        "xmas:*".to_string(),
    ];

    let options = BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("main".to_string()),
            import: entry_str,
        }]),
        cwd: Some(cwd.clone()),
        external: Some(external_patterns.into()),
        dir: Some(out_dir.to_string_lossy().into_owned()),
        format: Some(OutputFormat::Esm),
        entry_filenames: Some("[name].js".to_string().into()),
        ..Default::default()
    };

    let mut bundler = Bundler::new(options)
        .map_err(|e| format!("create bundler: {e:?}"))?;

    let _output = bundler
        .write()
        .await
        .map_err(|e| format!("rolldown error: {e:?}"))?;

    // rolldown writes chunks to disk; find the main one and read it back
    let main_path = out_dir.join("main.js");
    let content = std::fs::read(&main_path)
        .map_err(|e| format!("read {}: {e}", main_path.display()))?;

    Ok((main_path, content))
}