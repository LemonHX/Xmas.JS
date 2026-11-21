//! A bare import of a package nobody installed: the loader asks the package manager for it, and the
//! import that failed then resolves and loads.
//!
//! The test that downloads a package needs the npm registry, so it is ignored by default:
//!
//!     cargo test -p xmas-runtime --test autoload -- --ignored

use std::fs::create_dir_all;

use xmas_runtime::testkit::Harness;

/// A project directory of our own, so the install lands somewhere disposable.
fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("xmas-autoload-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    create_dir_all(root.join("node_modules")).expect("project directory");
    std::fs::write(
        root.join("package.json"),
        "{\"name\": \"autoload-test\", \"private\": true}\n",
    )
    .expect("package.json");
    root
}

#[test]
#[ignore = "needs the npm registry"]
fn an_import_of_a_package_nobody_installed_installs_it() {
    let root = project("install");
    let entry = root.join("app.ts");
    std::fs::write(
        &entry,
        "import { nanoid } from 'nanoid';\nexport const id = nanoid();\n",
    )
    .expect("app.ts");

    let mut h = Harness::with_modules();
    h.import_file(&entry).expect("import");
    let id = h.expect_text("String(globalThis.__xmas_entry.id)");

    assert_eq!(id.len(), 21, "nanoid gives 21 characters: {id:?}");
    assert!(
        root.join("node_modules/nanoid/package.json").is_file(),
        "the package is on disk"
    );
    // A run does not decide what the project depends on: package.json is as it was.
    let manifest = std::fs::read_to_string(root.join("package.json")).expect("package.json");
    assert!(!manifest.contains("nanoid"), "package.json is untouched: {manifest}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_package_in_a_project_with_no_manifest_says_so() {
    let root = std::env::temp_dir().join(format!("xmas-autoload-bare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    create_dir_all(&root).expect("directory");
    let entry = root.join("app.ts");
    std::fs::write(&entry, "import left_pad_that_is_not_here from 'left-pad';\n").expect("app.ts");

    let mut h = Harness::with_modules();
    let message = h.import_file(&entry).expect_err("no package.json above it");
    assert!(
        message.contains("package.json"),
        "the reason names what is missing: {message}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_relative_import_that_is_missing_does_not_install_anything() {
    let root = project("relative");
    let entry = root.join("app.ts");
    std::fs::write(&entry, "import './nothing-here.ts';\n").expect("app.ts");

    let mut h = Harness::with_modules();
    let message = h.import_file(&entry).expect_err("the file is not there");
    assert!(
        message.contains("nothing-here"),
        "the reason names the file: {message}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
