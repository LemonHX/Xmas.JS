//! Install the package an import asked for.
//!
//! A bare specifier that is not in `node_modules` is a request for that package: the loader resolves
//! it, finds nothing on disk, and calls here. This is the package manager without its CLI — the same
//! resolve, the same store, the same plan — with two deliberate differences:
//!
//! - Nothing is written to `package.json`. A file that imports a package is not a decision about the
//!   project's dependencies; when the user wants it recorded they run `xmas add`.
//! - Everything is quiet. The install runs inside an import, so a progress bar redrawing over the
//!   program's own output would be noise.
//!
//! The commands in `commands/*.rs` work on paths relative to the process's working directory, which
//! is why this takes a lock and steps into the project for the length of the install.

use std::env;
use std::path::Path;
use std::sync::Mutex;

use color_eyre::eyre::{eyre, Result};
use compact_str::ToCompactString;

use crate::package::commands::{exec_install_scripts, init_storage};
use crate::package::npm::fetch_package;
use crate::package::metadata::PackageSpecifier;
use crate::package::plan::{execute_plan, setup_bins, Plan};
use crate::package::util::{load_graph_from_lockfile, read_json, write_json, VersionSpecifier};

/// One install at a time: everything below reads and writes paths relative to the working directory.
static ONCE: Mutex<()> = Mutex::new(());

/// Install `name` into the project rooted at `root`, with its dependencies.
pub fn install_package(root: &Path, name: &str) -> Result<()> {
    let _once = ONCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = env::current_dir()?;
    env::set_current_dir(root)?;
    let outcome = install_here(name);
    // Whatever happened, the process goes back to where it was: everything else in this process
    // assumes the directory it started in.
    env::set_current_dir(&previous)?;
    outcome
}

fn install_here(name: &str) -> Result<()> {
    init_storage()?;
    let response = smol::block_on(fetch_package(name))?;
    let latest = response
        .dist_tags
        .get("latest")
        .ok_or_else(|| eyre!("`{name}` has no latest tag in the registry"))?;
    let version: VersionSpecifier =
        serde_json::from_value(serde_json::Value::String(format!("^{latest}")))?;
    let spec = PackageSpecifier {
        name: name.to_compact_string(),
        version,
        optional: false,
    };

    let mut graph = load_graph_from_lockfile();
    smol::block_on(graph.append(std::iter::once(spec.clone()), true))?;
    let trees = graph.build_trees(std::slice::from_ref(&spec))?;
    let plan = Plan::new(
        trees
            .iter()
            .map(|tree| (tree.root.name.to_compact_string(), tree.clone()))
            .collect(),
    );

    smol::block_on(execute_plan(plan.clone()))?;
    setup_bins(&plan)?;
    for tree in plan.trees.values() {
        let stack = vec![tree.root.name.to_compact_string()];
        exec_install_scripts(tree, &stack)?;
    }

    // Keep the plan the project already had. The next `xmas install` compares against it, and
    // dropping the other trees would make it reinstall everything.
    let mut installed: Plan =
        read_json("node_modules/.xmas/plan.json").unwrap_or_else(|_| Plan::new(Default::default()));
    for (name, tree) in plan.trees.iter() {
        installed.trees.insert(name.clone(), tree.clone());
    }
    write_json("node_modules/.xmas/plan.json", &installed)?;
    Ok(())
}
