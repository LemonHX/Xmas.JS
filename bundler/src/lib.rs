//! Bundler module powered by Rolldown
//!
//! Rolldown is a fast Rust-based bundler that's Rollup-compatible and designed for Vite.
//! It provides 10-30x faster bundling than Rollup with full plugin ecosystem support.
//!
//! Features:
//! - Fast Rust-based bundling
//! - Rollup-compatible API
//! - Built-in minification
//! - Tree-shaking
//! - Code splitting
//! - Source maps

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use thiserror::Error;

/// Errors that can occur during bundling
#[derive(Error, Debug)]
pub enum BundleError {
    #[error("Bundling failed: {0}")]
    BundleFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Rolldown feature not enabled")]
    FeatureNotEnabled,
}

/// Result type for bundler operations
pub type BundleResult<T> = Result<T, BundleError>;

/// Configuration for the bundler
#[derive(Debug, Clone, Parser)]
#[command(name = "bundle", about = "Bundle TypeScript/JavaScript files")]
pub struct BundleConfig {
    /// Entry point(s) for the bundle
    #[arg(required = true)]
    pub entry: Vec<PathBuf>,

    /// Output directory
    #[arg(short = 'o', long, default_value = "dist")]
    pub output_dir: PathBuf,

    /// Output filename
    #[arg(short = 'n', long)]
    pub output_filename: Option<String>,

    /// Enable minification
    #[arg(short = 'm', long)]
    pub minify: bool,

    /// Enable source maps
    #[arg(short = 's', long)]
    pub source_map: bool,

    /// Target format (esm, cjs, iife)
    #[arg(short = 'f', long, default_value = "esm")]
    pub format: BundleFormat,

    /// Enable tree-shaking
    #[arg(long, default_value = "true")]
    pub tree_shake: bool,

    /// External modules (won't be bundled)
    #[arg(short = 'e', long)]
    pub external: Vec<String>,
}

/// Bundle output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum BundleFormat {
    /// ES Module format
    #[default]
    Esm,
    /// CommonJS format
    Cjs,
    /// Immediately Invoked Function Expression
    Iife,
}

impl Default for BundleConfig {
    fn default() -> Self {
        Self {
            entry: Vec::new(),
            output_dir: PathBuf::from("dist"),
            output_filename: None,
            minify: false,
            source_map: false,
            format: BundleFormat::Esm,
            tree_shake: true,
            external: Vec::new(),
        }
    }
}

/// Bundle TypeScript/JavaScript files using Rolldown
pub async fn bundle(config: BundleConfig) -> BundleResult<()> {
    use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat};

    // Convert entry points to InputItem
    let input_items: Vec<InputItem> = config
        .entry
        .iter()
        .enumerate()
        .map(|(idx, path)| {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&format!("entry{}", idx))
                .to_string();

            InputItem {
                name: Some(name),
                import: path.to_string_lossy().to_string(),
            }
        })
        .collect();

    // Convert format
    let output_format = match config.format {
        BundleFormat::Esm => OutputFormat::Esm,
        BundleFormat::Cjs => OutputFormat::Cjs,
        BundleFormat::Iife => OutputFormat::Iife,
    };

    // Create bundler with options
    let bundler = Bundler::new(BundlerOptions {
        input: Some(input_items),
        dir: Some(config.output_dir.to_string_lossy().to_string()),
        format: Some(output_format),
        minify: Some(rolldown::RawMinifyOptions::Bool(config.minify)),
        sourcemap: config.source_map.then(|| rolldown::SourceMapType::File),
        external: if config.external.is_empty() {
            None
        } else {
            Some(rolldown::IsExternal::from(config.external.clone()))
        },
        ..Default::default()
    });

    // Run bundler
    let output = bundler
        .map_err(|e| BundleError::BundleFailed(e.to_string()))?
        .write()
        .await
        .map_err(|e| BundleError::BundleFailed(format!("Rolldown bundling failed: {:?}", e)))?;

    for w in output.warnings {
        eprintln!("Warning: {}", w);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BundleConfig::default();
        assert_eq!(config.format, BundleFormat::Esm);
        assert!(config.tree_shake);
        assert!(!config.minify);
    }
}
