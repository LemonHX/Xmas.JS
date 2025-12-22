use std::path::Path;

use crate::{fs::access::check_could_ctx_access_permission, utils::result::ResultExt};
use rsquickjs::{Ctx, Exception, Result};

pub(crate) fn rename_error(from: &str, to: &str) -> String {
    [
        "Can't rename file/folder from \"",
        from,
        "\" to \"",
        to,
        "\"",
    ]
    .concat()
}

pub async fn rename(ctx: Ctx<'_>, old_path: String, new_path: String) -> Result<()> {
    if !check_could_ctx_access_permission(&ctx, &Path::new(&old_path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    tokio::fs::rename(&old_path, &new_path)
        .await
        .or_throw_msg(&ctx, &rename_error(&old_path, &new_path))?;
    Ok(())
}

pub fn rename_sync(ctx: Ctx<'_>, old_path: String, new_path: String) -> Result<()> {
    if !check_could_ctx_access_permission(&ctx, &Path::new(&old_path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    std::fs::rename(&old_path, &new_path)
        .or_throw_msg(&ctx, &rename_error(&old_path, &new_path))?;
    Ok(())
}
