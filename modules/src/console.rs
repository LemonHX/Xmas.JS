use std::io::{stderr, stdout, IsTerminal, Write};

// use llrt_logging::{build_formatted_string, FormatOptions, NEWLINE};
use crate::utils::{
    console::{build_formatted_string, FormatOptions, NEWLINE},
    module::{export_default, ModuleInfo},
};
use rquickjs::{
    module::{Declarations, Exports, ModuleDef},
    prelude::{Func, Rest},
    Class, Ctx, Object, Result, Value,
};

#[derive(Debug, Clone, PartialEq, Eq, rquickjs::class::Trace, rquickjs::JsLifetime)]
pub enum LogType {
    Stdio,
    Trace,
}

#[derive(rquickjs::class::Trace, rquickjs::JsLifetime)]
#[rquickjs::class]
pub struct Console {}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

#[rquickjs::methods(rename_all = "camelCase")]
impl Console {
    #[qjs(constructor)]
    pub fn new() -> Self {
        // We ignore the parameters for now since we don't support stream
        Self {}
    }

    pub fn log<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log(ctx, args)
    }

    pub fn clear<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        clear(ctx, args)
    }
    pub fn debug<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log_debug(ctx, args)
    }
    pub fn info<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log(ctx, args)
    }
    pub fn trace<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log_trace(ctx, args)
    }
    pub fn error<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log_error(ctx, args)
    }
    pub fn warn<'js>(&self, ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
        log_warn(ctx, args)
    }
    pub fn assert<'js>(
        &self,
        ctx: Ctx<'js>,
        expression: bool,
        args: Rest<Value<'js>>,
    ) -> Result<()> {
        log_assert(ctx, expression, args)
    }
}

fn get_modeule_name_helper(ctx: Ctx<'_>) -> String {
    ctx.script_or_module_name(1)
        .map(|a| a.to_string())
        .unwrap_or_else(|| Ok("unknown".to_string()))
        .unwrap_or_else(|_| "unknown".to_string())
}

pub fn log<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => write_log(stdout(), &ctx, args),
            LogType::Trace => {
                let module_name = get_modeule_name_helper(ctx.clone());
                format_log(false, true, &ctx, args).map(|msg| {
                    tracing::info!(module = module_name, "{}", msg);
                })
            }
        })
        .unwrap()
}

pub fn log_fatal<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    log_error(ctx, args)
}

pub fn log_error<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => write_log(stderr(), &ctx, args),
            LogType::Trace => {
                let module_name = get_modeule_name_helper(ctx.clone());
                format_log(false, true, &ctx, args).map(|msg| {
                    tracing::error!(module = module_name, "{}", msg);
                })
            }
        })
        .unwrap()
}

fn log_warn<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => write_log(stderr(), &ctx, args),
            LogType::Trace => {
                let module_name = get_modeule_name_helper(ctx.clone());
                format_log(false, true, &ctx, args).map(|msg| {
                    tracing::warn!(module = module_name, "{}", msg);
                })
            }
        })
        .unwrap()
}

fn log_debug<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => write_log(stderr(), &ctx, args),
            LogType::Trace => {
                let module_name = get_modeule_name_helper(ctx.clone());
                format_log(false, true, &ctx, args).map(|msg| {
                    tracing::debug!(module = module_name, "{}", msg);
                })
            }
        })
        .unwrap()
}

fn log_trace<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => write_log(stderr(), &ctx, args),
            LogType::Trace => {
                let module_name = get_modeule_name_helper(ctx.clone());
                format_log(false, true, &ctx, args).map(|msg| {
                    tracing::trace!(module = module_name, "{}", msg);
                })
            }
        })
        .unwrap()
}

fn log_assert<'js>(ctx: Ctx<'js>, expression: bool, args: Rest<Value<'js>>) -> Result<()> {
    if !expression {
        log_error(ctx, args)
    } else {
        Ok(())
    }
}

fn clear<'js>(ctx: Ctx<'js>, _args: Rest<Value<'js>>) -> Result<()> {
    ctx.userdata::<LogType>()
        .map(|log_type| match *log_type {
            LogType::Stdio => {
                let _ = stdout().write_all(b"\x1b[1;1H\x1b[0J");
            }
            LogType::Trace => {
                // no op
            }
        })
        .unwrap();
    Ok(())
}

fn format_log<'js>(
    color: bool,
    newline: bool,
    ctx: &Ctx<'js>,
    args: Rest<Value<'js>>,
) -> Result<String> {
    let mut result = String::new();
    let mut options = FormatOptions::new(ctx, color, newline)?;
    build_formatted_string(&mut result, ctx, args, &mut options)?;
    Ok(result)
}

pub fn write_log<'js, T>(mut output: T, ctx: &Ctx<'js>, args: Rest<Value<'js>>) -> Result<()>
where
    T: Write + IsTerminal,
{
    let is_tty = output.is_terminal();
    let mut log = format_log(is_tty, true, ctx, args)?;
    log.push(NEWLINE);

    // we don't care if output is interrupted
    let _ = output.write_all(log.as_bytes());
    Ok(())
}

pub struct ConsoleModule;

impl ModuleDef for ConsoleModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare(stringify!(Console))?;
        declare.declare("default")?;

        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        export_default(ctx, exports, |default| {
            Class::<Console>::define(default)?;
            Ok(())
        })
    }
}

impl From<ConsoleModule> for ModuleInfo<ConsoleModule> {
    fn from(val: ConsoleModule) -> Self {
        ModuleInfo {
            name: "console",
            module: val,
        }
    }
}

pub fn init(ctx: &Ctx<'_>, log_type: LogType) -> Result<()> {
    ctx.store_userdata(log_type)?;
    let globals = ctx.globals();

    let console = Object::new(ctx.clone())?;

    console.set("assert", Func::from(log_assert))?;
    console.set("clear", Func::from(clear))?;
    console.set("debug", Func::from(log_debug))?;
    console.set("error", Func::from(log_error))?;
    console.set("info", Func::from(log))?;
    console.set("log", Func::from(log))?;
    console.set("trace", Func::from(log_trace))?;
    console.set("warn", Func::from(log_warn))?;

    globals.set("console", console)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::utils::primordials::Primordial;

    #[tokio::test]
    async fn test_console() {
        use super::{init, LogType};
        use rquickjs::AsyncContext;
        use rquickjs::AsyncRuntime;
        use rquickjs::Function;
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        // tracing_subscriber::fmt::init();
        fn print(s: String) {
            println!("{s}");
        }
        ctx.with(|ctx| -> rquickjs::Result<()> {
            let global = ctx.globals();
            global.set(
                "__print",
                Function::new(ctx.clone(), print)?.with_name("__print")?,
            )?;
            crate::utils::primordials::BasePrimordials::init(&ctx)?;
            init(&ctx, LogType::Stdio).unwrap();
            ctx.eval::<(), _>(
                r#"
        __print("Console module initialized.");

        console.log("Hello, world!", 1);
        console.debug("Debug message", true);
        console.info("Info message", { key: "value" });
        console.warn("Warning message", [1, 2, 3]);
        console.error("Error message", null);
        console.trace("Trace message", 3.14);
        console.assert(true, "This should not log");
        console.assert(false, "This should log an error");
    "#,
            )
        })
        .await
        .unwrap()
    }
}
