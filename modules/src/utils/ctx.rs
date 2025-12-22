use super::primordials::{BasePrimordials, Primordial};
use rsquickjs::{atom::PredefinedAtom, CatchResultExt, CaughtError, Object};
use rsquickjs::{Ctx, Result};
use std::future::Future;
use std::ptr::NonNull;
use std::sync::OnceLock;
use tokio::sync::oneshot::{self, Receiver};

pub trait CtxExt {
    fn get_script_or_module_name(&self) -> Result<String>;
}

impl CtxExt for Ctx<'_> {
    fn get_script_or_module_name(&self) -> Result<String> {
        if let Some(name) = self.script_or_module_name(0) {
            name.to_string()
        } else {
            Ok(String::from("."))
        }
    }
}

#[allow(clippy::type_complexity)]
static ERROR_HANDLER: OnceLock<Box<dyn for<'js> Fn(&Ctx<'js>, CaughtError<'js>) + Sync + Send>> =
    OnceLock::new();

pub trait CtxExtension<'js> {
    /// Despite naming, this will not necessarily exit the parent process.
    /// It depends on the handler set by `set_spawn_error_handler`.
    fn spawn_exit<F, R>(&self, future: F) -> Result<Receiver<R>>
    where
        F: Future<Output = Result<R>> + 'js,
        R: 'js;

    fn spawn_exit_simple<F>(&self, future: F)
    where
        F: Future<Output = Result<()>> + 'js;

    fn get_background_task_poller(&self) -> tokio::task::JoinHandle<()>;
}

impl<'js> CtxExtension<'js> for Ctx<'js> {
    fn spawn_exit<F, R>(&self, future: F) -> Result<Receiver<R>>
    where
        F: Future<Output = Result<R>> + 'js,
        R: 'js,
    {
        let ctx = self.clone();

        let primordials = BasePrimordials::get(self)?;
        let type_error: Object = primordials.constructor_type_error.construct(())?;
        let stack: Option<String> = type_error.get(PredefinedAtom::Stack).ok();

        let (join_channel_tx, join_channel_rx) = oneshot::channel();

        self.spawn(async move {
            match future.await.catch(&ctx) {
                Ok(res) => {
                    //result here doesn't matter if receiver has dropped
                    let _ = join_channel_tx.send(res);
                }
                Err(err) => handle_spawn_error(&ctx, err, stack),
            }
        });
        Ok(join_channel_rx)
    }

    /// Same as above but fire & forget and without a forced stack trace collection
    fn spawn_exit_simple<F>(&self, future: F)
    where
        F: Future<Output = Result<()>> + 'js,
    {
        let ctx = self.clone();
        self.spawn(async move {
            if let Err(err) = future.await.catch(&ctx) {
                handle_spawn_error(&ctx, err, None)
            }
        });
    }

    /// Get a background task poller handle
    fn get_background_task_poller(&self) -> tokio::task::JoinHandle<()> {
        let ctx1 = self.clone().as_raw().as_ptr() as usize;
        let t = tokio::spawn(async move {
            let ctx = unsafe { Ctx::from_raw(NonNull::new(ctx1 as *mut _).unwrap()) };
            loop {
                ctx.await_background_once();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
        return t;
    }
}

fn handle_spawn_error<'js>(ctx: &Ctx<'js>, err: CaughtError<'js>, stack: Option<String>) {
    let error_handler = if let Some(handler) = ERROR_HANDLER.get() {
        handler
    } else {
        tracing::error!("Future error: {:?}", err);
        return;
    };
    if let CaughtError::Exception(err) = err {
        if err.stack().is_none() {
            if let Some(stack) = stack {
                err.set(PredefinedAtom::Stack, stack).unwrap();
            }
        }
        error_handler(ctx, CaughtError::Exception(err));
    } else {
        error_handler(ctx, err);
    }
}

pub fn set_spawn_error_handler<F>(handler: F)
where
    F: for<'js> Fn(&Ctx<'js>, CaughtError<'js>) + Sync + Send + 'static,
{
    _ = ERROR_HANDLER.set(Box::new(handler));
}
