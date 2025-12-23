use crate::event::Emitter;
use crate::utils::primordials::{BasePrimordials, Primordial};
use rsquickjs::{Class, Ctx, Result};

pub use self::{abort_controller::AbortController, abort_signal::AbortSignal};

mod abort_controller;
mod abort_signal;

pub fn init(ctx: &Ctx<'_>) -> Result<()> {
    let globals = ctx.globals();

    BasePrimordials::init(ctx)?;

    Class::<AbortController>::define(&globals)?;
    Class::<AbortSignal>::define(&globals)?;

    AbortSignal::add_event_emitter_prototype(ctx)?;
    AbortSignal::add_event_target_prototype(ctx)?;

    Ok(())
}
