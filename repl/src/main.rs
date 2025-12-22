use std::io::stdout;
use std::ptr::NonNull;
use colored::*;
use rsquickjs::prelude::Rest;
use rsquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Value};
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::HistoryHinter;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{CompletionType, Config, EditMode, Editor};
use rustyline::{Completer, Helper, Hinter, Validator};
use syntect::easy::HighlightLines;
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder};
use syntect::util::{LinesWithEndings, as_24_bit_terminal_escaped};
use syntect::highlighting::{Style, Theme, ThemeSet};
use xmas_js_modules::console::write_log;
use xmas_js_modules::permissions::Permissions;
use xmas_js_modules::utils::ctx::CtxExtension;

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
        let mut h = HighlightLines::new(self.syntaxes.find_syntax_by_extension("tsx").unwrap(), &self.theme);
        let mut out = String::new();
        for line in LinesWithEndings::from(line) {
            let ranges = h.highlight_line(line, &self.syntaxes).unwrap();
            let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges[..], false);
            out += &escaped;
        }
        std::borrow::Cow::Owned(out)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _: bool) -> std::borrow::Cow<'b, str> {
        std::borrow::Cow::Owned(prompt.green().bold().to_string())
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(hint.bright_black().to_string())
    }

    fn highlight_candidate<'c>(&self, candidate: &'c str, _: rustyline::CompletionType) -> std::borrow::Cow<'c, str> {
        std::borrow::Cow::Owned(candidate.bright_cyan().to_string())
    }
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
    rl.set_helper(Some(JSHelper{
        completer: FilenameCompleter::new(),
        validator: MatchingBracketValidator::new(),
        hinter: HistoryHinter::new(),
        syntaxes: {
            let mut syntaxset = SyntaxSetBuilder::new();
            let syntaxdef = SyntaxDefinition::load_from_str(include_str!("../tsx.sublime-syntax"), true, Some("js")).unwrap();
            syntaxset.add(syntaxdef);
            syntaxset.build()
        },
        theme: {
            let ts = ThemeSet::load_defaults();
            ts.themes["base16-ocean.dark"].clone()
        }
    }));
    if rl.load_history("history.js").is_err() {
        println!("No previous history.");
    }
    let runtime = AsyncRuntime::new()?;
    let context = AsyncContext::full(&runtime).await?;
    rsquickjs::async_with!(context => |ctx| {
        xmas_js_modules::init(&ctx, Permissions::allow_all(), xmas_js_modules::console::LogType::Stdio)?;
        let t = ctx.get_background_task_poller();
        loop {
            let readline = rl.readline("🎄 >> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str())?;
                    match ctx.eval_promise::<_>(line.as_bytes()) {
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
                    println!("CTRL-C received, exiting...");
                    break
                },
                Err(ReadlineError::Eof) => {
                    t.abort();
                    println!("CTRL-D received, save and exiting...");
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

// fn main() {
//     let ps = SyntaxSet::load_defaults_newlines();
//     let ts = ThemeSet::load_defaults();
//     let theme = &ts.themes["base16-ocean.dark"];
//     let syntaxdef = SyntaxDefinition::load_from_str(include_str!("../tsx.sublime-syntax"), true, Some("js")).unwrap();
//     let mut syntaxset = SyntaxSetBuilder::new();
//     syntaxset.add(syntaxdef);
//     let syntaxset = syntaxset.build();
//     let syntax: &SyntaxReference = syntaxset.find_syntax_by_extension("tsx").unwrap();

//     let mut highlighter = HighlightLines::new(&syntax, &theme);
//     let source = r#"
// import { foo } from 'bar';
// interface Person {
//     name: string;
// }
// async function x*() {
//     const element = <div>Hello, JSX!</div>;
//     console.log(greet(user));
//     yield await fetch('https://example.com');
// }
//     "#;
//     let lines = LinesWithEndings::from(source);
//     for line in lines {
//         let ranges: Vec<(Style, &str)> = highlighter.highlight_line(line, &syntaxset).unwrap();
//         let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
//         print!("{}", escaped);
//     }
// }