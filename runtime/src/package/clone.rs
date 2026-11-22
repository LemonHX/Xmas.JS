//! Cloning a package directory, in one call to the kernel.
//!
//! On APFS a clone is copy-on-write: the directory tree appears at the destination sharing its
//! blocks with the store, and the call returns before anything is written. Measured on the tree
//! this was written for: 1257 packages and 38k files in one second, against eleven for a hard
//! link per file, which is what happens everywhere else.
//!
//! A clone shares blocks, not the inode, so a tool that rewrites a file in `node_modules` cannot
//! reach back into the store and change what the next install places.

use std::path::Path;

/// Put a copy of `src` at `dst`, or say that this filesystem will not.
///
/// `dst` must not exist. A `false` is not an error: it means the caller should place the files
/// itself, which is what every other platform does.
#[cfg(target_os = "macos")]
pub fn clone_tree(src: &Path, dst: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let (Ok(from), Ok(to)) = (
        CString::new(src.as_os_str().as_bytes()),
        CString::new(dst.as_os_str().as_bytes()),
    ) else {
        return false;
    };

    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call, and
    // `clonefile` reads nothing else from this process.
    unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) == 0 }
}

/// No clone on this platform: the caller places the files one by one.
#[cfg(not(target_os = "macos"))]
pub fn clone_tree(src: &Path, dst: &Path) -> bool {
    let _ = (src, dst);
    false
}
