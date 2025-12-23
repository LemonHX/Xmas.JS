use crate::{qjs, Ctx, Error, FromJs, IntoJs, JsLifetime, Result, Value};

use std::{
    fmt,
    mem::{self, ManuallyDrop},
};

/// The wrapper for JS values to keep it from GC
///
/// For example you can store JS functions for later use.
/// ```rust,ignore
/// # use rquickjs::{AsyncRuntime, AsyncContext, Persistent, Function};
/// # let rt = AsyncRuntime::new().unwrap();
/// # let ctx = AsyncContext::full(&rt).await.unwrap();
/// let func = ctx.with(|ctx| {
///     Persistent::save(&ctx, ctx.eval::<Function, _>("a => a + 1").unwrap())
/// }).await;
/// let res: i32 = ctx.with(|ctx| {
///     let func = func.clone().restore(&ctx).unwrap();
///     func.call((2,)).unwrap()
/// }).await;
/// assert_eq!(res, 3);
/// let res: i32 = ctx.with(|ctx| {
///     let func = func.restore(&ctx).unwrap();
///     func.call((0,)).unwrap()
/// }).await;
/// assert_eq!(res, 1);
/// ```
///
/// It is an error (`Error::UnrelatedRuntime`) to restore the `Persistent` in a
/// context who isn't part of the original `Runtime`.
///
/// NOTE: Be careful and ensure that no persistent links outlives the runtime,
/// otherwise Runtime will abort the process when dropped.
///
#[derive(Eq, PartialEq, Hash)]
pub struct Persistent<T> {
    pub(crate) rt: *mut qjs::JSRuntime,
    pub(crate) value: T,
}

unsafe impl<T: Send> Send for Persistent<T> {}
unsafe impl<T: Sync> Sync for Persistent<T> {}

impl<T: Clone> Clone for Persistent<T> {
    fn clone(&self) -> Self {
        Persistent {
            rt: self.rt,
            value: self.value.clone(),
        }
    }
}

impl<T> fmt::Debug for Persistent<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Persistent")
            .field("rt", &self.rt)
            .field("value", &self.value)
            .finish()
    }
}

impl<T> Persistent<T> {
    unsafe fn outlive_transmute<'from, 'to, U>(t: U) -> U::Changed<'to>
    where
        U: JsLifetime<'from>,
    {
        // extremely unsafe code which should be safe if outlive is implemented correctly.

        // assertion to check if T and T::Target are the same size, they should be.
        // should compile away if they are the same size.
        assert_eq!(mem::size_of::<U>(), mem::size_of::<U::Changed<'static>>());
        assert_eq!(mem::align_of::<U>(), mem::align_of::<U::Changed<'static>>());

        // union to transmute between two unrelated types
        // Can't use transmute since it is unable to determine the size of both values.
        union Transmute<A, B> {
            a: ManuallyDrop<A>,
            b: ManuallyDrop<B>,
        }
        let data = Transmute::<U, U::Changed<'to>> {
            a: ManuallyDrop::new(t),
        };
        unsafe { ManuallyDrop::into_inner(data.b) }
    }

    /// Save the value of an arbitrary type
    pub fn save<'js>(ctx: &Ctx<'js>, val: T) -> Persistent<T::Changed<'static>>
    where
        T: JsLifetime<'js>,
    {
        let outlived: T::Changed<'static> =
            unsafe { Self::outlive_transmute::<'js, 'static, T>(val) };
        let ptr = unsafe { qjs::JS_GetRuntime(ctx.as_ptr()) };
        Persistent {
            rt: ptr,
            value: outlived,
        }
    }

    /// Restore the value of an arbitrary type
    pub fn restore<'js>(self, ctx: &Ctx<'js>) -> Result<T::Changed<'js>>
    where
        T: JsLifetime<'static>,
    {
        let ctx_runtime_ptr = unsafe { qjs::JS_GetRuntime(ctx.as_ptr()) };
        if self.rt != ctx_runtime_ptr {
            return Err(Error::UnrelatedRuntime);
        }
        Ok(unsafe { Self::outlive_transmute::<'static, 'js, T>(self.value) })
    }
}

impl<'js, T, R> FromJs<'js> for Persistent<R>
where
    R: JsLifetime<'static, Changed<'js> = T>,
    T: JsLifetime<'js, Changed<'static> = R> + FromJs<'js>,
{
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Persistent<R>> {
        let value = T::from_js(ctx, value)?;
        Ok(Persistent::save(ctx, value))
    }
}

impl<'js, T> IntoJs<'js> for Persistent<T>
where
    T: JsLifetime<'static>,
    T::Changed<'js>: IntoJs<'js>,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.restore(ctx)?.into_js(ctx)
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[tokio::test]
    #[should_panic(expected = "UnrelatedRuntime")]
    async fn different_runtime() {
        let rt1 = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt1).await.unwrap();
        let persistent_v = ctx
            .with(|ctx| {
                let v: Value = ctx.eval("1").unwrap();
                Persistent::save(&ctx, v)
            })
            .await;

        let rt2 = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt2).await.unwrap();
        ctx.with(|ctx| {
            let _ = persistent_v.clone().restore(&ctx).unwrap();
        })
        .await
    }

    #[tokio::test]
    async fn different_context() {
        let rt1 = AsyncRuntime::new().unwrap();
        let ctx1 = AsyncContext::full(&rt1).await.unwrap();
        let ctx2 = AsyncContext::full(&rt1).await.unwrap();

        let persistent_v = ctx1
            .with(|ctx| {
                let v: Object = ctx.eval("({ a: 1 })").unwrap();
                Persistent::save(&ctx, v)
            })
            .await;

        std::mem::drop(ctx1);

        ctx2.with(|ctx| {
            let obj: Object = persistent_v.clone().restore(&ctx).unwrap();
            assert_eq!(obj.get::<_, i32>("a").unwrap(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn persistent_function() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        let func = ctx
            .with(|ctx| {
                let func: Function = ctx.eval("a => a + 1").unwrap();
                Persistent::save(&ctx, func)
            })
            .await;

        let res: i32 = ctx
            .with(|ctx| {
                let func = func.clone().restore(&ctx).unwrap();
                func.call((2,)).unwrap()
            })
            .await;
        assert_eq!(res, 3);

        let ctx2 = AsyncContext::full(&rt).await.unwrap();
        let res: i32 = ctx2
            .with(|ctx| {
                let func = func.restore(&ctx).unwrap();
                func.call((0,)).unwrap()
            })
            .await;
        assert_eq!(res, 1);
    }

    #[tokio::test]
    async fn persistent_value() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        let persistent_v = ctx
            .with(|ctx| {
                let v: Value = ctx.eval("1").unwrap();
                Persistent::save(&ctx, v)
            })
            .await;

        ctx.with(|ctx| {
            let v = persistent_v.clone().restore(&ctx).unwrap();
            ctx.globals().set("v", v).unwrap();
            let eq: Value = ctx.eval("v == 1").unwrap();
            assert!(eq.as_bool().unwrap());
        })
        .await;
    }
}
