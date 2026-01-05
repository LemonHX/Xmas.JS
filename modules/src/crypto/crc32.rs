use std::hash::Hasher;

use crate::utils::bytes::ObjectBytes;
use crc32c::Crc32cHasher;
use rsquickjs::{prelude::This, Class, Ctx, Result};

#[rsquickjs::class]
#[derive(rsquickjs::class::Trace, rsquickjs::JsLifetime)]
pub struct Crc32c {
    #[qjs(skip_trace)]
    hasher: crc32c::Crc32cHasher,
}

#[rsquickjs::methods]
impl Crc32c {
    #[qjs(constructor)]
    fn new() -> Self {
        Self {
            hasher: Crc32cHasher::default(),
        }
    }

    #[qjs(rename = "digest")]
    fn crc32c_digest(&self) -> u64 {
        self.hasher.finish()
    }

    #[qjs(rename = "update")]
    fn crc32c_update<'js>(
        this: This<Class<'js, Self>>,
        ctx: Ctx<'js>,
        bytes: ObjectBytes<'js>,
    ) -> Result<Class<'js, Self>> {
        this.0.borrow_mut().hasher.write(bytes.as_bytes(&ctx)?);
        Ok(this.0)
    }
}

#[rsquickjs::class]
#[derive(rsquickjs::class::Trace, rsquickjs::JsLifetime)]
pub struct Crc32 {
    #[qjs(skip_trace)]
    hasher: crc32fast::Hasher,
}

#[rsquickjs::methods]
impl Crc32 {
    #[qjs(constructor)]
    fn new() -> Self {
        Self {
            hasher: crc32fast::Hasher::new(),
        }
    }

    #[qjs(rename = "digest")]
    fn crc32_digest(&self) -> u64 {
        self.hasher.finish()
    }

    #[qjs(rename = "update")]
    fn crc32_update<'js>(
        this: This<Class<'js, Self>>,
        ctx: Ctx<'js>,
        bytes: ObjectBytes<'js>,
    ) -> Result<Class<'js, Self>> {
        this.0.borrow_mut().hasher.write(bytes.as_bytes(&ctx)?);
        Ok(this.0)
    }
}
