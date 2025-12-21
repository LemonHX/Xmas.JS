use std::ptr::NonNull;

use crate::qjs;

use std::sync::Arc;

/// Trait to specify how to drop a context once it goes out of scope.
/// Implemented on Runtime and AsyncRuntime.
pub(crate) trait DropContext: Clone {
    unsafe fn drop_context(&self, ctx: NonNull<qjs::JSContext>);
}

unsafe impl<R: Send + DropContext> Send for ContextOwner<R> {}

/// Struct in charge of dropping contexts when they go out of scope
pub(crate) struct ContextOwner<R: DropContext> {
    pub(crate) ctx: Arc<NonNull<qjs::JSContext>>,
    pub(crate) rt: R,
}

impl<R: DropContext> ContextOwner<R> {
    pub(crate) unsafe fn new(ctx: NonNull<qjs::JSContext>, rt: R) -> Self {
        Self {
            ctx: Arc::new(ctx),
            rt,
        }
    }



    pub(crate) fn ctx(&self) -> NonNull<qjs::JSContext> {
        *self.ctx
    }

    pub(crate) fn rt(&self) -> &R {
        &self.rt
    }
}



impl<R: DropContext> Clone for ContextOwner<R> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            rt: self.rt.clone(),
        }
    }
}

impl<R: DropContext> Drop for ContextOwner<R> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.ctx) == 1 {
            unsafe { self.rt.drop_context(self.ctx()) }
        }
    }
}
