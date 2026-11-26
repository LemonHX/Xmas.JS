//! The one command line this project has.
//!
//! The package manager used to parse its own argv with its own copy of these subcommands, and the
//! binary then translated one parser's types into the other's. It is a module of the runtime now,
//! so what is below is the only parser: a subcommand either goes to `xmas_runtime::package` or to
//! the runtime itself, and the command functions take plain values.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use compact_str::CompactString;
use xmas_runtime::package::{self, commands, Options};

/// Xmas.JS - A Modern System Scripting Runtime for the JavaScript Era
#[derive(Parser)]
#[command(name = "xmas", author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Print verbose logs
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Refuse to modify the lockfile
    #[arg(long, global = true)]
    immutable: bool,

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

    /// Run the tests (files named *.test.ts, *_test.ts, *.spec.ts and the like)
    Test {
        /// Only the files whose path contains one of these
        filters: Vec<String>,

        /// Per-test timeout in milliseconds
        #[arg(long, default_value_t = 5000)]
        timeout: u64,

        /// Stop after the first failure
        #[arg(long)]
        bail: bool,

        /// Only run tests whose full name matches this regular expression
        #[arg(short = 't', long)]
        test_name_pattern: Option<String>,
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

    let options = Options::new().verbose(cli.verbose).immutable(cli.immutable);

    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        return Ok(());
    };

    match command {
        Commands::Serve {
            entry,
            port,
            host,
            workers,
        } => serve(&entry, &host, port, workers),

        Commands::Test { filters, timeout, bail, test_name_pattern } => {
            let options = xmas_runtime::testrun::Options {
                timeout_ms: timeout,
                bail,
                name_pattern: test_name_pattern,
            };
            let root = std::env::current_dir()?;
            if xmas_runtime::testrun::run(&root, &filters, &options) {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }

        // The package manager: one call per command, and the trace and the progress bars are put
        // away afterwards whatever the command did.
        command => package_command(command, options),
    }
}

/// Run one package manager command, and put away what it left behind.
fn package_command(command: Commands, options: Options) -> anyhow::Result<()> {
    package::install_error_reporter().map_err(anyhow::Error::msg)?;
    xmas_runtime::package::progress::set_verbose(options.verbose);

    let done = match command {
        Commands::Install => commands::cmd_install(&options),
        Commands::Update => commands::cmd_update(&options),
        Commands::Add { names, dev, pin } => commands::cmd_add(&names, dev, pin, &options),
        Commands::Run { name, watch } => commands::cmd_run(&options, &name, &watch),
        Commands::Clean => commands::cmd_clean(),
        Commands::Upgrade { pin } => commands::cmd_upgrade(pin),
        Commands::Exec { exe, args } => commands::cmd_exec(&options, &exe, &args),
        Commands::Remove { names, dev } => commands::cmd_remove(&names, dev, &options),
        Commands::Why { name, version } => commands::cmd_why(&name, version.as_ref()),
        Commands::Create { name } => commands::cmd_create(&options, &name),
        Commands::Dlx { name, args } => commands::exec::cmd_download_and_exec(&options, &name, &args),
        Commands::Serve { .. } | Commands::Test { .. } => {
            unreachable!("serve and test are handled before this")
        }
    };

    package::finish();

    done.map_err(|error| anyhow::anyhow!("{error:#}"))
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
    // Bound, not dropped: the handle owns the workers, and the loop below never ends to release it.
    let _server =
        xmas_runtime::entry::serve_app(&addr, workers, entry).map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("xmas: listening on http://{addr} ({workers} workers)");

    // The runtime runs its own threads; this one has nothing left to do but wait for a signal.
    loop {
        std::thread::park();
    }
}
