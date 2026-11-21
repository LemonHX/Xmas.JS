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
        // The id cannot be "version": clap already has one, from the --version flag.
        #[arg(id = "package_version")]
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

    // ==================== Server ====================
    /// Serve a module as an HTTP app
    Serve {
        /// Entry module (.ts, .tsx or .js)
        entry: PathBuf,

        /// Port to listen on
        #[arg(short, long, default_value_t = 8000)]
        port: u16,

        /// Address to bind
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// How many worker threads, and so how many realms; defaults to one per core
        #[arg(short, long)]
        workers: Option<usize>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(cwd) = &cli.working_dir {
        std::env::set_current_dir(cwd)?;
    }

    match cli.command {
        None => {
            anyhow::bail!("the runtime is being rewritten; only the package manager works")
        }

        Some(Commands::Install) => run_pm(xmas_package_manager::Subcommand::Install, cli.verbose),
        Some(Commands::Add { names, dev, pin }) => run_pm(
            xmas_package_manager::Subcommand::Add { names, dev, pin },
            cli.verbose,
        ),
        Some(Commands::Remove { names, dev }) => run_pm(
            xmas_package_manager::Subcommand::Remove { names, dev },
            cli.verbose,
        ),
        Some(Commands::Run { name, watch }) => run_pm(
            xmas_package_manager::Subcommand::Run { name, watch },
            cli.verbose,
        ),
        Some(Commands::Update) => run_pm(xmas_package_manager::Subcommand::Update, cli.verbose),
        Some(Commands::Upgrade { pin }) => run_pm(
            xmas_package_manager::Subcommand::Upgrade { pin },
            cli.verbose,
        ),
        Some(Commands::Clean) => run_pm(xmas_package_manager::Subcommand::Clean, cli.verbose),
        Some(Commands::Exec { exe, args }) => run_pm(
            xmas_package_manager::Subcommand::Exec { exe, args },
            cli.verbose,
        ),
        Some(Commands::Why { name, version }) => run_pm(
            xmas_package_manager::Subcommand::Why { name, version },
            cli.verbose,
        ),
        Some(Commands::Create { name }) => run_pm(
            xmas_package_manager::Subcommand::Create { name },
            cli.verbose,
        ),
        Some(Commands::Serve {
            entry,
            port,
            host,
            workers,
        }) => serve(&entry, &host, port, workers),

        Some(Commands::Dlx { name, args }) => run_pm(
            xmas_package_manager::Subcommand::DownloadAndExec { name, args },
            cli.verbose,
        ),
    }
}

/// Serve an app until the process is stopped.
///
/// The runtime runs its own worker threads and its own executor, so there is nothing here to
/// drive: this is a plain blocking call, and the process ends on a signal.
fn serve(entry: &PathBuf, host: &str, port: u16, workers: Option<usize>) -> anyhow::Result<()> {
    let workers = workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
    });
    let addr = format!("{host}:{port}");
    let server = xmas_runtime::entry::serve_app(&addr, workers, entry)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    println!(
        "listening on http://{} with {workers} worker{}",
        server.addr(),
        if workers == 1 { "" } else { "s" }
    );
    loop {
        std::thread::park();
    }
}

fn run_pm(cmd: xmas_package_manager::Subcommand, verbose: bool) -> anyhow::Result<()> {
    let args = xmas_package_manager::Args {
        verbose,
        immutable: false,
        working_dir: None,
        cmd,
    };
    xmas_package_manager::package_manager(&args).map_err(|e| anyhow::anyhow!("{}", e))
}
