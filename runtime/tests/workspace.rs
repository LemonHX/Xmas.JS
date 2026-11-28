//! Workspace packages reachable from a bare specifier.
//!
//! bun and pnpm link every package of the workspace into node_modules at install time, and that
//! link is what makes a subpath like `nsh/test` resolve through the package's `exports`. The
//! resolver's fallback (find the workspace root, find the package by name) is a guess: it reads the
//! subpath as a path, so a package whose `exports` maps `./test` somewhere else is not found.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use xmas_runtime::package::commands::link_workspaces;
use xmas_runtime::testkit::Harness;

/// Run `f` with the process in `dir`: install and the resolver read paths relative to it.
fn in_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    static ONCE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _once = ONCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::current_dir().expect("working directory");
    std::env::set_current_dir(dir).expect("step into the project");
    let outcome = f();
    std::env::set_current_dir(previous).expect("step back");
    outcome
}

fn project(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "xmas-workspace-{name}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("packages")).expect("project directory");
    std::fs::create_dir_all(root.join("node_modules")).expect("node_modules");
    root
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent directory");
    }
    std::fs::write(path, text).expect("write file");
}

/// A workspace with one package, `nsh`, whose `./test` subpath is not where the subpath says it is.
fn manifest(root: &Path) {
    write(
        &root.join("package.json"),
        "{\"name\": \"app\", \"private\": true, \"workspaces\": [\"packages/*\"]}\n",
    );
    write(
        &root.join("packages/nsh/package.json"),
        "{\"name\": \"nsh\", \"exports\": {\".\": \"./src/index.ts\", \"./test\": \"./src/testing/entry.ts\"}}\n",
    );
    write(
        &root.join("packages/nsh/src/index.ts"),
        "export const nsh = 1;\n",
    );
    write(
        &root.join("packages/nsh/src/testing/entry.ts"),
        "export const describe = () => 'described';\n",
    );
}

#[test]
fn a_workspace_package_is_linked_into_node_modules() {
    let root = project("link");
    manifest(&root);
    let linked = in_dir(&root, link_workspaces).expect("link");
    assert_eq!(linked.len(), 1, "one package: {linked:?}");

    let link = root.join("node_modules/nsh");
    let target = std::fs::canonicalize(link).expect("the link points somewhere real");
    assert_eq!(
        target,
        std::fs::canonicalize(root.join("packages/nsh")).unwrap()
    );
    assert!(
        std::fs::symlink_metadata(root.join("node_modules/nsh"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the package is a link, not a copy"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn linking_twice_changes_nothing() {
    let root = project("twice");
    manifest(&root);
    in_dir(&root, link_workspaces).expect("first link");
    let before = std::fs::read_link(root.join("node_modules/nsh")).expect("link");
    in_dir(&root, link_workspaces).expect("second link");
    let after = std::fs::read_link(root.join("node_modules/nsh")).expect("link");
    assert_eq!(before, after, "an already correct link is left alone");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_scoped_workspace_package_gets_its_scope_directory() {
    let root = project("scoped");
    write(
        &root.join("package.json"),
        "{\"name\": \"app\", \"workspaces\": [\"packages/*\"]}\n",
    );
    write(
        &root.join("packages/core/package.json"),
        "{\"name\": \"@eta/core\", \"main\": \"./index.js\"}\n",
    );
    write(
        &root.join("packages/core/index.js"),
        "export const core = 1;\n",
    );
    let linked = in_dir(&root, link_workspaces).expect("link");
    assert_eq!(linked.len(), 1);
    assert!(root.join("node_modules/@eta/core/index.js").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_exports_of_a_linked_package_are_what_resolves_a_subpath() {
    let root = project("subpath");
    manifest(&root);
    in_dir(&root, link_workspaces).expect("link");
    write(
        &root.join("app.ts"),
        "import { describe } from 'nsh/test';\nexport const said = describe();\n",
    );

    let mut h = Harness::with_modules();
    h.import_file(root.join("app.ts")).expect("import nsh/test");
    assert_eq!(h.expect_text("globalThis.__xmas_entry.said"), "described");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_project_without_workspaces_links_nothing() {
    let root = project("plain");
    write(&root.join("package.json"), "{\"name\": \"app\"}\n");
    let linked = in_dir(&root, link_workspaces).expect("link");
    assert!(linked.is_empty(), "{linked:?}");
    assert!(
        !root
            .join("node_modules")
            .read_dir()
            .map(|mut it| it.next().is_some())
            .unwrap_or(false)
    );
    let _ = std::fs::remove_dir_all(&root);
}
