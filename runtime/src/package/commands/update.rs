//! Update command implementation.

use color_eyre::eyre::eyre;
use color_eyre::eyre::Result;
use color_eyre::owo_colors::OwoColorize;
use color_eyre::Help;
use std::time::Instant;

use crate::package::commands::init_storage;
use crate::package::progress::PROGRESS_BAR;
use crate::package::resolve::{Graph, Lockfile};
use crate::package::util::{read_package, write_json};
use crate::package::Options;

/// Execute the update command.
pub fn cmd_update(args: &Options) -> Result<()> {
    if args.immutable {
        return Err(eyre!("Cannot update lockfile").suggestion("Remove the --immutable flag"));
    }

    let package = read_package()?;

    init_storage()?;

    let start = Instant::now();

    let mut graph = Graph::default();
    smol::block_on(graph.append(package.iter_all(), false))?;
    write_json("xmas.lock", Lockfile::new(graph.clone()))?;

    PROGRESS_BAR.suspend(|| {
        println!(
            "Prepared {} packages in {}ms",
            graph.relations.len().yellow(),
            start.elapsed().as_millis().yellow()
        )
    });

    Ok(())
}
