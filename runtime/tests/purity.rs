//! The runtime has to build on every target without a system library behind it:
//! no OpenSSL, no system zlib, no cmake-driven crypto. A dependency that links
//! one of those is the difference between a plain build and a support ticket,
//! so the resolved tree is checked rather than trusted.

use std::process::Command;

/// Crates that link or build something from the host: openssl and its bindings,
/// the platform TLS shims, the aws-lc build (needs cmake), and the C zlib/zstd.
const FORBIDDEN: &[&str] = &[
    "openssl",
    "openssl-sys",
    "openssl-probe",
    "native-tls",
    "schannel",
    "security-framework",
    "aws-lc-rs",
    "aws-lc-sys",
    "libz-sys",
    "libz-ng-sys",
    "zstd-sys",
    "libsqlite3-sys",
];

/// The package name at the front of a cargo tree line, past its tree glyphs.
fn package_name(line: &str) -> &str {
    line.trim_start_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .split_whitespace()
        .next()
        .unwrap_or("")
}

#[test]
fn the_runtime_links_no_system_library() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = env!("CARGO_MANIFEST_DIR");
    let output = Command::new(cargo)
        .args(["tree", "--edges", "normal", "--package", "xmas-runtime"])
        .current_dir(manifest)
        .output()
        .expect("run cargo tree");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = tree
        .lines()
        .map(package_name)
        .filter(|name| FORBIDDEN.contains(name))
        .collect();

    assert!(
        found.is_empty(),
        "the runtime tree contains crates that need a system library: {found:?}\n\
         keep TLS, compression and crypto in Rust: rustls, miniz_oxide, the RustCrypto crates"
    );
}

/// Our own code, not a dependency's: no unsafe blocks, no unsafe fns. Anything
/// that needs syscalls goes through std or a crate; a raw pointer here would be
/// the start of an attack surface nobody reviews.
///
/// One file is allowed to say `unsafe`: `package/clone.rs`, which asks the kernel to clone a
/// directory tree into `node_modules` in one call. std has no API for a copy-on-write clone, and
/// the alternative measured eleven seconds against one on the tree this was written for. Naming
/// it here is what keeps the rule everywhere else: a second one anywhere fails this test.
const ALLOWED: [&str; 1] = ["package/clone.rs"];

#[test]
fn the_runtime_has_no_unsafe_of_its_own() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    walk(&root, &root, &mut offenders);
    assert!(
        offenders.is_empty(),
        "unsafe in our own sources: {offenders:?}"
    );
}

fn walk(root: &std::path::Path, directory: &std::path::Path, offenders: &mut Vec<String>) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, offenders);
            continue;
        }
        if path.extension().and_then(|kind| kind.to_str()) != Some("rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED.contains(&relative.as_str()) {
            continue;
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(body) => body,
            Err(_) => continue,
        };
        for token in ["unsafe {", "unsafe fn", "unsafe impl"] {
            if body.contains(token) {
                offenders.push(format!("{} ({token})", path.display()));
                break;
            }
        }
    }
}
