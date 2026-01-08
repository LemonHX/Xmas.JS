//! Permissions module - now delegates to Vsys stored in context
//!
//! Vsys is stored in the JS context and permissions are accessed through it.

use std::path::Path;
use std::sync::Arc;

use rsquickjs::class::{Trace, Tracer};
use rsquickjs::JsLifetime;

// Re-export vsys types
pub use xmas_vsys::fs::FsVTable;
pub use xmas_vsys::permissions::{BlackOrWhiteList, Permissions};
pub use xmas_vsys::Vsys;

/// Wrapper to store Vsys in JS context with required trait implementations
#[derive(Clone)]
pub struct VsysContext(pub Arc<Vsys>);

impl<'js> Trace<'js> for VsysContext {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}

unsafe impl<'js> JsLifetime<'js> for VsysContext {
    type Changed<'to> = VsysContext;
}

impl std::ops::Deref for VsysContext {
    type Target = Vsys;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Initialize the context with a Vsys instance
pub fn init(ctx: rsquickjs::Ctx<'_>, vsys: Arc<Vsys>) -> rsquickjs::Result<()> {
    ctx.store_userdata(VsysContext(vsys))?;
    Ok(())
}

/// Helper to get Vsys from context
pub fn get_vsys(ctx: &rsquickjs::Ctx<'_>) -> Option<Arc<Vsys>> {
    ctx.userdata::<VsysContext>().map(|v| v.0.clone())
}

/// Helper to check filesystem permission from context
pub fn check_fs_permission(ctx: &rsquickjs::Ctx<'_>, path: &Path) -> bool {
    get_vsys(ctx)
        .map(|v| v.permissions().check_fs(path))
        .unwrap_or(false)
}

/// Helper to check network permission from context  
pub fn check_net_permission(ctx: &rsquickjs::Ctx<'_>, host: &str) -> bool {
    get_vsys(ctx)
        .map(|v| v.permissions().check_net(host))
        .unwrap_or(false)
}

/// Helper to get FsVTable from context
/// Returns the filesystem vtable from the Vsys instance, or None if not initialized
pub fn get_fs(ctx: &rsquickjs::Ctx<'_>) -> Option<Arc<Vsys>> {
    get_vsys(ctx)
}

/// Execute a filesystem operation using the vtable from context
/// This is a convenience macro-like function that handles the common pattern
/// of getting vsys, checking permission, and calling the fs operation
pub fn with_fs<F, T>(ctx: &rsquickjs::Ctx<'_>, path: &Path, op: F) -> rsquickjs::Result<T>
where
    F: FnOnce(&FsVTable) -> xmas_vsys::error::VsysResult<T>,
{
    let vsys = get_vsys(ctx).ok_or_else(|| {
        rsquickjs::Error::new_from_js("undefined", "Vsys not initialized in context")
    })?;

    if !vsys.permissions().check_fs(path) {
        return Err(rsquickjs::Exception::throw_message(
            ctx,
            "Permission denied",
        ));
    }

    op(vsys.fs()).map_err(|e| rsquickjs::Exception::throw_message(ctx, &e.to_string()))
}
