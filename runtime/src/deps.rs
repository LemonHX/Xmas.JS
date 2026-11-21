//! A bare specifier that is not installed yet.
//!
//! `import` goes through crate::resolve, which only looks at what is already on disk. When nothing is
//! there, the import is asking for a package nobody installed: this decides which package that is,
//! where its node_modules belongs, and asks the package manager for it. The loader then resolves one
//! more time, and the whole thing costs a download once and a directory lookup afterwards.
//!
//! The package manager is in this process already (the CLI is both halves), so nothing has to be
//! spawned and nothing leaves the machine except the registry request. See docs/modules.md.

use std::path::{Path, PathBuf};

/// What asking the package manager for a missing specifier came to.
pub enum Outcome {
    /// It is on disk now; resolve the specifier again.
    Installed,
    /// This specifier does not name a package, so there is nothing to install.
    NotAPackage,
    /// It named a package and installing it failed, for the reason inside.
    Failed(String),
}

/// Install what `specifier`, imported by `referrer`, names.
pub fn fetch(specifier: &str, referrer: &str) -> Outcome {
    let package = match package_of(specifier) {
        Some(package) => package,
        None => return Outcome::NotAPackage,
    };
    let root = match project_root(referrer) {
        Some(root) => root,
        None => {
            return Outcome::Failed(format!(
                "installing '{package}' would need a package.json above {referrer}, and there is none"
            ))
        }
    };
    // An import that downloads something is worth one line: it explains a pause and a network call
    // the program did not mention anywhere.
    eprintln!("xmas: installing {package} for '{specifier}'");
    match xmas_package_manager::autoload::install_package(&root, &package) {
        Ok(()) => Outcome::Installed,
        Err(error) => Outcome::Failed(format!("{error:#}")),
    }
}

/// The package a bare specifier names: `ms` and `ms/index.js` are `ms`, `@scope/pkg/x` is
/// `@scope/pkg`.
pub fn package_of(specifier: &str) -> Option<String> {
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with("node:")
        || specifier.starts_with('#')
        || specifier.contains(':')
    {
        return None;
    }
    let mut parts = specifier.split('/');
    let first = parts.next()?;
    if first.starts_with('@') {
        let second = parts.next()?;
        Some(format!("{first}/{second}"))
    } else {
        Some(first.to_owned())
    }
}

/// The nearest directory at or above the importer that has a package.json: where node_modules is.
pub fn project_root(referrer: &str) -> Option<PathBuf> {
    let mut directory = Path::new(referrer).parent()?;
    loop {
        if directory.join("package.json").is_file() {
            return Some(directory.to_path_buf());
        }
        directory = directory.parent()?;
    }
}
