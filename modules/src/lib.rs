#![feature(iter_array_chunks)]
use crate::utils::primordials::Primordial;

#[cfg(feature = "abort")]
pub mod abort;
pub mod exceptions;

pub mod buffer;
pub mod path;
pub mod permissions;

#[cfg(feature = "crypto")]
pub mod crypto;

#[cfg(feature = "event")]
pub mod event;

#[cfg(feature = "console")]
pub mod console;

#[cfg(feature = "source")]
pub mod script;

#[cfg(feature = "fs")]
pub mod fs;

#[cfg(feature = "tls")]
pub mod tls;

#[cfg(feature = "dns")]
pub mod dns;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "fetch")]
pub mod fetch;

#[cfg(feature = "url")]
pub mod url;

#[cfg(feature = "intl")]
pub mod intl;

pub mod async_hooks;
pub mod hooking;
pub mod module;
pub mod navigator;
pub mod serdeserclone;
pub mod text;
pub mod timers;
pub mod utils;

pub fn init(
    ctx: &rsquickjs::Ctx,
    vsys: std::sync::Arc<xmas_vsys::Vsys>,
    #[cfg(feature = "console")] log_type: console::LogType,
) -> rsquickjs::Result<()> {
    navigator::init(ctx)?;
    utils::primordials::BasePrimordials::init(ctx)?;
    permissions::init(ctx.clone(), vsys)?;
    exceptions::init(ctx)?;
    async_hooks::init(ctx)?;
    text::init(ctx)?;
    serdeserclone::init(ctx)?;
    module::module::init(ctx)?;
    buffer::init(ctx)?;
    timers::init(ctx)?;

    #[cfg(feature = "crypto")]
    {
        crypto::init(ctx)?;
    }
    #[cfg(feature = "source")]
    {
        script::init(ctx)?;
    }
    #[cfg(feature = "event")]
    {
        event::init(ctx)?;
    }
    #[cfg(feature = "abort")]
    {
        abort::init(ctx)?;
    }
    #[cfg(feature = "console")]
    {
        console::init(ctx, log_type)?;
    }
    #[cfg(feature = "url")]
    {
        url::init(ctx)?;
    }
    #[cfg(feature = "fetch")]
    {
        fetch::init(ctx)?;
    }
    #[cfg(feature = "intl")]
    {
        intl::init(ctx)?;
    }
    Ok(())
}
