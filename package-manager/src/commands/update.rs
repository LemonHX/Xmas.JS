//! Update command implementation.

use color_eyre::eyre::eyre;
use color_eyre::eyre::Result;
use color_eyre::owo_colors::OwoColorize;
use color_eyre::Help;
use std::time::Instant;

use crate::commands::init_storage;
use crate::progress::PROGRESS_BAR;
use crate::resolve::{Graph, Lockfile};
use crate::util::{read_package, write_json};
use crate::Args;

/// Execute the update command.
pub async fn cmd_update(args: &Args) -> Result<()> {
    if args.immutable {
        return Err(eyre!("Cannot update lockfile").suggestion("Remove the --immutable flag"));
    }

    let package = read_package().await?;

    init_storage().await?;

    let start = Instant::now();

    let mut graph = Graph::default();
    graph.append(package.iter_all(), false).await?;
    write_json("xmas.lock", Lockfile::new(graph.clone())).await?;

    PROGRESS_BAR.suspend(|| {
        println!(
            "Prepared {} packages in {}ms",
            graph.relations.len().yellow(),
            start.elapsed().as_millis().yellow()
        )
    });

    Ok(())
}
