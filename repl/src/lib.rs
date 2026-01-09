use clap::Parser;
use colored::*;
use rsquickjs::prelude::Rest;
use rsquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Value};
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::HistoryHinter;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{Completer, Helper, Hinter, Validator};
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::io::stdout;
use std::sync::Arc;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};
use syntect::util::LinesWithEndings;
use xmas_js_modules::console::write_log;
use xmas_js_modules::module::package::loader::PackageLoader;
use xmas_js_modules::module::package::resolver::PackageResolver;
use xmas_js_modules::permissions::Permissions;
use xmas_js_modules::utils::ctx::CtxExtension;
use xmas_js_modules::utils::result::ResultExt;

/// Transform static import statements to dynamic import for REPL compatibility
/// - `import * as name from "module"` -> `const name = await import("module")`
/// - `import { a, b } from "module"` -> `const { a, b } = await import("module")`
/// - `import name from "module"` -> `const { default: name } = await import("module")`
/// - `import "module"` -> `await import("module")`
fn transform_import_to_dynamic(input: &str) -> String {
    let trimmed = input.trim();
    
    // Check if it starts with "import"
    if !trimmed.starts_with("import ") && !trimmed.starts_with("import\t") {
        return input.to_string();
    }
    
    // Remove "import " prefix
    let rest = trimmed.strip_prefix("import").unwrap().trim();
    
    // Find "from" keyword position
    if let Some(from_pos) = rest.rfind(" from ") {
        let imports_part = rest[..from_pos].trim();
        let module_part = rest[from_pos + 6..].trim().trim_end_matches(';');
        
        // import * as name from "module"
        if imports_part.starts_with("* as ") {
            let name = imports_part.strip_prefix("* as ").unwrap().trim();
            return format!("const {} = await import({})", name, module_part);
        }
        
        // import { ... } from "module"
        if imports_part.starts_with('{') && imports_part.ends_with('}') {
            return format!("const {} = await import({})", imports_part, module_part);
        }
        
        // import name from "module" (default import)
        // Also handles: import name, { a, b } from "module"
        if imports_part.contains(',') {
            // import default, { named } from "module"
            let parts: Vec<&str> = imports_part.splitn(2, ',').collect();
            let default_name = parts[0].trim();
            let rest_imports = parts[1].trim();
            if rest_imports.starts_with('{') && rest_imports.ends_with('}') {
                let inner = &rest_imports[1..rest_imports.len()-1];
                return format!("const {{ default: {}, {} }} = await import({})", default_name, inner, module_part);
            }
        }
        
        // Simple default import: import name from "module"
        return format!("const {{ default: {} }} = await import({})", imports_part, module_part);
    }
    
    // Side-effect import: import "module"
    let module_part = rest.trim().trim_end_matches(';');
    if module_part.starts_with('"') || module_part.starts_with('\'') || module_part.starts_with('`') {
        return format!("await import({})", module_part);
    }
    
    // Can't parse, return as-is
    input.to_string()
}

#[derive(Helper, Completer, Hinter, Validator)]
struct JSHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,

    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter for JSHelper {
    fn highlight<'l>(&self, line: &'l str, _: usize) -> std::borrow::Cow<'l, str> {
        let mut h = HighlightLines::new(
            self.syntaxes.find_syntax_by_extension("tsx").unwrap(),
            &self.theme,
        );
        let mut out = String::new();
        for line in LinesWithEndings::from(line) {
            let ranges = h.highlight_line(line, &self.syntaxes).unwrap();
            let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges[..], false);
            out += &escaped;
        }
        std::borrow::Cow::Owned(out)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _: bool,
    ) -> std::borrow::Cow<'b, str> {
        std::borrow::Cow::Owned(prompt.green().bold().to_string())
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(hint.bright_black().to_string())
    }

    fn highlight_candidate<'c>(
        &self,
        candidate: &'c str,
        _: rustyline::CompletionType,
    ) -> std::borrow::Cow<'c, str> {
        std::borrow::Cow::Owned(candidate.bright_cyan().to_string())
    }
}

fn print_version() {
    let xcolor = Color::TrueColor {
        r: 204,
        g: 0,
        b: 102,
    };
    let mcolor = Color::TrueColor {
        r: 153,
        g: 240,
        b: 0,
    };
    let acolor = Color::TrueColor {
        r: 102,
        g: 102,
        b: 255,
    };
    let jscolor = Color::TrueColor {
        r: 255,
        g: 205,
        b: 51,
    };

    println!(
        "{}{}{}{}{}",
        " ██╗  ██╗".color(xcolor),
        "███╗   ███╗".color(mcolor),
        " █████╗ ".color(acolor),
        "███████╗".color(xcolor),
        "         ██╗ ███████╗".color(jscolor)
    );
    println!(
        "{}{}{}{}{}",
        " ╚██╗██╔╝".color(xcolor),
        "████╗ ████║".color(mcolor),
        "██╔══██╗".color(acolor),
        "██╔════╝".color(xcolor),
        "         ██║ ██╔════╝".color(jscolor)
    );
    println!(
        "{}{}{}{}{}",
        "  ╚███╔╝ ".color(xcolor),
        "██╔████╔██║".color(mcolor),
        "███████║".color(acolor),
        "███████╗".color(xcolor),
        "         ██║ ███████╗".color(jscolor)
    );
    println!(
        "{}{}{}{}{}",
        "  ██╔██╗ ".color(xcolor),
        "██║╚██╔╝██║".color(mcolor),
        "██╔══██║".color(acolor),
        "╚════██║".color(xcolor),
        "    ██   ██║ ╚════██║".color(jscolor)
    );
    println!(
        "{}{}{}{}{}",
        " ██╔╝ ██╗".color(xcolor),
        "██║ ╚═╝ ██║".color(mcolor),
        "██║  ██║".color(acolor),
        "███████║".color(xcolor),
        "██╗ ╚█████╔╝ ███████║".color(jscolor)
    );
    println!(
        "{}{}{}{}{}",
        " ╚═╝  ╚═╝".color(xcolor),
        "╚═╝     ╚═╝".color(mcolor),
        "╚═╝  ╚═╝".color(acolor),
        "╚══════╝".color(xcolor),
        "╚═╝  ╚════╝  ╚══════╝".color(jscolor)
    );
    println!(
        "☃️\t{} {}",
        "Xmas.JS version".bold().cyan(),
        env!("CARGO_PKG_VERSION").cyan().italic()
    );
    println!(
        "🛷\t{} {}",
        "/help".cyan().bold(),
        "for getting help".cyan()
    );
    println!(
        "⛷️\t{}{}{}{}\n",
        "CTRL+D".cyan().bold(),
        " for save and exit ".cyan(),
        "CTRL+C".cyan().bold(),
        " for interrupt exit".cyan()
    );
}

pub async fn repl() -> anyhow::Result<()> {
    tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(tracing::Level::WARN)
        .init();
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(JSHelper {
        completer: FilenameCompleter::new(),
        validator: MatchingBracketValidator::new(),
        hinter: HistoryHinter::new(),
        syntaxes: {
            let mut syntaxset = SyntaxSetBuilder::new();
            let syntaxdef = SyntaxDefinition::load_from_str(
                include_str!("../tsx.sublime-syntax"),
                true,
                Some("js"),
            )
            .unwrap();
            syntaxset.add(syntaxdef);
            syntaxset.build()
        },
        theme: {
            let ts = ThemeSet::load_defaults();
            ts.themes["base16-ocean.dark"].clone()
        },
    }));
    if rl.load_history("history.js").is_err() {}
    let runtime = AsyncRuntime::new()?;
    let context = AsyncContext::full(&runtime).await?;
    print_version();
    let allocator = xmas_js_modules::script::allocator();
    let (resolver, loader, ga) =
        xmas_js_modules::module::module_builder::ModuleBuilder::default().build();
    runtime
        .set_loader((resolver, PackageResolver), (loader, PackageLoader))
        .await;
    rsquickjs::async_with!(context => |ctx| {
        let vsys = xmas_vsys::Vsys::builder()
            .permissions(Permissions::allow_all())
            .build();
        xmas_js_modules::init(&ctx, Arc::new(vsys), xmas_js_modules::console::LogType::Stdio)?;
        ga.attach(&ctx)?;
        let t = ctx.get_background_task_poller();
        loop {
            let readline = rl.readline("🎄 >> ");
            match readline {
                Ok(line) => {
                    // Handle special commands
                    if line.starts_with("/") && !line.starts_with("//") && !line.ends_with("/") {
                        match line.strip_prefix("/").unwrap() {
                            "help" => {
                                println!("\n{}", "💡 Available commands:".bold().cyan());
                                println!("❄️\t{} - Show this help message", "/help".cyan().bold());
                                println!("❄️\t{} - Show version information", "/version".cyan().bold());
                                println!("❄️\t{} - Clear the console", "/clear".cyan().bold());
                                println!("❄️\t{} - Package manager commands", "/pm".cyan().bold());
                                println!("❄️\t{} - Cross platform shell commands", "/$".cyan().bold());
                                println!("❄️\t{} - Bundle JavaScript/TypeScript files", "/bun".cyan().bold());

                            },
                            "version" => {
                                print_version();
                            },
                            "clear" => {
                                // Clear the console
                                println!("\x1B[2J\x1B[1;1H");
                            },
                            // package manager commands
                            cmd => {
                                let args = cmd.split_ascii_whitespace().collect::<Vec<_>>();
                                if args[0] == "pm" {
                                    if let Ok(cmd) = xmas_package_manager::cli::Subcommand::try_parse_from(&args) {
                                        let args = xmas_package_manager::Args {
                                            verbose: true,
                                            working_dir: std::env::current_dir().ok(),
                                            immutable: false,
                                            cmd
                                        };
                                        let _ = xmas_package_manager::execute_command(&args).await;
                                    } else {
                                        eprintln!("{}: Invalid package manager command", "Error".red().bold());
                                    }
                                }
                                else if args[0] == "$" {
                                    let shell_command = args[1..].join(" ");
                                    let cwd = std::env::current_dir()?;
                                    let mut new_env = std::collections::HashMap::new();
                                    new_env.insert(std::ffi::OsString::from("PATH"), xmas_package_manager::commands::new_path().map_err(|e| {
                                        anyhow::anyhow!("Failed to construct PATH: {}", e)
                                    })?);
                                    let exit_code = xmas_package_manager::commands::exec::shell(&shell_command, cwd, new_env, deno_task_shell::KillSignal::default()).await.map_err(|e| {
                                        anyhow::anyhow!("Failed to execute shell command: {}", e)
                                    })?;
                                    if exit_code != 0 {
                                        eprintln!("{}: Shell command exited with code {}", "Error".red().bold(), exit_code);
                                    }
                                }
                                else if args[0] == "bun" {
                                    if let Ok(cmd) = xmas_bundler::BundleConfig::try_parse_from(&args) {
                                        let _ = xmas_bundler::bundle(cmd).await;
                                    } else {
                                        eprintln!("{}: Invalid bundler command", "Error".red().bold());
                                    }
                                }
                                else {
                                    eprintln!("{}: Unknown command '{}'", "Error".red().bold(), cmd);
                                }
                            }
                        }
                        continue;
                    }

                    rl.add_history_entry(line.as_str())?;
                    
                    // Transform import statements to dynamic import for REPL compatibility
                    // import * as name from "module" -> const name = await import("module")
                    // import { a, b } from "module" -> const { a, b } = await import("module")
                    // import name from "module" -> const { default: name } = await import("module")
                    let line = transform_import_to_dynamic(&line);
                    
                    let ast = xmas_js_modules::script::parse("tsx", &line, &allocator).or_throw(&ctx)?;
                    let transformed = xmas_js_modules::script::transform(
                        &format!("<repl_input>.tsx"),
                        None,
                        false,
                        &allocator,
                        ast,
                    ).or_throw(&ctx)?;
                    match ctx.eval_promise::<_>(transformed.as_bytes()) {
                        Ok(res) => {
                            res.into_future::<Value>().await
                            .catch(&ctx)
                            .and_then(|v| {
                                let v = if v.is_object() {
                                    v.as_object().unwrap().get("value").unwrap()
                                } else {
                                    v
                                };
                                let _ = write_log(stdout(), &ctx, Rest(vec![v]));
                                Ok(())
                            })
                            .unwrap_or_else(|err| {
                                eprintln!("{}: {}", "Error".red().bold(), err);
                                let err = ctx.catch();
                                eprintln!("{}: {:?}", "Exception".red().bold(), err.into_exception().map(|e| e.to_string()));
                        });
                        },
                        Err(err) => {
                            eprintln!("{}: {}", "Error".red().bold(), err);
                            let err = ctx.catch();
                            eprintln!("{}: {:?}", "Exception".red().bold(), err.into_exception().map(|e| e.to_string()));
                        }
                    }
                },
                Err(ReadlineError::Interrupted) => {
                    t.abort();
                    println!("{} {}", "CTRL-C".cyan().bold(),"received, exiting...".cyan());
                    break
                },
                Err(ReadlineError::Eof) => {
                    t.abort();
                    println!("{} {}", "CTRL-D".cyan().bold(),"received, save and exiting...".cyan());
                    rl.save_history("history.js")?;
                    break
                },
                Err(err) => {
                    println!("Error: {:?}", err);
                    break
                }
            }
        }
        Ok(())
    }).await
}
