//! Install command implementation.

use async_recursion::async_recursion;
use color_eyre::eyre::{eyre, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::{CompactString, ToCompactString};
use deno_task_shell::KillSignal;
use itertools::Itertools;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs::{create_dir_all, read_to_string};
use tokio::process::Command;

use crate::commands::exec::shell;
use crate::config::read_config;
use crate::npm::DependencyTree;
use crate::package::PackageMetadata;
use crate::plan::{execute_plan, setup_bins, tree_size, Plan};
use crate::progress::{finish_progress, log_progress, log_verbose, set_total, PROGRESS_BAR};
use crate::resolve::Lockfile;
use crate::scoped_path::scoped_join;
use crate::util::{load_graph_from_lockfile, read_package, write_json};
use crate::Args;

/// Execute the install command.
pub async fn cmd_install(args: &Args) -> Result<()> {
    install(args).await
}

/// Prepare a plan for installing packages.
pub async fn prepare_plan(args: &Args, package: &PackageMetadata) -> Result<Plan> {
    log_progress("Preparing");

    let mut graph = load_graph_from_lockfile().await;

    if !args.immutable {
        graph.append(package.iter_all(), true).await?;
        write_json("xmas.lock", Lockfile::new(graph.clone())).await?;
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

async fn read_plan(path: &str) -> Result<Plan> {
    let plan = read_to_string(path).await?;
    Ok(serde_json::from_str(&plan)?)
}

/// Verify that the current installation matches the plan.
pub async fn verify_installation(package: &PackageMetadata, plan: &Plan) -> Result<bool> {
    let installed = read_plan("node_modules/.xmas/plan.json").await?;

    if &installed != plan {
        return Ok(false);
    }

    Ok(installed.satisfies(package))
}

async fn exec_install_scripts_in(stack: &[CompactString]) -> Result<()> {
    let path = stack.join("/node_modules/");

    let dir = scoped_join("node_modules", path)?;

    let package_json = match read_to_string(dir.join("package.json")).await {
        Ok(x) => x,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let package_json: PackageMetadata = serde_json::from_str(&package_json)?;

    for script_name in ["preinstall", "install", "postinstall"] {
        if let Some(Value::String(script)) = package_json.scripts.get(script_name) {
            PROGRESS_BAR.suspend(|| {
                println!("Executing {script_name} script for {}", stack.join(" > "));
            });

            let mut new_env = HashMap::new();
            new_env.insert(OsString::from("PATH"), new_path()?);
            let child = shell(script, dir.clone(), new_env, KillSignal::default()).await?;

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

async fn exec_install_scripts(
    tree: &DependencyTree,
    initial_stack: &[CompactString],
) -> Result<()> {
    let mut work_stack: Vec<(&DependencyTree, Vec<CompactString>)> =
        vec![(tree, initial_stack.to_vec())];

    while let Some((current_tree, mut stack)) = work_stack.pop() {
        exec_install_scripts_in(&stack).await?;

        stack.push(current_tree.root.name.clone());
        for child_tree in current_tree.children.values() {
            work_stack.push((child_tree, stack.clone()));
        }
    }

    Ok(())
}

/// Install packages based on package.json.
pub async fn install(args: &Args) -> Result<()> {
    let package = read_package().await?;

    init_storage().await?;
    let config = read_config().await?;

    let start = Instant::now();

    let plan = prepare_plan(args, &package).await?;
    let size = tree_size(&plan.trees);
    set_total(size as u64 * 2); // download + install

    if matches!(verify_installation(&package, &plan).await, Ok(true)) {
        log_verbose("Packages already installed")
    } else {
        execute_plan(plan.clone()).await?;

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
                exec_install_scripts(tree, &mut vec![name.clone()]).await?;
            }
        }

        setup_bins(&plan).await?;

        write_json("node_modules/.xmas/plan.json", &plan).await?;
    }

    PROGRESS_BAR.finish_and_clear();

    Ok(())
}

/// Create a new PATH with node_modules/.bin prepended.
pub fn new_path() -> Result<OsString> {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut paths = env::split_paths(&path).collect::<Vec<_>>();
    let new = PathBuf::from("node_modules/.bin");
    paths.insert(0, new);
    let new_path = env::join_paths(paths)?;
    Ok(new_path)
}

/// Join node_modules/.bin to the PATH environment variable.
pub fn join_paths() -> Result<()> {
    let path = new_path()?;
    log_verbose(&format!("Setting PATH to {path:?}"));
    env::set_var("PATH", path);

    Ok(())
}

/// Initialize storage directories.
pub async fn init_storage() -> Result<()> {
    create_dir_all(".xmas/store").await?;
    create_dir_all("node_modules/.xmas").await?;
    create_dir_all("node_modules/.bin").await?;

    Ok(())
}
