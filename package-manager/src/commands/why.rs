//! Why command implementation.

use color_eyre::eyre::{eyre, Result};
use color_eyre::owo_colors::OwoColorize;
use compact_str::CompactString;
use itertools::Itertools;
use multimap::MultiMap;
use node_semver::Version;
use rustc_hash::FxHashSet;
use std::collections::VecDeque;

use crate::package::PackageSpecifier;
use crate::resolve::Graph;
use crate::util::{load_graph_from_lockfile, read_package};

/// Execute the why command.
pub async fn cmd_why(name: &CompactString, version: Option<&Version>) -> Result<()> {
    let package = read_package().await?;

    let graph = load_graph_from_lockfile().await;

    let map = build_map(&graph)?;

    let mut seen = FxHashSet::default();
    let mut queue = VecDeque::new();

    if let Some(version) = version {
        queue.push_back((name.clone(), version.clone()));
    } else {
        for (req, resolved) in graph.relations.iter() {
            if name == &req.name {
                queue.push_back((name.clone(), resolved.version.clone()));
            }
        }
    }

    if queue.is_empty() {
        return Err(eyre!("Package {} is not used", name));
    }

    while let Some((name, version)) = queue.pop_front() {
        if seen.insert((name.clone(), version.clone())) {
            if let Some(required_by) = map.get_vec(&(name.clone(), version.clone())) {
                let required_by: FxHashSet<_> = required_by
                    .iter()
                    .map(|x| graph.resolve_req(x))
                    .try_collect()?;
                if !required_by.is_empty() {
                    println!(
                        "{}",
                        format!("{}@{} is used by:", name.yellow(), version).bold()
                    );
                    for dep in required_by {
                        queue.push_back((dep.package.name.clone(), dep.version.clone()));
                        println!(" - {}@{}", dep.package.name, dep.version);
                    }
                    println!();
                }
            } else if package
                .iter_all()
                .any(|x| x.name == name && x.version.satisfies(&version))
            {
                println!(
                    "{}",
                    format!("{}@{} is used by package.json", name.yellow(), version).bold()
                );
                println!();
            } else {
                return Err(eyre!("Package {}@{} is not used", name, version));
            }
        }
    }

    println!("Analyzed {} packages", seen.len().yellow());

    Ok(())
}

fn build_map(graph: &Graph) -> Result<MultiMap<(CompactString, Version), PackageSpecifier>> {
    let mut map = MultiMap::new();

    for (from, to) in graph.relations.iter() {
        for child_req in to.package.iter() {
            let child_dep = graph.resolve_req(&child_req)?;
            map.insert(
                (child_dep.package.name.clone(), child_dep.version),
                from.clone(),
            );
        }
    }

    Ok(map)
}
