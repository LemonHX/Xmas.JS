use rsquickjs::{prelude::Func, ArrayBuffer, Ctx, Result, Value};

pub fn init<'js>(ctx: &Ctx<'js>) -> Result<()> {
    let globals = ctx.globals();
    globals.set(
        "internalSerialize",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::ArrayBuffer<'js>> {
                let array_buffer = value.serialize()?;
                let array_buffer: rsquickjs::ArrayBuffer<'js> =
                    rsquickjs::ArrayBuffer::new(ctx.clone(), array_buffer)?;
                Ok(array_buffer)
            },
        ),
    )?;

    globals.set(
        "internalDeserialize",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::Value<'js>> {
                if value.is_array_buffer() {
                    let vec = ArrayBuffer::from_value(value).unwrap();
                    let vec = match vec.as_bytes() {
                        Some(b) => b,
                        None => {
                            return Err(rsquickjs::Error::new_from_js(
                                "TypeError",
                                "Failed to get bytes from ArrayBuffer",
                            ))
                        }
                    };
                    let value = Value::deserialize(ctx, vec)?;
                    return Ok(value);
                } else {
                    return Err(rsquickjs::Error::new_from_js(
                        "TypeError",
                        "Value must be an ArrayBuffer",
                    ));
                }
            },
        ),
    )?;

    globals.set(
        "structuredClone",
        Func::from(
            |ctx: Ctx<'js>, value: rsquickjs::Value<'js>| -> Result<rsquickjs::Value<'js>> {
                let vec = value.serialize()?;
                let value = Value::deserialize(ctx, &vec)?;
                Ok(value)
            },
        ),
    )?;

    Ok(())
}
