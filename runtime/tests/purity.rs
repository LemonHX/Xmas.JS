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
