use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use compact_str::CompactString;

/// Xmas.JS - A Modern System Scripting Runtime for the JavaScript Era
#[derive(Parser)]
#[command(name = "xmas", author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Print verbose logs
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Run in a custom working directory
    #[arg(long, global = true, alias = "cwd")]
    working_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    // ==================== Package Manager ====================
    /// Install packages defined in package.json
    #[command(alias = "i")]
    Install,

    /// Add package to package.json
    #[command(alias = "a")]
    Add {
        /// Package names to add
        names: Vec<CompactString>,
        /// Add to `devDependencies` instead of `dependencies`
        #[arg(short = 'D', long)]
        dev: bool,
        /// Pin dependencies to a specific version
        #[arg(long, alias = "exact")]
        pin: bool,
    },

    /// Remove package from package.json
    #[command(alias = "rm")]
    Remove {
        /// Package names to remove
        names: Vec<CompactString>,
        /// Remove from `devDependencies` instead of `dependencies`
        #[arg(short = 'D', long)]
        dev: bool,
    },

    /// Run a script defined in package.json
    Run {
        /// Script name
        name: CompactString,
        /// Watch files for changes
        #[arg(long)]
        watch: Vec<PathBuf>,
    },

    /// Prepare and save a newly planned lockfile
    Update,

    /// Update packages to the latest available version
    Upgrade {
        /// Pin dependencies to a specific version
        #[arg(long)]
        pin: bool,
    },

    /// Clean node_modules and cache
    Clean,

    /// Execute a command (not a script)
    Exec {
        /// Executable to run
        exe: OsString,
        /// Arguments to pass
        args: Vec<OsString>,
    },

    /// Find all uses of a given package
    Why {
        /// Package name
        name: CompactString,
        /// Package version
        version: Option<node_semver::Version>,
    },

    /// Create new project from a starter kit
    Create {
        /// Starter kit name (e.g., vite, next)
        name: CompactString,
    },

    /// Download and execute a package (like npx)
    #[command(name = "x")]
    Dlx {
        /// Package name
        name: OsString,
        /// Arguments to pass
        args: Vec<OsString>,
    },

    // ==================== Bundler ====================
    /// Bundle TypeScript/JavaScript files (powered by Rolldown)
    #[command(alias = "bundle")]
    Bun {
        /// Entry point(s) for the bundle
        entry: Vec<PathBuf>,

        /// Output directory
        #[arg(short = 'o', long, default_value = "dist")]
        output_dir: PathBuf,

        /// Output filename
        #[arg(short = 'n', long)]
        output_filename: Option<String>,

        /// Enable minification
        #[arg(short = 'm', long)]
        minify: bool,

        /// Enable source maps
        #[arg(short = 's', long)]
        source_map: bool,

        /// Target format (esm, cjs, iife)
        #[arg(short = 'f', long, default_value = "esm")]
        format: xmas_bundler::BundleFormat,

        /// External modules (won't be bundled)
        #[arg(short = 'e', long)]
        external: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(cwd) = &cli.working_dir {
        std::env::set_current_dir(cwd)?;
    }

    match cli.command {
        None => {
            anyhow::bail!("the runtime is being rewritten; only package manager and bundler work")
        }

        Some(Commands::Install) => {
            run_pm(xmas_package_manager::Subcommand::Install, cli.verbose).await
        }
        Some(Commands::Add { names, dev, pin }) => {
            run_pm(
                xmas_package_manager::Subcommand::Add { names, dev, pin },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Remove { names, dev }) => {
            run_pm(
                xmas_package_manager::Subcommand::Remove { names, dev },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Run { name, watch }) => {
            run_pm(
                xmas_package_manager::Subcommand::Run { name, watch },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Update) => {
            run_pm(xmas_package_manager::Subcommand::Update, cli.verbose).await
        }
        Some(Commands::Upgrade { pin }) => {
            run_pm(
                xmas_package_manager::Subcommand::Upgrade { pin },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Clean) => run_pm(xmas_package_manager::Subcommand::Clean, cli.verbose).await,
        Some(Commands::Exec { exe, args }) => {
            run_pm(
                xmas_package_manager::Subcommand::Exec { exe, args },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Why { name, version }) => {
            run_pm(
                xmas_package_manager::Subcommand::Why { name, version },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Create { name }) => {
            run_pm(
                xmas_package_manager::Subcommand::Create { name },
                cli.verbose,
            )
            .await
        }
        Some(Commands::Dlx { name, args }) => {
            run_pm(
                xmas_package_manager::Subcommand::DownloadAndExec { name, args },
                cli.verbose,
            )
            .await
        }

        Some(Commands::Bun {
            entry,
            output_dir,
            output_filename,
            minify,
            source_map,
            format,
            external,
        }) => {
            let config = xmas_bundler::BundleConfig {
                entry,
                output_dir,
                output_filename,
                minify,
                source_map,
                format,
                tree_shake: true,
                external,
            };
            xmas_bundler::bundle(config)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        }
    }
}

async fn run_pm(cmd: xmas_package_manager::Subcommand, verbose: bool) -> anyhow::Result<()> {
    let args = xmas_package_manager::Args {
        verbose,
        immutable: false,
        working_dir: None,
        cmd,
    };
    xmas_package_manager::package_manager(&args)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
}
