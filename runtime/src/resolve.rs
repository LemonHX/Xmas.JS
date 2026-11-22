//! Where an `import` points.
//!
//! The engine hands the loader a referrer and the specifier as written, and asks for a name; this is
//! the whole of the rule that turns the second into a path on disk. Node's rules, cut down to what a
//! server app needs: relative and absolute paths, extension probing, and `node_modules` with a
//! package's `exports`, `module` or `main`.
//!
//! Nothing here runs JavaScript and nothing here is asynchronous: a specifier either names a file
//! that exists or it does not, and the answer is the canonical path of that file.

use std::path::{Path, PathBuf};

/// What a specifier without an extension is probed with, in order. TypeScript first, because this
/// runtime compiles `.ts` and `.tsx` and a project written in types should not have to say so.
const EXTENSIONS: [&str; 8] = ["ts", "tsx", "js", "jsx", "mts", "mjs", "cjs", "json"];

/// What a directory import falls back to when its package.json says nothing useful.
const INDICES: [&str; 8] = [
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "index.mts",
    "index.mjs",
    "index.cjs",
    "index.json",
];

/// Which door a specifier came through. `import` prefers a package's ESM entry; `require` takes the
/// conditions and the entry field node gives a CommonJS caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Import,
    Require,
}

/// Resolve `specifier`, imported by the module named `referrer`, to the canonical path of a file.
pub fn resolve(referrer: &str, specifier: &str) -> Result<String, String> {
    resolve_as(referrer, specifier, Mode::Import)
}

/// `resolve` for a `require()` call.
pub fn resolve_require(referrer: &str, specifier: &str) -> Result<String, String> {
    resolve_as(referrer, specifier, Mode::Require)
}

fn resolve_as(referrer: &str, specifier: &str, mode: Mode) -> Result<String, String> {
    if specifier.is_empty() {
        return Err(format!("an empty specifier, imported by {referrer}"));
    }
    // The modules this runtime ships answer to both spellings and are already in the table, so a
    // 'node:' name that reaches here is one it does not have.
    if let Some(rest) = specifier.strip_prefix("node:") {
        return Err(format!(
            "'node:{rest}' is not a module this runtime ships (see docs/modules.md)"
        ));
    }

    if is_relative(specifier) {
        let base = directory(referrer);
        let candidate = base.join(specifier);
        return probe(&candidate, mode)
            .ok_or_else(|| format!("cannot find '{specifier}' from {referrer}"));
    }

    if is_absolute(specifier) {
        return probe(Path::new(specifier), mode).ok_or_else(|| format!("cannot find '{specifier}'"));
    }

    // Bare: the package comes from the nearest node_modules, the way node walks up.
    let (package, subpath) = split_bare(specifier);
    let mut current = Some(directory(referrer));
    while let Some(dir) = current {
        let root = dir.join("node_modules").join(package);
        if root.is_dir() {
            if let Some(target) = exports_target(&root, subpath, mode) {
                if let Some(found) = probe(&root.join(target), mode) {
                    return Ok(found);
                }
            }
            if subpath.is_empty() {
                if let Some(target) = package_entry(&root, mode) {
                    if let Some(found) = probe(&root.join(target), mode) {
                        return Ok(found);
                    }
                }
                if let Some(found) = index(&root) {
                    return Ok(found);
                }
            } else if let Some(found) = probe(&root.join(subpath), mode) {
                return Ok(found);
            }
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    Err(format!(
        "cannot find package '{specifier}' from {referrer} (no node_modules with it)"
    ))
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

fn is_absolute(specifier: &str) -> bool {
    let bytes = specifier.as_bytes();
    matches!(bytes.first(), Some(b'/') | Some(b'\\'))
        || (bytes.len() > 2 && bytes[1] == b':' && matches!(bytes[0], b'A'..=b'Z' | b'a'..=b'z'))
}

/// The directory a referrer's relative imports are relative to. A referrer that is only a name
/// (a module installed under one) resolves against the working directory.
fn directory(referrer: &str) -> PathBuf {
    match Path::new(referrer).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// A bare specifier's package and the rest of it: `@scope/name/sub` is `("@scope/name", "sub")`.
fn split_bare(specifier: &str) -> (&str, &str) {
    let mut parts = specifier.splitn(2, '/');
    let first = parts.next().unwrap_or(specifier);
    let rest = parts.next().unwrap_or("");
    if first.starts_with('@') {
        match rest.split_once('/') {
            Some((second, tail)) => {
                let end = first.len() + 1 + second.len();
                (&specifier[..end], tail)
            }
            None => (specifier, ""),
        }
    } else {
        (first, rest)
    }
}

/// The canonical path of a file `candidate` names, if one exists.
///
/// A file is a file; a directory is its package entry or its index; anything else is probed with the
/// extensions above, including the TypeScript source of a `.js` specifier, which is how a project
/// written in types spells its own imports.
fn probe(candidate: &Path, mode: Mode) -> Option<String> {
    if candidate.is_file() {
        return canonical(candidate);
    }
    if candidate.is_dir() {
        if let Some(target) = package_entry(candidate, mode) {
            if let Some(found) = probe(&candidate.join(target), mode) {
                return Some(found);
            }
        }
        if let Some(found) = index(candidate) {
            return Some(found);
        }
        return None;
    }

    let extension = candidate
        .extension()
        .and_then(|text| text.to_str())
        .unwrap_or("");
    for paired in paired_sources(extension) {
        // './name.js' is how a project written in types spells its own source: the .js is replaced,
        // not appended to.
        let path = candidate.with_extension(paired);
        if path.is_file() {
            if let Some(found) = canonical(&path) {
                return Some(found);
            }
        }
    }
    for extension in EXTENSIONS {
        if let Some(found) = probe_with(candidate, extension) {
            return Some(found);
        }
    }
    None
}

/// The TypeScript sources a compiled-looking extension may have been written as.
fn paired_sources(extension: &str) -> &'static [&'static str] {
    match extension {
        "js" => &["ts", "tsx"],
        "jsx" => &["tsx"],
        "mjs" => &["mts"],
        "cjs" => &["cts"],
        _ => &[],
    }
}

fn probe_with(candidate: &Path, extension: &str) -> Option<String> {
    let mut path = candidate.as_os_str().to_owned();
    path.push(".");
    path.push(extension);
    let path = PathBuf::from(path);
    if path.is_file() {
        canonical(&path)
    } else {
        None
    }
}

fn index(dir: &Path) -> Option<String> {
    INDICES.iter().find_map(|name| {
        let path = dir.join(name);
        if path.is_file() {
            canonical(&path)
        } else {
            None
        }
    })
}

fn canonical(path: &Path) -> Option<String> {
    let absolute = std::fs::canonicalize(path).ok()?;
    // A specifier reaches JavaScript as a string, where a backslash is an escape: windows paths are
    // spelled with forward slashes everywhere above this layer.
    Some(absolute.to_string_lossy().replace('\\', "/"))
}

/// The file a package's `exports` field names for `subpath`, relative to the package directory.
fn exports_target(root: &Path, subpath: &str, mode: Mode) -> Option<PathBuf> {
    let manifest = manifest(root)?;
    let exports = manifest.get("exports")?;
    let key = if subpath.is_empty() {
        ".".to_owned()
    } else {
        format!("./{subpath}")
    };
    let mut target: &serde_json::Value = match exports {
        serde_json::Value::String(_) if subpath.is_empty() => exports,
        serde_json::Value::Object(map) => map.get(&key)?,
        _ => return None,
    };
    // Conditions: the ones this runtime can honestly claim to be.
    loop {
        match target {
            serde_json::Value::String(target) => {
                let relative = target.strip_prefix("./")?;
                return Some(PathBuf::from(relative));
            }
            serde_json::Value::Object(map) => {
                let conditions: &[&str] = match mode {
                    Mode::Import => &["import", "node", "default", "require"],
                    Mode::Require => &["node", "require", "default"],
                };
                target = conditions
                    .iter()
                    .find_map(|name| map.get(*name))?;
            }
            _ => return None,
        }
    }
}

/// The old-style entry: `module` then `main`.
fn package_entry(root: &Path, mode: Mode) -> Option<PathBuf> {
    let manifest = manifest(root)?;
    let fields: &[&str] = match mode {
        Mode::Import => &["module", "main"],
        Mode::Require => &["main"],
    };
    for field in fields {
        if let Some(serde_json::Value::String(target)) = manifest.get(*field) {
            return Some(PathBuf::from(target));
        }
    }
    None
}

fn manifest(root: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_specifier_splits_into_package_and_subpath() {
        assert_eq!(split_bare("react"), ("react", ""));
        assert_eq!(split_bare("react/jsx-runtime"), ("react", "jsx-runtime"));
        assert_eq!(split_bare("@scope/name"), ("@scope/name", ""));
        assert_eq!(split_bare("@scope/name/sub/x"), ("@scope/name", "sub/x"));
    }

    #[test]
    fn windows_and_absolute_spellings_are_told_apart() {
        assert!(is_absolute("/home/x/app.ts"));
        assert!(is_absolute("C:/x/app.ts"));
        assert!(!is_absolute("./x.ts"));
        assert!(!is_relative("x.ts"));
        assert!(is_relative("../x.ts"));
    }
}
