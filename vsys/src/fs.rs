//! Filesystem virtual table for vsys
//!
//! This module provides a pluggable filesystem abstraction layer.
//! By default, it uses the real filesystem (std::fs / tokio::fs),
//! but can be replaced with custom implementations.

use std::fs::Metadata;
use std::path::Path;
use std::time::SystemTime;

use crate::error::{VsysError, VsysResult};

/// File type information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Other,
}

/// File statistics (platform-independent subset)
#[derive(Debug, Clone)]
pub struct FileStat {
    pub file_type: FileType,
    pub size: u64,
    pub readonly: bool,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// Unix mode (0 on Windows)
    pub mode: u32,
    /// Unix uid (0 on Windows)
    pub uid: u32,
    /// Unix gid (0 on Windows)
    pub gid: u32,
}

impl FileStat {
    /// Create from std::fs::Metadata
    pub fn from_metadata(metadata: &Metadata) -> Self {
        let file_type = if metadata.is_file() {
            FileType::File
        } else if metadata.is_dir() {
            FileType::Directory
        } else if metadata.is_symlink() {
            FileType::Symlink
        } else {
            FileType::Other
        };

        #[cfg(unix)]
        let (mode, uid, gid) = {
            use std::os::unix::fs::MetadataExt;
            (metadata.mode(), metadata.uid(), metadata.gid())
        };

        #[cfg(not(unix))]
        let (mode, uid, gid) = (0o666, 0, 0);

        Self {
            file_type,
            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            modified: metadata.modified().ok(),
            accessed: metadata.accessed().ok(),
            created: metadata.created().ok(),
            mode,
            uid,
            gid,
        }
    }

    pub fn is_file(&self) -> bool {
        self.file_type == FileType::File
    }

    pub fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type == FileType::Symlink
    }
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
}

/// File open options
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub truncate: bool,
    pub create: bool,
    pub create_new: bool,
    pub mode: u32,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(mut self, read: bool) -> Self {
        self.read = read;
        self
    }

    pub fn write(mut self, write: bool) -> Self {
        self.write = write;
        self
    }

    pub fn append(mut self, append: bool) -> Self {
        self.append = append;
        self
    }

    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    pub fn create_new(mut self, create_new: bool) -> Self {
        self.create_new = create_new;
        self
    }

    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = mode;
        self
    }
}

/// Seek position for file handles
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

impl From<SeekFrom> for std::io::SeekFrom {
    fn from(seek: SeekFrom) -> Self {
        match seek {
            SeekFrom::Start(n) => std::io::SeekFrom::Start(n),
            SeekFrom::End(n) => std::io::SeekFrom::End(n),
            SeekFrom::Current(n) => std::io::SeekFrom::Current(n),
        }
    }
}

/// File handle - opaque wrapper around a file descriptor/handle
///
/// This uses a Box<dyn ...> to allow different implementations
/// while maintaining a consistent interface.
pub struct FsHandle {
    inner: Box<dyn FsHandleOps + Send + Sync>,
}

impl FsHandle {
    pub fn new<T: FsHandleOps + Send + Sync + 'static>(inner: T) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> VsysResult<usize> {
        self.inner.read(buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> VsysResult<usize> {
        self.inner.write(buf)
    }

    pub fn seek(&mut self, pos: SeekFrom) -> VsysResult<u64> {
        self.inner.seek(pos)
    }

    pub fn sync_all(&self) -> VsysResult<()> {
        self.inner.sync_all()
    }

    pub fn sync_data(&self) -> VsysResult<()> {
        self.inner.sync_data()
    }

    pub fn stat(&self) -> VsysResult<FileStat> {
        self.inner.stat()
    }

    pub fn set_len(&self, size: u64) -> VsysResult<()> {
        self.inner.set_len(size)
    }

    pub fn set_permissions(&self, readonly: bool) -> VsysResult<()> {
        self.inner.set_permissions(readonly)
    }

    #[cfg(unix)]
    pub fn set_mode(&self, mode: u32) -> VsysResult<()> {
        self.inner.set_mode(mode)
    }

    #[cfg(not(unix))]
    pub fn set_mode(&self, _mode: u32) -> VsysResult<()> {
        Ok(())
    }
}

/// Trait for file handle operations
pub trait FsHandleOps {
    fn read(&mut self, buf: &mut [u8]) -> VsysResult<usize>;
    fn write(&mut self, buf: &[u8]) -> VsysResult<usize>;
    fn seek(&mut self, pos: SeekFrom) -> VsysResult<u64>;
    fn sync_all(&self) -> VsysResult<()>;
    fn sync_data(&self) -> VsysResult<()>;
    fn stat(&self) -> VsysResult<FileStat>;
    fn set_len(&self, size: u64) -> VsysResult<()>;
    fn set_permissions(&self, readonly: bool) -> VsysResult<()>;
    fn set_mode(&self, mode: u32) -> VsysResult<()>;
}

/// Default file handle implementation using std::fs::File
pub struct StdFsHandle {
    file: std::fs::File,
}

impl StdFsHandle {
    pub fn new(file: std::fs::File) -> Self {
        Self { file }
    }
}

impl FsHandleOps for StdFsHandle {
    fn read(&mut self, buf: &mut [u8]) -> VsysResult<usize> {
        use std::io::Read;
        self.file.read(buf).map_err(Into::into)
    }

    fn write(&mut self, buf: &[u8]) -> VsysResult<usize> {
        use std::io::Write;
        self.file.write(buf).map_err(Into::into)
    }

    fn seek(&mut self, pos: SeekFrom) -> VsysResult<u64> {
        use std::io::Seek;
        self.file.seek(pos.into()).map_err(Into::into)
    }

    fn sync_all(&self) -> VsysResult<()> {
        self.file.sync_all().map_err(Into::into)
    }

    fn sync_data(&self) -> VsysResult<()> {
        self.file.sync_data().map_err(Into::into)
    }

    fn stat(&self) -> VsysResult<FileStat> {
        let metadata = self.file.metadata()?;
        Ok(FileStat::from_metadata(&metadata))
    }

    fn set_len(&self, size: u64) -> VsysResult<()> {
        self.file.set_len(size).map_err(Into::into)
    }

    fn set_permissions(&self, readonly: bool) -> VsysResult<()> {
        let mut perms = self.file.metadata()?.permissions();
        perms.set_readonly(readonly);
        self.file.set_permissions(perms).map_err(Into::into)
    }

    #[cfg(unix)]
    fn set_mode(&self, mode: u32) -> VsysResult<()> {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode);
        self.file.set_permissions(perms).map_err(Into::into)
    }

    #[cfg(not(unix))]
    fn set_mode(&self, _mode: u32) -> VsysResult<()> {
        Ok(())
    }
}

/// Filesystem operations vtable
///
/// All functions are safe Rust function pointers. For C ABI compatibility,
/// wrap these in extern "C" functions when needed.
pub struct FsVTable {
    // Read operations
    pub read: fn(path: &Path) -> VsysResult<Vec<u8>>,
    pub read_to_string: fn(path: &Path) -> VsysResult<String>,
    pub stat: fn(path: &Path) -> VsysResult<FileStat>,
    pub lstat: fn(path: &Path) -> VsysResult<FileStat>,
    pub read_dir: fn(path: &Path) -> VsysResult<Vec<DirEntry>>,
    pub read_link: fn(path: &Path) -> VsysResult<std::path::PathBuf>,
    pub exists: fn(path: &Path) -> bool,
    pub is_file: fn(path: &Path) -> bool,
    pub is_dir: fn(path: &Path) -> bool,

    // Write operations
    pub write: fn(path: &Path, data: &[u8]) -> VsysResult<()>,
    pub append: fn(path: &Path, data: &[u8]) -> VsysResult<()>,
    pub create_dir: fn(path: &Path) -> VsysResult<()>,
    pub create_dir_all: fn(path: &Path) -> VsysResult<()>,
    pub remove_file: fn(path: &Path) -> VsysResult<()>,
    pub remove_dir: fn(path: &Path) -> VsysResult<()>,
    pub remove_dir_all: fn(path: &Path) -> VsysResult<()>,
    pub rename: fn(from: &Path, to: &Path) -> VsysResult<()>,
    pub copy: fn(from: &Path, to: &Path) -> VsysResult<u64>,
    pub symlink: fn(original: &Path, link: &Path) -> VsysResult<()>,
    pub truncate: fn(path: &Path, size: u64) -> VsysResult<()>,

    // Access check (F_OK=0, R_OK=4, W_OK=2, X_OK=1)
    pub access: fn(path: &Path, mode: u32) -> VsysResult<()>,

    // Temp directory
    pub mkdtemp: fn(prefix: &str) -> VsysResult<std::path::PathBuf>,

    // Permissions
    pub set_permissions: fn(path: &Path, readonly: bool) -> VsysResult<()>,
    pub set_mode: fn(path: &Path, mode: u32) -> VsysResult<()>,
    pub chown: fn(path: &Path, uid: u32, gid: u32) -> VsysResult<()>,

    // Canonicalize
    pub canonicalize: fn(path: &Path) -> VsysResult<std::path::PathBuf>,

    // File handle operations
    pub open: fn(path: &Path, options: &OpenOptions) -> VsysResult<FsHandle>,
}

impl Default for FsVTable {
    fn default() -> Self {
        Self {
            // Read operations
            read: default_read,
            read_to_string: default_read_to_string,
            stat: default_stat,
            lstat: default_lstat,
            read_dir: default_read_dir,
            read_link: default_read_link,
            exists: default_exists,
            is_file: default_is_file,
            is_dir: default_is_dir,

            // Write operations
            write: default_write,
            append: default_append,
            create_dir: default_create_dir,
            create_dir_all: default_create_dir_all,
            remove_file: default_remove_file,
            remove_dir: default_remove_dir,
            remove_dir_all: default_remove_dir_all,
            rename: default_rename,
            copy: default_copy,
            symlink: default_symlink,
            truncate: default_truncate,

            // Access check
            access: default_access,

            // Temp directory
            mkdtemp: default_mkdtemp,

            // Permissions
            set_permissions: default_set_permissions,
            set_mode: default_set_mode,
            chown: default_chown,

            // Canonicalize
            canonicalize: default_canonicalize,

            // File handle
            open: default_open,
        }
    }
}

impl FsVTable {
    /// Create a vtable that denies all operations
    pub fn deny_all() -> Self {
        Self {
            read: |_| Err(VsysError::PermissionDenied("fs read denied".into())),
            read_to_string: |_| Err(VsysError::PermissionDenied("fs read denied".into())),
            stat: |_| Err(VsysError::PermissionDenied("fs stat denied".into())),
            lstat: |_| Err(VsysError::PermissionDenied("fs lstat denied".into())),
            read_dir: |_| Err(VsysError::PermissionDenied("fs readdir denied".into())),
            read_link: |_| Err(VsysError::PermissionDenied("fs readlink denied".into())),
            exists: |_| false,
            is_file: |_| false,
            is_dir: |_| false,
            write: |_, _| Err(VsysError::PermissionDenied("fs write denied".into())),
            append: |_, _| Err(VsysError::PermissionDenied("fs append denied".into())),
            create_dir: |_| Err(VsysError::PermissionDenied("fs mkdir denied".into())),
            create_dir_all: |_| Err(VsysError::PermissionDenied("fs mkdir denied".into())),
            remove_file: |_| Err(VsysError::PermissionDenied("fs remove denied".into())),
            remove_dir: |_| Err(VsysError::PermissionDenied("fs rmdir denied".into())),
            remove_dir_all: |_| Err(VsysError::PermissionDenied("fs rmdir denied".into())),
            rename: |_, _| Err(VsysError::PermissionDenied("fs rename denied".into())),
            copy: |_, _| Err(VsysError::PermissionDenied("fs copy denied".into())),
            symlink: |_, _| Err(VsysError::PermissionDenied("fs symlink denied".into())),
            truncate: |_, _| Err(VsysError::PermissionDenied("fs truncate denied".into())),
            access: |_, _| Err(VsysError::PermissionDenied("fs access denied".into())),
            mkdtemp: |_| Err(VsysError::PermissionDenied("fs mkdtemp denied".into())),
            set_permissions: |_, _| Err(VsysError::PermissionDenied("fs chmod denied".into())),
            set_mode: |_, _| Err(VsysError::PermissionDenied("fs chmod denied".into())),
            chown: |_, _, _| Err(VsysError::PermissionDenied("fs chown denied".into())),
            canonicalize: |_| Err(VsysError::PermissionDenied("fs canonicalize denied".into())),
            open: |_, _| Err(VsysError::PermissionDenied("fs open denied".into())),
        }
    }

    /// Create a read-only vtable
    pub fn read_only() -> Self {
        let mut vtable = Self::default();
        vtable.write = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.append = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.create_dir = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.create_dir_all = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.remove_file = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.remove_dir = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.remove_dir_all = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.rename = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.copy = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.symlink = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.truncate = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.mkdtemp = |_| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.set_permissions = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.set_mode = |_, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable.chown = |_, _, _| Err(VsysError::PermissionDenied("fs is read-only".into()));
        vtable
    }
}

// Default implementations using std::fs

fn default_read(path: &Path) -> VsysResult<Vec<u8>> {
    std::fs::read(path).map_err(Into::into)
}

fn default_read_to_string(path: &Path) -> VsysResult<String> {
    std::fs::read_to_string(path).map_err(Into::into)
}

fn default_stat(path: &Path) -> VsysResult<FileStat> {
    let metadata = std::fs::metadata(path)?;
    Ok(FileStat::from_metadata(&metadata))
}

fn default_lstat(path: &Path) -> VsysResult<FileStat> {
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(FileStat::from_metadata(&metadata))
}

fn default_read_dir(path: &Path) -> VsysResult<Vec<DirEntry>> {
    let entries = std::fs::read_dir(path)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_type = entry.file_type().ok()?;
            let ft = if file_type.is_file() {
                FileType::File
            } else if file_type.is_dir() {
                FileType::Directory
            } else if file_type.is_symlink() {
                FileType::Symlink
            } else {
                FileType::Other
            };
            Some(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                file_type: ft,
            })
        })
        .collect();
    Ok(entries)
}

fn default_read_link(path: &Path) -> VsysResult<std::path::PathBuf> {
    std::fs::read_link(path).map_err(Into::into)
}

fn default_exists(path: &Path) -> bool {
    path.exists()
}

fn default_is_file(path: &Path) -> bool {
    path.is_file()
}

fn default_is_dir(path: &Path) -> bool {
    path.is_dir()
}

fn default_write(path: &Path, data: &[u8]) -> VsysResult<()> {
    std::fs::write(path, data).map_err(Into::into)
}

fn default_create_dir(path: &Path) -> VsysResult<()> {
    std::fs::create_dir(path).map_err(Into::into)
}

fn default_create_dir_all(path: &Path) -> VsysResult<()> {
    std::fs::create_dir_all(path).map_err(Into::into)
}

fn default_remove_file(path: &Path) -> VsysResult<()> {
    std::fs::remove_file(path).map_err(Into::into)
}

fn default_remove_dir(path: &Path) -> VsysResult<()> {
    std::fs::remove_dir(path).map_err(Into::into)
}

fn default_remove_dir_all(path: &Path) -> VsysResult<()> {
    std::fs::remove_dir_all(path).map_err(Into::into)
}

fn default_rename(from: &Path, to: &Path) -> VsysResult<()> {
    std::fs::rename(from, to).map_err(Into::into)
}

fn default_copy(from: &Path, to: &Path) -> VsysResult<u64> {
    std::fs::copy(from, to).map_err(Into::into)
}

#[cfg(unix)]
fn default_symlink(original: &Path, link: &Path) -> VsysResult<()> {
    std::os::unix::fs::symlink(original, link).map_err(Into::into)
}

#[cfg(windows)]
fn default_symlink(original: &Path, link: &Path) -> VsysResult<()> {
    // On Windows, we need to determine if it's a file or directory symlink
    if original.is_dir() {
        std::os::windows::fs::symlink_dir(original, link).map_err(Into::into)
    } else {
        std::os::windows::fs::symlink_file(original, link).map_err(Into::into)
    }
}

fn default_set_permissions(path: &Path, readonly: bool) -> VsysResult<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(readonly);
    std::fs::set_permissions(path, perms).map_err(Into::into)
}

#[cfg(unix)]
fn default_set_mode(path: &Path, mode: u32) -> VsysResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms).map_err(Into::into)
}

#[cfg(not(unix))]
fn default_set_mode(_path: &Path, _mode: u32) -> VsysResult<()> {
    // No-op on non-Unix systems
    Ok(())
}

fn default_canonicalize(path: &Path) -> VsysResult<std::path::PathBuf> {
    std::fs::canonicalize(path).map_err(Into::into)
}

fn default_append(path: &Path, data: &[u8]) -> VsysResult<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    file.write_all(data)?;
    Ok(())
}

fn default_truncate(path: &Path, size: u64) -> VsysResult<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(size)?;
    Ok(())
}

fn default_access(path: &Path, mode: u32) -> VsysResult<()> {
    // F_OK = 0: Check existence
    // R_OK = 4: Check read permission
    // W_OK = 2: Check write permission
    // X_OK = 1: Check execute permission
    const F_OK: u32 = 0;
    const R_OK: u32 = 4;
    const W_OK: u32 = 2;
    const X_OK: u32 = 1;

    let metadata = std::fs::metadata(path)?;

    // F_OK - just check existence (already done by metadata)
    if mode == F_OK {
        return Ok(());
    }

    let perms = metadata.permissions();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = perms.mode();
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };

        // Get file owner info
        use std::os::unix::fs::MetadataExt;
        let file_uid = metadata.uid();
        let file_gid = metadata.gid();

        // Determine which permission bits to check (owner, group, or other)
        let shift = if uid == file_uid {
            6 // owner bits
        } else if gid == file_gid {
            3 // group bits
        } else {
            0 // other bits
        };

        if (mode & R_OK) != 0 && (file_mode >> shift) & 4 == 0 {
            return Err(VsysError::PermissionDenied("read permission denied".into()));
        }
        if (mode & W_OK) != 0 && (file_mode >> shift) & 2 == 0 {
            return Err(VsysError::PermissionDenied(
                "write permission denied".into(),
            ));
        }
        if (mode & X_OK) != 0 && (file_mode >> shift) & 1 == 0 {
            return Err(VsysError::PermissionDenied(
                "execute permission denied".into(),
            ));
        }
    }

    #[cfg(not(unix))]
    {
        // On Windows, just check readonly for write access
        if (mode & W_OK) != 0 && perms.readonly() {
            return Err(VsysError::PermissionDenied(
                "write permission denied".into(),
            ));
        }
        // X_OK doesn't apply meaningfully on Windows
    }

    Ok(())
}

fn default_mkdtemp(prefix: &str) -> VsysResult<std::path::PathBuf> {
    use std::env;
    let temp_dir = env::temp_dir();
    let unique_name = format!("{}{}", prefix, uuid::Uuid::new_v4().simple());
    let dir_path = temp_dir.join(unique_name);
    std::fs::create_dir_all(&dir_path)?;
    Ok(dir_path)
}

#[cfg(unix)]
fn default_chown(path: &Path, uid: u32, gid: u32) -> VsysResult<()> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| VsysError::Custom("invalid path".into()))?;
    let result = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(not(unix))]
fn default_chown(_path: &Path, _uid: u32, _gid: u32) -> VsysResult<()> {
    // No-op on non-Unix systems
    Ok(())
}

fn default_open(path: &Path, options: &OpenOptions) -> VsysResult<FsHandle> {
    let mut std_options = std::fs::OpenOptions::new();
    std_options
        .read(options.read)
        .write(options.write)
        .append(options.append)
        .truncate(options.truncate)
        .create(options.create)
        .create_new(options.create_new);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if options.mode != 0 {
            std_options.mode(options.mode);
        }
    }

    let file = std_options.open(path)?;
    Ok(FsHandle::new(StdFsHandle::new(file)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_fs_read_write() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        let vtable = FsVTable::default();

        // Write
        (vtable.write)(&file_path, b"hello world").unwrap();

        // Read
        let data = (vtable.read)(&file_path).unwrap();
        assert_eq!(data, b"hello world");

        // Read to string
        let text = (vtable.read_to_string)(&file_path).unwrap();
        assert_eq!(text, "hello world");

        // Stat
        let stat = (vtable.stat)(&file_path).unwrap();
        assert!(stat.is_file());
        assert_eq!(stat.size, 11);
    }

    #[test]
    fn test_deny_all_fs() {
        let vtable = FsVTable::deny_all();

        let result = (vtable.read)(Path::new("/tmp/test"));
        assert!(result.is_err());

        assert!(!(vtable.exists)(Path::new("/tmp")));
    }

    #[test]
    fn test_read_only_fs() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        // Create file first
        std::fs::write(&file_path, b"test").unwrap();

        let vtable = FsVTable::read_only();

        // Read should work
        let data = (vtable.read)(&file_path).unwrap();
        assert_eq!(data, b"test");

        // Write should fail
        let result = (vtable.write)(&file_path, b"new data");
        assert!(result.is_err());
    }

    #[test]
    fn test_append() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("append_test.txt");

        let vtable = FsVTable::default();

        // Write initial content
        (vtable.write)(&file_path, b"hello").unwrap();

        // Append more content
        (vtable.append)(&file_path, b" world").unwrap();

        // Read and verify
        let data = (vtable.read_to_string)(&file_path).unwrap();
        assert_eq!(data, "hello world");
    }

    #[test]
    fn test_truncate() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("truncate_test.txt");

        let vtable = FsVTable::default();

        // Write content
        (vtable.write)(&file_path, b"hello world").unwrap();

        // Truncate to 5 bytes
        (vtable.truncate)(&file_path, 5).unwrap();

        // Read and verify
        let data = (vtable.read)(&file_path).unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn test_access() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("access_test.txt");

        let vtable = FsVTable::default();

        // File doesn't exist yet
        assert!((vtable.access)(&file_path, 0).is_err());

        // Create file
        (vtable.write)(&file_path, b"test").unwrap();

        // F_OK should succeed now
        assert!((vtable.access)(&file_path, 0).is_ok());
    }

    #[test]
    fn test_mkdtemp() {
        let vtable = FsVTable::default();

        let temp_dir = (vtable.mkdtemp)("xmas_test_").unwrap();

        // Directory should exist
        assert!(temp_dir.is_dir());
        assert!(temp_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("xmas_test_"));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_open_and_handle() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("handle_test.txt");

        let vtable = FsVTable::default();

        // Open for writing
        let options = OpenOptions::new().write(true).create(true);
        let mut handle = (vtable.open)(&file_path, &options).unwrap();

        // Write through handle
        handle.write(b"hello from handle").unwrap();
        handle.sync_all().unwrap();

        // Open for reading
        let options = OpenOptions::new().read(true);
        let mut handle = (vtable.open)(&file_path, &options).unwrap();

        // Read through handle
        let mut buf = vec![0u8; 100];
        let n = handle.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello from handle");

        // Stat through handle
        let stat = handle.stat().unwrap();
        assert_eq!(stat.size, 17);
    }
}
