//! Install command implementation.

use color_eyre::eyre::{eyre, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::{CompactString, ToCompactString};
use itertools::Itertools;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::{create_dir_all, read_dir, read_to_string, remove_dir_all, remove_file};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::package::commands::exec::shell;
use crate::package::config::read_config;
use crate::package::npm::DependencyTree;
use crate::package::metadata::PackageMetadata;
use crate::package::plan::{execute_plan, setup_bins, tree_size, unique_count, Plan};
use crate::package::progress::{finish_progress, log_progress, log_verbose, set_total, PROGRESS_BAR};
use crate::package::resolve::Lockfile;
use crate::package::scoped_path::scoped_join;
use crate::package::util::{load_graph_from_lockfile, read_package, write_json};
use crate::package::Options;

/// Execute the install command.
pub fn cmd_install(args: &Options) -> Result<()> {
    install(args)
}

/// Prepare a plan for installing packages.
pub fn prepare_plan(args: &Options, package: &PackageMetadata) -> Result<Plan> {
    log_progress("Preparing");

    let mut graph = load_graph_from_lockfile();

    if !args.immutable {
        smol::block_on(graph.append(package.iter_all(), true))?;
        write_json("xmas.lock", Lockfile::new(graph.clone()))?;
    }

    log_progress("Retrieved dependency graph");

    let trees = graph.build_trees(&package.iter_all().collect_vec())?;
    log_progress(&format!("Fetched {} root deps", trees.len().yellow()));

    let plan = Plan::new(
        trees
            .iter()
            .map(|x| (x.root.name.to_compact_string(), x.clone()))
            .collect(),
    );

    log_progress(&format!(
        "Planned {} dependencies",
        plan.trees.len().yellow()
    ));

    Ok(plan)
}

fn read_plan(path: &str) -> Result<Plan> {
    let plan = read_to_string(path)?;
    Ok(serde_json::from_str(&plan)?)
}

/// Verify that the current installation matches the plan.
pub fn verify_installation(package: &PackageMetadata, plan: &Plan) -> Result<bool> {
    let installed = read_plan("node_modules/.xmas/plan.json")?;

    if &installed != plan {
        return Ok(false);
    }

    Ok(installed.satisfies(package))
}

/// Whether the project's package.json lists `name` under `trustedDependencies`.
fn is_trusted(name: &str) -> bool {
    read_to_string("package.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|json| {
            json.get("trustedDependencies")
                .and_then(|list| list.as_array())
                .map(|list| list.iter().any(|item| item.as_str() == Some(name)))
        })
        .unwrap_or(false)
}

fn exec_install_scripts_in(stack: &[CompactString]) -> Result<()> {
    let path = stack.join("/node_modules/");

    let dir = scoped_join("node_modules", path)?;

    let package_json = match read_to_string(dir.join("package.json")) {
        Ok(x) => x,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let package_json: PackageMetadata = serde_json::from_str(&package_json)?;

    // A dependency's lifecycle scripts run only when the project lists it in
    // `trustedDependencies`. Anything else is skipped with a warning.
    let scripts: Vec<&str> = ["preinstall", "install", "postinstall"]
        .into_iter()
        .filter(|name| matches!(package_json.scripts.get(*name), Some(Value::String(_))))
        .collect();
    if !scripts.is_empty() {
        let name = stack.last().map(|name| name.as_str()).unwrap_or_default();
        if !is_trusted(name) {
            PROGRESS_BAR.suspend(|| {
                eprintln!(
                    "warning: skipped {} script of {} (add it to \"trustedDependencies\" in package.json to run it)",
                    scripts.join("/"),
                    stack.join(" > ")
                );
            });
            return Ok(());
        }
    }

    for script_name in ["preinstall", "install", "postinstall"] {
        if let Some(Value::String(script)) = package_json.scripts.get(script_name) {
            PROGRESS_BAR.suspend(|| {
                println!("Executing {script_name} script for {}", stack.join(" > "));
            });

            let mut new_env = HashMap::new();
            new_env.insert(OsString::from("PATH"), new_path()?);
            let child = shell(script, dir.clone(), new_env)?;

            if child > 0 {
                return Err(eyre!(
                    "{} script failed with exit code {}",
                    script_name,
                    child
                ));
            }
        }
    }

    Ok(())
}

pub fn exec_install_scripts(tree: &DependencyTree, initial_stack: &[CompactString]) -> Result<()> {
    let mut work_stack: Vec<(&DependencyTree, Vec<CompactString>)> =
        vec![(tree, initial_stack.to_vec())];

    while let Some((current_tree, mut stack)) = work_stack.pop() {
        exec_install_scripts_in(&stack)?;

        stack.push(current_tree.root.name.clone());
        for child_tree in current_tree.children.values() {
            work_stack.push((child_tree, stack.clone()));
        }
    }

    Ok(())
}

/// Remove what is in node_modules and not in the plan.
///
/// Only the top level is swept. A package that stays keeps its own nested tree with it, so what
/// is left under a surviving package is still reachable from it; a package that goes takes its
/// subtree with it. The two directories the runtime owns -- .bin and .xmas -- are not packages.
fn prune(plan: &Plan) -> Result<()> {
    let entries = match read_dir("node_modules") {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".bin" || name == ".xmas" {
            continue;
        }
        if entry.file_type()?.is_dir() && name.starts_with('@') {
            // A scope is a directory of packages, not a package: keep the ones the plan names.
            for scoped in read_dir(entry.path())? {
                let scoped = scoped?;
                let scoped_name = scoped.file_name().to_string_lossy().into_owned();
                let full: CompactString = format!("{name}/{scoped_name}").into();
                if !plan.trees.contains_key(&full) {
                    remove_dir_all(scoped.path())?;
                }
            }
            // A scope with every package gone is itself gone.
            if read_dir(entry.path())?.next().is_none() {
                remove_dir_all(entry.path())?;
            }
            continue;
        }
        let full: CompactString = name.as_str().into();
        if !plan.trees.contains_key(&full) {
            remove_dir_all(entry.path())?;
        }
    }

    Ok(())
}

/// Install packages based on package.json.
pub fn install(args: &Options) -> Result<()> {
    let package = read_package()?;

    init_storage()?;
    let config = read_config()?;

    let start = Instant::now();

    let plan = prepare_plan(args, &package)?;
    let size = tree_size(&plan.trees);
    // One step per distinct download, one per placed package.
    set_total((unique_count(&plan.trees) + size) as u64);

    // A tree that already matches the plan can still hold what the plan dropped: removing a
    // package rewrites package.json and nothing else, so the directory it left behind stays
    // until someone sweeps. The sweep runs either way.
    prune(&plan)?;
    let workspaces = link_workspaces()?;

    if workspaces.is_empty() && matches!(verify_installation(&package, &plan), Ok(true)) {
        log_verbose("Packages already installed")
    } else {
        smol::block_on(execute_plan(plan.clone()))?;

        finish_progress();
        PROGRESS_BAR.suspend(|| {
            if size > 0 {
                println!(
                    "Installed {} packages in {}ms",
                    size.yellow(),
                    start.elapsed().as_millis().yellow()
                )
            }
        });

        if !config.disallow_install_scripts {
            for (name, tree) in plan.trees.iter() {
                exec_install_scripts(tree, &mut vec![name.clone()])?;
            }
        }

        setup_bins(&plan)?;

        write_json("node_modules/.xmas/plan.json", &plan)?;
    }

    PROGRESS_BAR.finish_and_clear();

    Ok(())
}

/// A package of this project's workspace: its name and the directory it is written in.
struct WorkspacePackage {
    name: CompactString,
    dir: PathBuf,
}

/// The packages this project's workspace declares, in the order the patterns name them.
///
/// The workspace root is the directory install runs in: its package.json holds `workspaces`, an
/// array of patterns or an object with `packages`. A pattern is a directory, or `dir/*`, or
/// `dir/**` taken as `dir/*`. A directory listed twice is one package.
fn workspace_packages(root: &Path) -> Result<Vec<WorkspacePackage>> {
    let Ok(text) = read_to_string(root.join("package.json")) else {
        return Ok(Vec::new());
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return Ok(Vec::new());
    };
    let patterns: Vec<String> = match json.get("workspaces") {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|p| p.as_str())
            .map(str::to_owned)
            .collect(),
        Some(Value::Object(map)) => map
            .get("packages")
            .and_then(|p| p.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|p| p.as_str())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        _ => return Ok(Vec::new()),
    };

    let mut found: Vec<WorkspacePackage> = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
        let star = pattern
            .strip_suffix("/**")
            .or_else(|| pattern.strip_suffix("/*"))
            .map(str::to_owned)
            .unwrap_or_else(|| pattern.to_owned());
        let mut candidates = vec![root.join(&star)];
        if let Some(rest) = pattern.strip_prefix("**/") {
            // '**/x' is 'x' and one directory below it, the depth the workspace globs of a
            // project this size actually use.
            candidates = vec![root.join(rest), root.join("packages").join(rest)];
        }
        if pattern.contains('/') && (pattern.ends_with("/**") || pattern.ends_with("/*")) {
            candidates = read_dir(root.join(&star))
                .map(|it| it.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
        }
        for candidate in candidates {
            let Ok(text) = read_to_string(candidate.join("package.json")) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(Value::String(name)) = json.get("name") else {
                continue;
            };
            if found.iter().any(|p| p.name == name.as_str()) {
                continue;
            }
            found.push(WorkspacePackage {
                name: name.as_str().into(),
                dir: candidate,
            });
        }
    }
    Ok(found)
}

/// Link every workspace package into `node_modules` under its name, the way bun and pnpm do.
///
/// Without the link a package of the workspace is only reachable where it sits: the resolver reads
/// a bare specifier as a package of node_modules, and the fallback that finds it in the workspace
/// is a guess -- it takes the subpath as a path. A link puts `exports` back in charge of a subpath
/// like `nsh/test`, which is what every workspace package here is reached through.
///
/// A link that is already right is left alone. A directory or a foreign link at that name is a
/// real package node_modules holds, and the plan owns it, so it is left alone too.
pub fn link_workspaces() -> Result<Vec<CompactString>> {
    let packages = workspace_packages(Path::new("."))?;
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    let mut linked = Vec::with_capacity(packages.len());
    for package in packages {
        let scope = match package.name.split_once('/') {
            Some((scope, _)) => {
                create_dir_all(Path::new("node_modules").join(scope))?;
                Some(scope.to_owned())
            }
            None => None,
        };
        let link = match &scope {
            Some(scope) => Path::new("node_modules").join(scope).join(
                package
                    .name
                    .split_once('/')
                    .map(|(_, rest)| rest)
                    .unwrap_or(&package.name),
            ),
            None => Path::new("node_modules").join(package.name.as_str()),
        };
        let target = std::fs::canonicalize(&package.dir)?;

        if let Ok(existing) = std::fs::read_link(&link) {
            if std::fs::canonicalize(&link).ok().as_ref() == Some(&target) {
                linked.push(package.name);
                continue;
            }
            // A link of ours from an earlier run that now points elsewhere: relink it.
            if existing.is_relative() || existing.starts_with("..") {
                remove_file(&link)?;
            } else {
                continue;
            }
        } else if link.exists() {
            continue;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)?;
        #[cfg(not(unix))]
        {
            return Err(eyre!(
                "workspace packages are linked with a symlink, which this platform does not have"
            ));
        }
        log_verbose(&format!("Linked workspace package {}", package.name));
        linked.push(package.name);
    }

    Ok(linked)
}

/// Create a new PATH with node_modules/.bin prepended.
pub fn new_path() -> Result<OsString> {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut paths = env::split_paths(&path).collect::<Vec<_>>();
    // Absolute, because this process changes directory under the PATH it just built: `x`
    // installs into a temporary directory, sets PATH there, then goes back to the caller's
    // directory to run the binary. A relative entry would be read from the wrong place and the
    // binary would not be found.
    let new = env::current_dir()?.join("node_modules").join(".bin");
    paths.insert(0, new);
    let new_path = env::join_paths(paths)?;
    Ok(new_path)
}


/// Initialize storage directories.
pub fn init_storage() -> Result<()> {
    create_dir_all(".xmas/store")?;
    create_dir_all("node_modules/.xmas")?;
    create_dir_all("node_modules/.bin")?;

    Ok(())
}
