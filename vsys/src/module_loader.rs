//! Module loader virtual table for vsys
//!
//! This module provides a pluggable module loading/resolution abstraction.
//! The module loader uses the vsys FsVTable for all filesystem operations,
//! making it fully virtualizable for sandboxed environments.
//!
//! # Design
//!
//! The module loader vtable takes a reference to the parent Vsys for all operations.
//! This allows the loader to:
//! - Use the virtual filesystem (FsVTable) for file operations
//! - Check permissions before loading modules
//! - Support custom module sources (bundled, remote, in-memory)

use std::path::{Path, PathBuf};

use crate::error::{VsysError, VsysResult};
use crate::fs::FsVTable;

/// Module format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleFormat {
    /// ECMAScript module (import/export)
    ESM,
    /// CommonJS module (require/module.exports)
    CJS,
    /// JSON file
    Json,
    /// Binary/bytecode
    Binary,
}

/// Resolved module information
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Resolved absolute path or URL
    pub path: String,
    /// Module format
    pub format: ModuleFormat,
    /// Whether this is a built-in/native module
    pub is_builtin: bool,
    /// Whether this is a CommonJS module that needs wrapping for ESM
    pub needs_cjs_wrapper: bool,
}

/// Loaded module source
#[derive(Debug, Clone)]
pub struct ModuleSource {
    /// Module source code or binary
    pub source: Vec<u8>,
    /// Module format
    pub format: ModuleFormat,
    /// Original path/URL
    pub path: String,
}

/// Module loader/resolver vtable
///
/// This provides the core module loading functionality that can be customized.
/// All functions receive a reference to `FsVTable` to perform filesystem operations,
/// ensuring the module loader respects the virtual filesystem abstraction.
///
/// # C ABI Compatibility
///
/// All function pointers use simple types and can be safely called from C.
/// The `FsVTable` pointer allows the loader to perform filesystem operations
/// through the virtual layer.
pub struct ModuleLoaderVTable {
    /// Resolve a module specifier to an absolute path
    ///
    /// # Arguments
    /// * `fs` - The filesystem vtable to use for file operations
    /// * `specifier` - The import specifier (e.g., "./foo", "lodash", "node:fs")
    /// * `referrer` - The path of the module doing the import
    /// * `is_esm` - Whether this is an ESM import (vs CommonJS require)
    ///
    /// # Returns
    /// Resolved module information or error
    pub resolve: fn(
        fs: &FsVTable,
        specifier: &str,
        referrer: &str,
        is_esm: bool,
    ) -> VsysResult<ResolvedModule>,

    /// Load a module's source code
    ///
    /// # Arguments
    /// * `fs` - The filesystem vtable to use for file operations
    /// * `path` - The resolved path from `resolve`
    ///
    /// # Returns
    /// Module source or error
    pub load: fn(fs: &FsVTable, path: &str) -> VsysResult<ModuleSource>,

    /// Check if a module exists at the given path
    ///
    /// # Arguments
    /// * `fs` - The filesystem vtable to use for file operations
    /// * `path` - The path to check
    pub exists: fn(fs: &FsVTable, path: &str) -> bool,

    /// Check if a specifier is a built-in module
    pub is_builtin: fn(specifier: &str) -> bool,

    /// List all built-in module names
    pub list_builtins: fn() -> Vec<String>,

    /// Find the closest package.json from a directory
    ///
    /// # Arguments
    /// * `fs` - The filesystem vtable to use for file operations  
    /// * `start_dir` - The directory to start searching from
    ///
    /// # Returns
    /// Path to package.json if found
    pub find_package_json: fn(fs: &FsVTable, start_dir: &str) -> Option<String>,

    /// Read and parse package.json
    ///
    /// # Arguments
    /// * `fs` - The filesystem vtable to use for file operations
    /// * `path` - Path to package.json
    ///
    /// # Returns
    /// Parsed package.json as JSON value
    pub read_package_json: fn(fs: &FsVTable, path: &str) -> VsysResult<serde_json::Value>,
}

impl Default for ModuleLoaderVTable {
    fn default() -> Self {
        Self {
            resolve: default_resolve,
            load: default_load,
            exists: default_exists,
            is_builtin: default_is_builtin,
            list_builtins: default_list_builtins,
            find_package_json: default_find_package_json,
            read_package_json: default_read_package_json,
        }
    }
}

impl ModuleLoaderVTable {
    /// Create a loader that only allows built-in modules
    pub fn builtins_only() -> Self {
        Self {
            resolve: builtins_only_resolve,
            load: builtins_only_load,
            exists: |_, _| false,
            is_builtin: default_is_builtin,
            list_builtins: default_list_builtins,
            find_package_json: |_, _| None,
            read_package_json: |_, _| {
                Err(VsysError::ModuleResolution {
                    specifier: String::new(),
                    message: "Filesystem access not allowed".to_string(),
                })
            },
        }
    }
}

// Supported file extensions
const JS_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs"];
#[allow(dead_code)]
const TS_EXTENSIONS: &[&str] = &[".ts", ".mts", ".cts", ".tsx", ".jsx"];
const ALL_EXTENSIONS: &[&str] = &[
    ".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".tsx", ".jsx", ".json",
];

// Built-in modules (node: prefix)
const BUILTIN_MODULES: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "dns",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

fn default_is_builtin(specifier: &str) -> bool {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    BUILTIN_MODULES.contains(&name)
}

fn default_list_builtins() -> Vec<String> {
    BUILTIN_MODULES.iter().map(|s| s.to_string()).collect()
}

fn default_resolve(
    fs: &FsVTable,
    specifier: &str,
    referrer: &str,
    is_esm: bool,
) -> VsysResult<ResolvedModule> {
    // Handle node: prefix
    if specifier.starts_with("node:") || default_is_builtin(specifier) {
        let name = specifier.strip_prefix("node:").unwrap_or(specifier);
        return Ok(ResolvedModule {
            path: name.to_string(),
            format: ModuleFormat::ESM,
            is_builtin: true,
            needs_cjs_wrapper: false,
        });
    }

    // Handle file:// URLs
    let specifier = specifier.strip_prefix("file://").unwrap_or(specifier);

    // Determine if relative or bare specifier
    let is_relative =
        specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/');

    if is_relative {
        // Resolve relative to referrer
        let referrer_path = Path::new(referrer);
        let base_dir = referrer_path.parent().unwrap_or(Path::new("."));
        let resolved = base_dir.join(specifier);

        // Try to resolve with extensions
        if let Some((path, format, is_cjs)) = try_resolve_file(fs, &resolved, is_esm) {
            return Ok(ResolvedModule {
                path: path.to_string_lossy().into_owned(),
                format,
                is_builtin: false,
                needs_cjs_wrapper: is_cjs && is_esm,
            });
        }

        return Err(VsysError::ModuleResolution {
            specifier: specifier.to_string(),
            message: format!("Cannot find module '{}'", specifier),
        });
    }

    // Bare specifier - try node_modules resolution
    if let Some((path, format, is_cjs)) = try_resolve_node_modules(fs, specifier, referrer, is_esm)
    {
        return Ok(ResolvedModule {
            path: path.to_string_lossy().into_owned(),
            format,
            is_builtin: false,
            needs_cjs_wrapper: is_cjs && is_esm,
        });
    }

    Err(VsysError::ModuleResolution {
        specifier: specifier.to_string(),
        message: format!("Cannot find package '{}'", specifier),
    })
}

/// Check if a path is a file using the virtual fs
fn is_file(fs: &FsVTable, path: &Path) -> bool {
    (fs.is_file)(path)
}

/// Check if a path is a directory using the virtual fs
fn is_dir(fs: &FsVTable, path: &Path) -> bool {
    (fs.is_dir)(path)
}

/// Check if a path exists using the virtual fs
fn path_exists(fs: &FsVTable, path: &Path) -> bool {
    (fs.exists)(path)
}

fn try_resolve_file(
    fs: &FsVTable,
    path: &Path,
    _is_esm: bool,
) -> Option<(PathBuf, ModuleFormat, bool)> {
    // Try exact path
    if is_file(fs, path) {
        let format = detect_format(path);
        let is_cjs = matches!(format, ModuleFormat::CJS);
        return Some((path.to_path_buf(), format, is_cjs));
    }

    // Try with extensions
    for ext in ALL_EXTENSIONS {
        let with_ext = path.with_extension(&ext[1..]); // Remove leading dot
        if is_file(fs, &with_ext) {
            let format = detect_format(&with_ext);
            let is_cjs = matches!(format, ModuleFormat::CJS);
            return Some((with_ext, format, is_cjs));
        }
    }

    // Try as directory with index
    if is_dir(fs, path) {
        for ext in ALL_EXTENSIONS {
            let index = path.join(format!("index{}", ext));
            if is_file(fs, &index) {
                let format = detect_format(&index);
                let is_cjs = matches!(format, ModuleFormat::CJS);
                return Some((index, format, is_cjs));
            }
        }
    }

    None
}

fn try_resolve_node_modules(
    fs: &FsVTable,
    specifier: &str,
    referrer: &str,
    is_esm: bool,
) -> Option<(PathBuf, ModuleFormat, bool)> {
    let referrer_path = Path::new(referrer);
    let mut current = referrer_path.parent();

    while let Some(dir) = current {
        let node_modules = dir.join("node_modules").join(specifier);

        // Try package.json main field
        let package_json = node_modules.join("package.json");
        if is_file(fs, &package_json) {
            if let Ok(content) = (fs.read)(&package_json) {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&content) {
                    // Determine if CJS based on type field
                    let is_cjs = json
                        .get("type")
                        .and_then(|t| t.as_str())
                        .map(|t| t != "module")
                        .unwrap_or(true);

                    // Try "exports", "module", "main" fields in order
                    let main_field = if is_esm {
                        json.get("exports")
                            .and_then(|e| e.get("."))
                            .and_then(|d| d.get("import"))
                            .or_else(|| json.get("module"))
                            .or_else(|| json.get("main"))
                            .and_then(|v| v.as_str())
                    } else {
                        json.get("exports")
                            .and_then(|e| e.get("."))
                            .and_then(|d| d.get("require"))
                            .or_else(|| json.get("main"))
                            .and_then(|v| v.as_str())
                    };

                    if let Some(main) = main_field {
                        let main_path = node_modules.join(main);
                        if let Some((resolved, format, _)) =
                            try_resolve_file(fs, &main_path, is_esm)
                        {
                            return Some((resolved, format, is_cjs));
                        }
                    }

                    // Try index.js as fallback
                    for ext in JS_EXTENSIONS {
                        let index = node_modules.join(format!("index{}", ext));
                        if is_file(fs, &index) {
                            let format = detect_format(&index);
                            return Some((index, format, is_cjs));
                        }
                    }
                }
            }
        }

        // Try direct file resolution
        if let Some(resolved) = try_resolve_file(fs, &node_modules, is_esm) {
            return Some(resolved);
        }

        current = dir.parent();
    }

    None
}

fn detect_format(path: &Path) -> ModuleFormat {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "mjs" | "mts" => ModuleFormat::ESM,
        "cjs" | "cts" => ModuleFormat::CJS,
        "json" => ModuleFormat::Json,
        "js" | "ts" | "tsx" | "jsx" => {
            // Default to ESM for now
            // In production, should check package.json type field
            ModuleFormat::ESM
        }
        _ => ModuleFormat::Binary,
    }
}

fn default_load(fs: &FsVTable, path: &str) -> VsysResult<ModuleSource> {
    // Built-in modules are handled separately
    if default_is_builtin(path) {
        return Err(VsysError::ModuleLoad {
            path: path.to_string(),
            message: "Built-in modules should be loaded by the runtime".to_string(),
        });
    }

    let path_obj = Path::new(path);
    let source = (fs.read)(path_obj)?;
    let format = detect_format(path_obj);

    Ok(ModuleSource {
        source,
        format,
        path: path.to_string(),
    })
}

fn default_exists(fs: &FsVTable, path: &str) -> bool {
    path_exists(fs, Path::new(path))
}

fn default_find_package_json(fs: &FsVTable, start_dir: &str) -> Option<String> {
    let mut current_dir = PathBuf::from(start_dir);
    loop {
        let package_json_path = current_dir.join("package.json");
        if path_exists(fs, &package_json_path) {
            return Some(package_json_path.to_string_lossy().into_owned());
        }
        if !current_dir.pop() {
            break;
        }
    }
    None
}

fn default_read_package_json(fs: &FsVTable, path: &str) -> VsysResult<serde_json::Value> {
    let path_obj = Path::new(path);
    let content = (fs.read)(path_obj)?;
    serde_json::from_slice(&content).map_err(|e| VsysError::ModuleLoad {
        path: path.to_string(),
        message: format!("Failed to parse package.json: {}", e),
    })
}

fn builtins_only_resolve(
    fs: &FsVTable,
    specifier: &str,
    _referrer: &str,
    _is_esm: bool,
) -> VsysResult<ResolvedModule> {
    let _ = fs; // unused in builtins-only mode
    if default_is_builtin(specifier) {
        let name = specifier.strip_prefix("node:").unwrap_or(specifier);
        return Ok(ResolvedModule {
            path: name.to_string(),
            format: ModuleFormat::ESM,
            is_builtin: true,
            needs_cjs_wrapper: false,
        });
    }

    Err(VsysError::ModuleResolution {
        specifier: specifier.to_string(),
        message: "Only built-in modules are allowed".to_string(),
    })
}

fn builtins_only_load(fs: &FsVTable, path: &str) -> VsysResult<ModuleSource> {
    let _ = fs; // unused in builtins-only mode
    Err(VsysError::ModuleLoad {
        path: path.to_string(),
        message: "Only built-in modules are allowed".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FsVTable;

    #[test]
    fn test_is_builtin() {
        assert!(default_is_builtin("fs"));
        assert!(default_is_builtin("node:fs"));
        assert!(default_is_builtin("path"));
        assert!(!default_is_builtin("lodash"));
        assert!(!default_is_builtin("./foo"));
    }

    #[test]
    fn test_resolve_builtin() {
        let vtable = ModuleLoaderVTable::default();
        let fs = FsVTable::default();
        let result = (vtable.resolve)(&fs, "node:fs", "/app/index.js", true).unwrap();
        assert!(result.is_builtin);
        assert_eq!(result.path, "fs");
    }

    #[test]
    fn test_builtins_only() {
        let vtable = ModuleLoaderVTable::builtins_only();
        let fs = FsVTable::default();

        // Built-in should work
        let result = (vtable.resolve)(&fs, "fs", "/app/index.js", true);
        assert!(result.is_ok());

        // Non-builtin should fail
        let result = (vtable.resolve)(&fs, "./foo", "/app/index.js", true);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(detect_format(Path::new("foo.mjs")), ModuleFormat::ESM);
        assert_eq!(detect_format(Path::new("foo.cjs")), ModuleFormat::CJS);
        assert_eq!(detect_format(Path::new("foo.json")), ModuleFormat::Json);
        assert_eq!(detect_format(Path::new("foo.js")), ModuleFormat::ESM);
    }

    #[test]
    fn test_resolved_module_cjs_wrapper() {
        let resolved = ResolvedModule {
            path: "/app/lib.cjs".to_string(),
            format: ModuleFormat::CJS,
            is_builtin: false,
            needs_cjs_wrapper: true,
        };
        assert!(resolved.needs_cjs_wrapper);
    }
}
