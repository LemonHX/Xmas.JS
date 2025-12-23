use colored::*;
use core::alloc;
use rsquickjs::prelude::Rest;
use rsquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Value};
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::HistoryHinter;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{Completer, Helper, Hinter, Validator};
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::io::stdout;
use std::ptr::NonNull;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, Theme, ThemeSet};
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder};
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use xmas_js_modules::console::write_log;
use xmas_js_modules::permissions::Permissions;
use xmas_js_modules::utils::ctx::CtxExtension;
use xmas_js_modules::utils::result::ResultExt;

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
    println!(
        "{}{}{}{}{}",
        " ██╗  ██╗".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "███╗   ███╗".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        " █████╗ ".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "███████╗".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "         ██╗ ███████╗".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
    );
    println!(
        "{}{}{}{}{}",
        " ╚██╗██╔╝".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "████╗ ████║".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        "██╔══██╗".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "██╔════╝".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "         ██║ ██╔════╝".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
    );
    println!(
        "{}{}{}{}{}",
        "  ╚███╔╝ ".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "██╔████╔██║".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        "███████║".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "███████╗".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "         ██║ ███████╗".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
    );
    println!(
        "{}{}{}{}{}",
        "  ██╔██╗ ".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "██║╚██╔╝██║".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        "██╔══██║".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "╚════██║".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "    ██   ██║ ╚════██║".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
    );
    println!(
        "{}{}{}{}{}",
        " ██╔╝ ██╗".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "██║ ╚═╝ ██║".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        "██║  ██║".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "███████║".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "██╗ ╚█████╔╝ ███████║".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
    );
    println!(
        "{}{}{}{}{}",
        " ╚═╝  ╚═╝".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "╚═╝     ╚═╝".color(Color::TrueColor {
            r: 153,
            g: 240,
            b: 0
        }),
        "╚═╝  ╚═╝".color(Color::TrueColor {
            r: 102,
            g: 102,
            b: 255
        }),
        "╚══════╝".color(Color::TrueColor {
            r: 204,
            g: 0,
            b: 102
        }),
        "╚═╝  ╚════╝  ╚══════╝".color(Color::TrueColor {
            r: 255,
            g: 205,
            b: 51
        })
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
        "⛷️\t{}",
        "CTRL+D for save and exit, CTRL+C for interrupt exit".cyan()
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
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
    rsquickjs::async_with!(context => |ctx| {
        xmas_js_modules::init(&ctx, Permissions::allow_all(), xmas_js_modules::console::LogType::Stdio)?;
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
                            },
                            "version" => {
                                print_version();
                            },
                            "clear" => {
                                // Clear the console
                                println!("\x1B[2J\x1B[1;1H");
                            },
                            cmd => {
                                eprintln!("{}: Unknown command '{}'", "Error".red().bold(), cmd);
                            }
                        }
                        continue;
                    }

                    rl.add_history_entry(line.as_str())?;
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

                            .unwrap_or_else(|err| eprintln!("{}: {}", "Error".red().bold(), err));
                        },
                        Err(err) => {
                            eprintln!("{}: {}", "Error".red().bold(), err);
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
