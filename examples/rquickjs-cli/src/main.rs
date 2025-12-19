use std::io::Write;

use rquickjs::{CatchResultExt, AsyncContext, Function, Object, Result, AsyncRuntime, Value};

fn print(s: String) {
    println!("{s}");
}

#[tokio::main]
async fn main() -> Result<()> {
    let rt = AsyncRuntime::new()?;
    let ctx = AsyncContext::full(&rt).await?;

    ctx.with(|ctx| -> Result<()> {
        let global = ctx.globals();
        global.set(
            "__print",
            Function::new(ctx.clone(), print)?.with_name("__print")?,
        )?;
        ctx.eval::<(), _>(
            r#"
globalThis.console = {
  log(...v) {
    globalThis.__print(`${v.join(" ")}`)
  }
}
"#,
        )?;

        let console: Object = global.get("console")?;
        let js_log: Function = console.get("log")?;
        loop {
            let mut input = String::new();
            print!("> ");
            std::io::stdout().flush()?;
            std::io::stdin().read_line(&mut input)?;
            ctx.eval::<Value, _>(input.as_bytes())
                .and_then(|ret| js_log.call::<(Value<'_>,), ()>((ret,)))
                .catch(&ctx)
                .unwrap_or_else(|err| println!("{err}"));
        }
    }).await?;

    Ok(())
}
