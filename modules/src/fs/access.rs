use std::{
    fs::Metadata,
    path::{self, Path},
};

#[allow(dead_code, unused_imports)]
use super::{CONSTANT_F_OK, CONSTANT_R_OK, CONSTANT_W_OK, CONSTANT_X_OK};
use crate::{permissions, utils::result::ResultExt};
use rsquickjs::{prelude::Opt, Ctx, Exception, Result};
use tokio::fs;

// if !check_could_ctx_access_permission(&ctx, &path) {
//     return Err(Exception::throw_message(
//         &ctx,
//         "Permission denied. Cannot access the file",
//     ));
// }
pub fn check_could_ctx_access_permission(ctx: &Ctx, path: &Path) -> bool {
    let user_permissions = ctx.userdata::<crate::permissions::Permissions>().unwrap();
    let file_permission = &user_permissions.fs;
    let mut white_list = false;
    let items = match file_permission {
        permissions::BlackOrWhiteList::BlackList(items) => items,
        permissions::BlackOrWhiteList::WhiteList(items) => {
            white_list = true;
            items
        }
    };

    // split path to pattern path and normal path
    let mut normal_paths = vec![];
    let mut pattern_paths = vec![];
    for item in items {
        if item.contains('*') {
            pattern_paths.push(
                path::Path::new(&item[..item.len() - 1])
                    .canonicalize()
                    .unwrap_or_else(|_| path::PathBuf::from(item)),
            );
        } else {
            normal_paths.push(
                path::Path::new(item)
                    .canonicalize()
                    .unwrap_or_else(|_| path::PathBuf::from(item)),
            );
        }
    }

    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    if let Some(p) = canonical_path.as_path().parent() {
        let found = pattern_paths.iter().find(|x| x == &p).is_some();
        if white_list {
            return found;
        } else {
            return !found;
        }
    }

    let found = normal_paths
        .iter()
        .find(|x| *x == &canonical_path)
        .is_some();
    if white_list {
        return found;
    } else {
        return !found;
    }
}

pub async fn access(ctx: Ctx<'_>, path: String, mode: Opt<u32>) -> Result<()> {
    let metadata = fs::metadata(&path).await.or_throw_msg(
        &ctx,
        &["No such file or directory \"", &path, "\""].concat(),
    )?;
    if !check_could_ctx_access_permission(&ctx, Path::new(&path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    verify_metadata(&ctx, mode, metadata)
}

pub fn access_sync(ctx: Ctx<'_>, path: String, mode: Opt<u32>) -> Result<()> {
    let metadata = std::fs::metadata(path.clone()).or_throw_msg(
        &ctx,
        &["No such file or directory \"", &path, "\""].concat(),
    )?;

    if !check_could_ctx_access_permission(&ctx, Path::new(&path)) {
        return Err(Exception::throw_message(
            &ctx,
            "Permission denied. Cannot access the file",
        ));
    }
    verify_metadata(&ctx, mode, metadata)
}

fn verify_metadata(ctx: &Ctx, mode: Opt<u32>, metadata: Metadata) -> Result<()> {
    let permissions = metadata.permissions();

    let mode = mode.unwrap_or(CONSTANT_F_OK);

    if mode & CONSTANT_W_OK != 0 && permissions.readonly() {
        return Err(Exception::throw_message(
            ctx,
            "Permission denied. File not writable",
        ));
    }

    if mode & CONSTANT_X_OK != 0 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if permissions.mode() & 0o100 == 0 {
                return Err(Exception::throw_message(
                    ctx,
                    "Permission denied. File not executable",
                ));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
            // Get the file attributes
            let file_attributes = metadata.file_attributes();

            // Check if the file has execute permissions
            if file_attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                return Err(Exception::throw_message(ctx, "Permission denied"));
            }
        }
    }

    Ok(())
}
