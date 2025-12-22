#![feature(iter_array_chunks)]
use crate::utils::primordials::Primordial;

pub mod permissions;

pub mod buffer;

pub mod path;

#[cfg(feature = "event")]
pub mod event;

#[cfg(feature = "console")]
pub mod console;

#[cfg(feature = "source")]
pub mod source;

#[cfg(feature = "fs")]
pub mod fs;

pub mod utils;

pub fn init(
    ctx: &rsquickjs::Ctx,
    permissions: permissions::Permissions,
    #[cfg(feature = "console")] log_type: console::LogType,
) -> rsquickjs::Result<()> {
    utils::primordials::BasePrimordials::init(ctx)?;
    permissions::init(ctx.clone(), permissions)?;
    buffer::init(&ctx)?;
    #[cfg(feature = "event")]
    {
        event::init(ctx)?;
    }
    #[cfg(feature = "console")]
    {
        console::init(ctx, log_type)?;
    }

    Ok(())
}
