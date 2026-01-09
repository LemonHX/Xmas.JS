use std::path::PathBuf;

use clap::{Parser, Subcommand};
use colored::*;
use compact_str::CompactString;
use rsquickjs::{context::EvalOptions, Promise};
use std::ffi::OsString;
use xmas::utils::ctx::CtxExtension;

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

    /// Script file to run (if no subcommand is provided)
    #[arg(trailing_var_arg = true)]
    script: Vec<OsString>,
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

    // ==================== REPL ====================
    /// Start the interactive REPL
    Repl,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set working directory if specified
    if let Some(cwd) = &cli.working_dir {
        std::env::set_current_dir(cwd)?;
    }

    match cli.command {
        // No command - enter REPL or run script
        None => {
            if cli.script.is_empty() {
                // No script provided, enter REPL
                xmas::repl().await
            } else {
                // Run script file
                let script_path = cli.script[0].to_string_lossy().to_string();
                run_script(&script_path, &cli.script[1..]).await
            }
        }

        // REPL command
        Some(Commands::Repl) => xmas::repl().await,

        // Package manager commands
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

        // Bundler command
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

async fn run_script(script_path: &str, _args: &[OsString]) -> anyhow::Result<()> {
    use rsquickjs::{AsyncContext, AsyncRuntime};
    use std::sync::Arc;
    use xmas_js_modules::module::module_builder::ModuleBuilder;
    use xmas_js_modules::module::package::loader::PackageLoader;
    use xmas_js_modules::module::package::resolver::PackageResolver;
    use xmas_js_modules::permissions::Permissions;

    // Initialize tracing
    tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(tracing::Level::WARN)
        .without_time()
        .init();

    // Get the script name without extension for output
    let script_name = std::path::Path::new(script_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bundle");

    // Bundle the script first
    println!("{} {}...", "Bundling".cyan().bold(), script_path);
    let bundle_config = xmas_bundler::BundleConfig {
        entry: vec![PathBuf::from(script_path)],
        output_dir: PathBuf::from("."),
        output_filename: Some(format!("{}.js", script_name)),
        minify: false,
        source_map: false,
        format: xmas_bundler::BundleFormat::Esm,
        tree_shake: true,
        external: vec![],
    };
    xmas_bundler::bundle(bundle_config)
        .await
        .map_err(|e| anyhow::anyhow!("Bundle error: {}", e))?;

    // Now run the bundled output
    let bundled_path = format!("{}.js", script_name);
    println!("{} {}...", "Running".green().bold(), bundled_path);

    let runtime = AsyncRuntime::new()?;
    let context = AsyncContext::full(&runtime).await?;

    let (resolver, loader, ga) = ModuleBuilder::default().build();
    runtime
        .set_loader((resolver, PackageResolver), (loader, PackageLoader))
        .await;

    // Read the bundled output
    let script_content = std::fs::read_to_string(&bundled_path)?;

    rsquickjs::async_with!(context => |ctx| {
        let vsys = xmas_vsys::Vsys::builder()
            .permissions(Permissions::allow_all())
            .build();
        xmas_js_modules::init(&ctx, Arc::new(vsys), xmas_js_modules::console::LogType::Stdio)?;
        ga.attach(&ctx)?;
        let poller = ctx.get_background_task_poller();

        // Execute the bundled script directly (already transformed JS)
        match ctx.eval_with_options(
            script_content,
            EvalOptions {
                promise: true,
                filename: Some(bundled_path.into()),
                ..Default::default()
            },
        ) {
            Ok(promise) => {
                let promise : Promise<'_> = promise;
                match promise.into_future::<()>().await {
                    Ok(value) => {
                        println!("{}: {:?}", "Result".green().bold(), value);
                    },
                    Err(e) => {
                        eprintln!("{}: {}", "Error".red().bold(), e);
                                        let err = ctx.catch();
                eprintln!("{}: {:?}", "Exception".red().bold(), err.into_exception().map(|e| e.to_string()));
                    }
                }
            }
            Err(e) => {
                eprintln!("{}: {}", "Error".red().bold(), e);
                let err = ctx.catch();
                eprintln!("{}: {:?}", "Exception".red().bold(), err.into_exception().map(|e| e.to_string()));
            }
        }
        poller.abort();
        Ok(())
    })
    .await
}
