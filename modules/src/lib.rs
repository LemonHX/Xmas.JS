use crate::utils::primordials::Primordial;

pub mod permissions;

#[cfg(feature = "event")]
pub mod event;

#[cfg(feature = "console")]
pub mod console;

pub mod utils;

pub fn init(
    ctx: &rquickjs::Ctx, 
    permissions: permissions::Permissions,
    #[cfg(feature = "console")]
    log_type: console::LogType,
) -> rquickjs::Result<()> {
    utils::primordials::BasePrimordials::init(ctx)?;
    permissions::init(ctx.clone(), permissions)?;
    
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