use std::io::stdout;
use colored::*;
use rsquickjs::prelude::Rest;
use rsquickjs::{AsyncContext, AsyncRuntime, CatchResultExt};
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
        xmas_js_modules::init(&ctx, Permissions::allow_all(), xmas_js_modules::console::LogType::Stdio
    )?;
    loop {
        let readline = rl.readline("🎄 >> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                
                ctx.eval::<rsquickjs::Value, _>(line.as_bytes())
                .and_then(|ret| write_log(stdout(), &ctx, Rest(vec![ret])))
                .catch(&ctx)
                .unwrap_or_else(|err| eprintln!("{err}"));
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C received, exiting...");
                break
            },
            Err(ReadlineError::Eof) => {
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