use std::io::stdout;

use rsquickjs::prelude::Rest;
use rsquickjs::{AsyncContext, AsyncRuntime, CatchResultExt};
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor};
use xmas_js_modules::console::write_log;
use xmas_js_modules::permissions::Permissions;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rl = DefaultEditor::new()?;
    if rl.load_history("history.js").is_err() {
        println!("No previous history.");
    }
    let runtime = AsyncRuntime::new()?;
    let context = AsyncContext::full(&runtime).await?;
    rsquickjs::async_with!(context => |ctx| {
        xmas_js_modules::init(&ctx, Permissions::allow_all(), xmas_js_modules::console::LogType::Stdio
    )?;
    loop {
        let readline = rl.readline(">> ");
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