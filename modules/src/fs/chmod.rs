use crate::fs::access::check_could_ctx_access_permission;
#[cfg(unix)]
use crate::utils::result::ResultExt;
use rsquickjs::{Ctx, Exception, Result};
#[cfg(unix)]
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;

#[cfg(unix)]
pub(crate) fn chmod_error(path: &str) -> String {
    ["Can't set permissions of \"", path, "\""].concat()
}

pub(crate) async fn set_mode(ctx: Ctx<'_>, path: &str, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::set_permissions(path, PermissionsExt::from_mode(mode))
            .await
            .or_throw_msg(&ctx, &chmod_error(path))?;
    }
    #[cfg(not(unix))]
    {
        _ = ctx;
        _ = path;
        _ = mode;
    }
    Ok(())
}

pub(crate) fn set_mode_sync(ctx: Ctx<'_>, path: &str, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::set_permissions(path, PermissionsExt::from_mode(mode))
            .or_throw_msg(&ctx, &chmod_error(path))?;
    }
    #[cfg(not(unix))]
    {
        _ = ctx;
        _ = path;
        _ = mode;
    }
    Ok(())
}

pub async fn chmod(ctx: Ctx<'_>, path: String, mode: u32) -> Result<()> {
    if !check_could_ctx_access_permission(&ctx, &Path::new(&path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    set_mode(ctx, &path, mode).await
}

pub fn chmod_sync(ctx: Ctx<'_>, path: String, mode: u32) -> Result<()> {
    if !check_could_ctx_access_permission(&ctx, &Path::new(&path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    set_mode_sync(ctx, &path, mode)
}
