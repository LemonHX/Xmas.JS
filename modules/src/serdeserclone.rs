use rsquickjs::{prelude::Func, ArrayBuffer, Ctx, Result, TypedArray, Value};

pub fn init<'js>(ctx: &Ctx<'js>) -> Result<()> {
    let globals = ctx.globals();
    globals.set(
        "internalSerialize",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::Value<'js>> {
                value.serialize(ctx)
            },
        ),
    )?;

    globals.set(
        "internalDeserialize",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::Value<'js>> {
                value.deserialize(ctx)
            },
        ),
    )?;

    globals.set(
        "structuredClone",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::Value<'js>> {
                value.structured_clone(ctx)
            },
        ),
    )?;

    Ok(())
}
