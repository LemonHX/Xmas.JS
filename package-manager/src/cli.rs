//! Command-line interface definitions for Cotton.

use clap::Parser;
use compact_str::CompactString;
use node_semver::Version;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Print verbose logs (including progress indicators)
    #[clap(short, long, global = true)]
    pub verbose: bool,
    /// Prevent any modifications to the lockfile
    #[clap(long, global = true)]
    pub immutable: bool,
    /// Run in a custom working directory
    #[clap(long, global = true, alias = "cwd")]
    pub working_dir: Option<PathBuf>,

    /// Subcommand to execute
    #[clap(subcommand)]
    pub cmd: Subcommand,
}

#[derive(Parser, Debug, Clone)]
pub enum Subcommand {
    /// Install packages defined in package.json
    #[clap(alias = "i")]
    Install,
    /// Prepare and save a newly planned lockfile
    Update,
    /// Add package to package.json
    #[clap(alias = "a")]
    Add {
        names: Vec<CompactString>,
        /// Add to `devDependencies` instead of `dependencies`
        #[clap(short = 'D', long)]
        dev: bool,
        /// Pin dependencies to a specific version
        #[clap(long, alias = "exact")]
        pin: bool,
    },
    /// Run a script defined in package.json
    Run {
        name: CompactString,
        #[clap(long)]
        watch: Vec<PathBuf>,
    },
    /// Clean packages installed in `node_modules` and remove cache
    Clean,
    /// Update packages specified in package.json to the latest available version
    Upgrade {
        /// Pin dependencies to a specific version
        #[clap(long)]
        pin: bool,
    },
    /// Execute a command that is not specified as a script
    Exec { exe: OsString, args: Vec<OsString> },
    /// Remove package from package.json
    Remove {
        names: Vec<CompactString>,
        /// Remove from `devDependencies` instead of `dependencies`
        #[clap(short = 'D', long)]
        dev: bool,
    },
    /// Find all uses of a given package
    Why {
        name: CompactString,
        version: Option<Version>,
    },
    /// Create new projects from a `create-` starter kit
    Create { name: CompactString },
    /// Download (if needed) and execute a command
    #[clap(name = "x")]
    DownloadAndExec { name: OsString, args: Vec<OsString> },
}
