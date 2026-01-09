//! Node.js-compatible filesystem module using vsys FsVTable
//!
//! All filesystem operations are delegated to the vsys virtual filesystem layer,
//! enabling sandboxed execution and custom filesystem implementations.

use std::path::Path;

use crate::buffer::Buffer;
use crate::permissions::get_vsys;
use crate::utils::module::{export_default, ModuleInfo};
use crate::utils::object::ObjectExt;

use either::Either;
use rsquickjs::class::{Trace, Tracer};
use rsquickjs::function::Opt;
use rsquickjs::prelude::{Async, Func};
use rsquickjs::JsLifetime;
use rsquickjs::{
    module::{Declarations, Exports, ModuleDef},
    Class, Ctx, Error, Exception, FromJs, IntoJs, Object, Result, Value,
};
use xmas_vsys::fs::{FileStat, FileType, OpenOptions};

// Re-export constants
pub const CONSTANT_F_OK: u32 = 0;
pub const CONSTANT_R_OK: u32 = 4;
pub const CONSTANT_W_OK: u32 = 2;
pub const CONSTANT_X_OK: u32 = 1;

// ============================================================================
// Helper macros and functions
// ============================================================================

/// Get vsys and check fs permission, return error if denied
fn check_permission<'js>(ctx: &Ctx<'js>, path: &Path) -> Result<std::sync::Arc<xmas_vsys::Vsys>> {
    let vsys =
        get_vsys(ctx).ok_or_else(|| Exception::throw_message(ctx, "Vsys not initialized"))?;

    if !vsys.permissions().check_fs(path) {
        return Err(Exception::throw_message(
            ctx,
            "Permission denied. Cannot access the file",
        ));
    }

    Ok(vsys)
}

// ============================================================================
// Stats class
// ============================================================================

#[derive(Clone)]
#[rsquickjs::class]
pub struct Stats {
    inner: FileStat,
}

impl<'js> Trace<'js> for Stats {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}

unsafe impl<'js> JsLifetime<'js> for Stats {
    type Changed<'to> = Stats;
}

#[rsquickjs::methods]
impl Stats {
    #[qjs(get)]
    pub fn size(&self) -> u64 {
        self.inner.size
    }

    #[qjs(get)]
    pub fn mode(&self) -> u32 {
        self.inner.mode
    }

    #[qjs(get)]
    pub fn uid(&self) -> u32 {
        self.inner.uid
    }

    #[qjs(get)]
    pub fn gid(&self) -> u32 {
        self.inner.gid
    }

    #[qjs(rename = "isFile")]
    pub fn is_file(&self) -> bool {
        self.inner.is_file()
    }

    #[qjs(rename = "isDirectory")]
    pub fn is_directory(&self) -> bool {
        self.inner.is_dir()
    }

    #[qjs(rename = "isSymbolicLink")]
    pub fn is_symbolic_link(&self) -> bool {
        self.inner.is_symlink()
    }

    #[qjs(get)]
    pub fn mtime(&self) -> Option<f64> {
        self.inner.modified.map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        })
    }

    #[qjs(get)]
    pub fn atime(&self) -> Option<f64> {
        self.inner.accessed.map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        })
    }

    #[qjs(get)]
    pub fn ctime(&self) -> Option<f64> {
        self.inner.created.map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        })
    }

    #[qjs(get)]
    pub fn birthtime(&self) -> Option<f64> {
        self.ctime()
    }
}

// ============================================================================
// Dirent class
// ============================================================================

#[derive(Clone)]
#[rsquickjs::class]
pub struct Dirent {
    name: String,
    file_type: FileType,
}

impl<'js> Trace<'js> for Dirent {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}

unsafe impl<'js> JsLifetime<'js> for Dirent {
    type Changed<'to> = Dirent;
}

#[rsquickjs::methods]
impl Dirent {
    #[qjs(get)]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[qjs(rename = "isFile")]
    pub fn is_file(&self) -> bool {
        self.file_type == FileType::File
    }

    #[qjs(rename = "isDirectory")]
    pub fn is_directory(&self) -> bool {
        self.file_type == FileType::Directory
    }

    #[qjs(rename = "isSymbolicLink")]
    pub fn is_symbolic_link(&self) -> bool {
        self.file_type == FileType::Symlink
    }
}

// ============================================================================
// FileHandle class
// ============================================================================

#[rsquickjs::class]
pub struct FileHandle {
    handle: Option<xmas_vsys::fs::FsHandle>,
    #[allow(dead_code)]
    path: String,
}

impl<'js> Trace<'js> for FileHandle {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}

unsafe impl<'js> JsLifetime<'js> for FileHandle {
    type Changed<'to> = FileHandle;
}

#[rsquickjs::methods]
impl FileHandle {
    pub async fn read<'js>(&mut self, ctx: Ctx<'js>, size: Opt<usize>) -> Result<Value<'js>> {
        let handle = self
            .handle
            .as_mut()
            .ok_or_else(|| Exception::throw_message(&ctx, "File handle is closed"))?;

        let size = size.0.unwrap_or(4096);
        let mut buf = vec![0u8; size];

        let n = handle
            .read(&mut buf)
            .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

        buf.truncate(n);
        Buffer(buf).into_js(&ctx)
    }

    pub async fn write<'js>(&mut self, ctx: Ctx<'js>, data: Value<'js>) -> Result<usize> {
        let handle = self
            .handle
            .as_mut()
            .ok_or_else(|| Exception::throw_message(&ctx, "File handle is closed"))?;

        let bytes = crate::utils::bytes::ObjectBytes::from(&ctx, &data)?;
        let buf = bytes.as_bytes(&ctx)?;

        handle
            .write(buf)
            .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
    }

    pub async fn close(&mut self) -> Result<()> {
        self.handle.take();
        Ok(())
    }

    pub fn stat<'js>(&self, ctx: Ctx<'js>) -> Result<Stats> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| Exception::throw_message(&ctx, "File handle is closed"))?;

        let stat = handle
            .stat()
            .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

        Ok(Stats { inner: stat })
    }
}

// ============================================================================
// Options structs
// ============================================================================

pub struct ReadFileOptions {
    pub encoding: Option<String>,
}

impl<'js> FromJs<'js> for ReadFileOptions {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or(Error::new_from_js(value.type_name(), "Object"))?;
        let encoding = obj.get_optional::<_, String>("encoding")?;
        Ok(Self { encoding })
    }
}

pub struct WriteFileOptions {
    pub mode: Option<u32>,
}

impl<'js> FromJs<'js> for WriteFileOptions {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or(Error::new_from_js(value.type_name(), "Object"))?;
        let mode = obj.get_optional::<_, u32>("mode")?;
        Ok(Self { mode })
    }
}

pub struct MkdirOptions {
    pub recursive: bool,
    #[allow(dead_code)]
    pub mode: Option<u32>,
}

impl Default for MkdirOptions {
    fn default() -> Self {
        Self {
            recursive: false,
            mode: None,
        }
    }
}

impl<'js> FromJs<'js> for MkdirOptions {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or(Error::new_from_js(value.type_name(), "Object"))?;
        let recursive = obj.get_optional::<_, bool>("recursive")?.unwrap_or(false);
        let mode = obj.get_optional::<_, u32>("mode")?;
        Ok(Self { recursive, mode })
    }
}

pub struct ReaddirOptions {
    pub with_file_types: bool,
}

impl Default for ReaddirOptions {
    fn default() -> Self {
        Self {
            with_file_types: false,
        }
    }
}

impl<'js> FromJs<'js> for ReaddirOptions {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or(Error::new_from_js(value.type_name(), "Object"))?;
        let with_file_types = obj
            .get_optional::<_, bool>("withFileTypes")?
            .unwrap_or(false);
        Ok(Self { with_file_types })
    }
}

pub struct RmOptions {
    pub recursive: bool,
    pub force: bool,
}

impl Default for RmOptions {
    fn default() -> Self {
        Self {
            recursive: false,
            force: false,
        }
    }
}

impl<'js> FromJs<'js> for RmOptions {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or(Error::new_from_js(value.type_name(), "Object"))?;
        let recursive = obj.get_optional::<_, bool>("recursive")?.unwrap_or(false);
        let force = obj.get_optional::<_, bool>("force")?.unwrap_or(false);
        Ok(Self { recursive, force })
    }
}

// ============================================================================
// Async fs functions (for promises)
// ============================================================================

pub async fn access(ctx: Ctx<'_>, path: String, mode: Opt<u32>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let mode = mode.0.unwrap_or(CONSTANT_F_OK);

    (vsys.fs().access)(path_obj, mode).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub async fn read_file(
    ctx: Ctx<'_>,
    path: String,
    options: Opt<Either<String, ReadFileOptions>>,
) -> Result<Value<'_>> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let bytes =
        (vsys.fs().read)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    let buffer = Buffer(bytes);

    if let Some(opts) = options.0 {
        let encoding = match opts {
            Either::Left(enc) => Some(enc),
            Either::Right(opts) => opts.encoding,
        };
        if let Some(enc) = encoding {
            return buffer.to_string(&ctx, &enc).and_then(|s| s.into_js(&ctx));
        }
    }

    buffer.into_js(&ctx)
}

pub async fn write_file<'js>(
    ctx: Ctx<'js>,
    path: String,
    data: Value<'js>,
    options: Opt<Either<String, WriteFileOptions>>,
) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let bytes = crate::utils::bytes::ObjectBytes::from(&ctx, &data)?;
    let buf = bytes.as_bytes(&ctx)?;

    (vsys.fs().write)(path_obj, buf).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    #[cfg(unix)]
    if let Some(Either::Right(opts)) = options.0 {
        if let Some(mode) = opts.mode {
            (vsys.fs().set_mode)(path_obj, mode)
                .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = options;

    Ok(())
}

pub async fn rename(ctx: Ctx<'_>, old_path: String, new_path: String) -> Result<()> {
    let old = Path::new(&old_path);
    let new = Path::new(&new_path);
    let vsys = check_permission(&ctx, old)?;

    (vsys.fs().rename)(old, new).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub async fn read_dir<'js>(
    ctx: Ctx<'js>,
    path: String,
    options: Opt<ReaddirOptions>,
) -> Result<Value<'js>> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let entries = (vsys.fs().read_dir)(path_obj)
        .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    let with_file_types = options.0.map(|o| o.with_file_types).unwrap_or(false);

    if with_file_types {
        let arr = rsquickjs::Array::new(ctx.clone())?;
        for (i, entry) in entries.into_iter().enumerate() {
            let dirent = Dirent {
                name: entry.name,
                file_type: entry.file_type,
            };
            arr.set(i, Class::instance(ctx.clone(), dirent)?)?;
        }
        arr.into_js(&ctx)
    } else {
        let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
        names.into_js(&ctx)
    }
}

pub async fn mkdir(ctx: Ctx<'_>, path: String, options: Opt<MkdirOptions>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let opts = options.0.unwrap_or_default();

    let result = if opts.recursive {
        (vsys.fs().create_dir_all)(path_obj)
    } else {
        (vsys.fs().create_dir)(path_obj)
    };

    result.map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    #[cfg(unix)]
    if let Some(mode) = opts.mode {
        (vsys.fs().set_mode)(path_obj, mode)
            .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
    }

    Ok(())
}

pub async fn mkdtemp(ctx: Ctx<'_>, prefix: String) -> Result<String> {
    let vsys =
        get_vsys(&ctx).ok_or_else(|| Exception::throw_message(&ctx, "Vsys not initialized"))?;

    let path =
        (vsys.fs().mkdtemp)(&prefix).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(path.to_string_lossy().into_owned())
}

pub async fn rmfile(ctx: Ctx<'_>, path: String, options: Opt<RmOptions>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let opts = options.0.unwrap_or_default();

    let result = if opts.recursive {
        (vsys.fs().remove_dir_all)(path_obj)
    } else {
        (vsys.fs().remove_file)(path_obj)
    };

    match result {
        Ok(()) => Ok(()),
        Err(_) if opts.force => Ok(()), // Ignore errors in force mode
        Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
    }
}

pub async fn rmdir(ctx: Ctx<'_>, path: String) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().remove_dir)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub async fn stat_fn(ctx: Ctx<'_>, path: String) -> Result<Stats> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let stat =
        (vsys.fs().stat)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(Stats { inner: stat })
}

pub async fn lstat_fn(ctx: Ctx<'_>, path: String) -> Result<Stats> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let stat =
        (vsys.fs().lstat)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(Stats { inner: stat })
}

pub async fn chmod(ctx: Ctx<'_>, path: String, mode: u32) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().set_mode)(path_obj, mode).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub async fn symlink(ctx: Ctx<'_>, target: String, path: String) -> Result<()> {
    let target_obj = Path::new(&target);
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().symlink)(target_obj, path_obj)
        .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub async fn open(
    ctx: Ctx<'_>,
    path: String,
    flags: Opt<String>,
    mode: Opt<u32>,
) -> Result<FileHandle> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let flags = flags.0.unwrap_or_else(|| "r".to_string());
    let mut options = OpenOptions::new();

    match flags.as_str() {
        "r" => {
            options = options.read(true);
        }
        "r+" => {
            options = options.read(true).write(true);
        }
        "w" => {
            options = options.write(true).create(true).truncate(true);
        }
        "w+" => {
            options = options.read(true).write(true).create(true).truncate(true);
        }
        "a" => {
            options = options.append(true).create(true);
        }
        "a+" => {
            options = options.read(true).append(true).create(true);
        }
        "wx" | "xw" => {
            options = options.write(true).create_new(true);
        }
        _ => {
            options = options.read(true);
        }
    }

    if let Some(m) = mode.0 {
        options = options.mode(m);
    }

    let handle = (vsys.fs().open)(path_obj, &options)
        .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(FileHandle {
        handle: Some(handle),
        path,
    })
}

// ============================================================================
// Sync fs functions
// ============================================================================

pub fn access_sync(ctx: Ctx<'_>, path: String, mode: Opt<u32>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let mode = mode.0.unwrap_or(CONSTANT_F_OK);

    (vsys.fs().access)(path_obj, mode).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub fn read_file_sync(
    ctx: Ctx<'_>,
    path: String,
    options: Opt<Either<String, ReadFileOptions>>,
) -> Result<Value<'_>> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let bytes =
        (vsys.fs().read)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    let buffer = Buffer(bytes);

    if let Some(opts) = options.0 {
        let encoding = match opts {
            Either::Left(enc) => Some(enc),
            Either::Right(opts) => opts.encoding,
        };
        if let Some(enc) = encoding {
            return buffer.to_string(&ctx, &enc).and_then(|s| s.into_js(&ctx));
        }
    }

    buffer.into_js(&ctx)
}

pub fn write_file_sync<'js>(
    ctx: Ctx<'js>,
    path: String,
    data: Value<'js>,
    options: Opt<Either<String, WriteFileOptions>>,
) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let bytes = crate::utils::bytes::ObjectBytes::from(&ctx, &data)?;
    let buf = bytes.as_bytes(&ctx)?;

    (vsys.fs().write)(path_obj, buf).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    #[cfg(unix)]
    if let Some(Either::Right(opts)) = options.0 {
        if let Some(mode) = opts.mode {
            (vsys.fs().set_mode)(path_obj, mode)
                .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = options;

    Ok(())
}

pub fn rename_sync(ctx: Ctx<'_>, old_path: String, new_path: String) -> Result<()> {
    let old = Path::new(&old_path);
    let new = Path::new(&new_path);
    let vsys = check_permission(&ctx, old)?;

    (vsys.fs().rename)(old, new).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub fn read_dir_sync<'js>(
    ctx: Ctx<'js>,
    path: String,
    options: Opt<ReaddirOptions>,
) -> Result<Value<'js>> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let entries = (vsys.fs().read_dir)(path_obj)
        .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    let with_file_types = options.0.map(|o| o.with_file_types).unwrap_or(false);

    if with_file_types {
        let arr = rsquickjs::Array::new(ctx.clone())?;
        for (i, entry) in entries.into_iter().enumerate() {
            let dirent = Dirent {
                name: entry.name,
                file_type: entry.file_type,
            };
            arr.set(i, Class::instance(ctx.clone(), dirent)?)?;
        }
        arr.into_js(&ctx)
    } else {
        let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
        names.into_js(&ctx)
    }
}

pub fn mkdir_sync(ctx: Ctx<'_>, path: String, options: Opt<MkdirOptions>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let opts = options.0.unwrap_or_default();

    let result = if opts.recursive {
        (vsys.fs().create_dir_all)(path_obj)
    } else {
        (vsys.fs().create_dir)(path_obj)
    };

    result.map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    #[cfg(unix)]
    if let Some(mode) = opts.mode {
        (vsys.fs().set_mode)(path_obj, mode)
            .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
    }

    Ok(())
}

pub fn mkdtemp_sync(ctx: Ctx<'_>, prefix: String) -> Result<String> {
    let vsys =
        get_vsys(&ctx).ok_or_else(|| Exception::throw_message(&ctx, "Vsys not initialized"))?;

    let path =
        (vsys.fs().mkdtemp)(&prefix).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(path.to_string_lossy().into_owned())
}

pub fn rmfile_sync(ctx: Ctx<'_>, path: String, options: Opt<RmOptions>) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;
    let opts = options.0.unwrap_or_default();

    let result = if opts.recursive {
        (vsys.fs().remove_dir_all)(path_obj)
    } else {
        (vsys.fs().remove_file)(path_obj)
    };

    match result {
        Ok(()) => Ok(()),
        Err(_) if opts.force => Ok(()),
        Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
    }
}

pub fn rmdir_sync(ctx: Ctx<'_>, path: String) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().remove_dir)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub fn stat_fn_sync(ctx: Ctx<'_>, path: String) -> Result<Stats> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let stat =
        (vsys.fs().stat)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(Stats { inner: stat })
}

pub fn lstat_fn_sync(ctx: Ctx<'_>, path: String) -> Result<Stats> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    let stat =
        (vsys.fs().lstat)(path_obj).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;

    Ok(Stats { inner: stat })
}

pub fn chmod_sync(ctx: Ctx<'_>, path: String, mode: u32) -> Result<()> {
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().set_mode)(path_obj, mode).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

pub fn symlink_sync(ctx: Ctx<'_>, target: String, path: String) -> Result<()> {
    let target_obj = Path::new(&target);
    let path_obj = Path::new(&path);
    let vsys = check_permission(&ctx, path_obj)?;

    (vsys.fs().symlink)(target_obj, path_obj)
        .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

// ============================================================================
// Module definitions
// ============================================================================

pub struct FsPromisesModule;

impl ModuleDef for FsPromisesModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare("access")?;
        declare.declare("open")?;
        declare.declare("readFile")?;
        declare.declare("writeFile")?;
        declare.declare("rename")?;
        declare.declare("readdir")?;
        declare.declare("mkdir")?;
        declare.declare("mkdtemp")?;
        declare.declare("rm")?;
        declare.declare("rmdir")?;
        declare.declare("stat")?;
        declare.declare("lstat")?;
        declare.declare("constants")?;
        declare.declare("chmod")?;
        declare.declare("symlink")?;
        declare.declare("default")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        let globals = ctx.globals();
        Class::<Dirent>::define(&globals)?;
        Class::<FileHandle>::define(&globals)?;
        Class::<Stats>::define(&globals)?;

        export_default(ctx, exports, |default| {
            export_promises(ctx, default)?;
            Ok(())
        })
    }
}

impl From<FsPromisesModule> for ModuleInfo<FsPromisesModule> {
    fn from(val: FsPromisesModule) -> Self {
        ModuleInfo {
            name: "fs/promises",
            module: val,
        }
    }
}

pub struct FsModule;

impl ModuleDef for FsModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare("promises")?;
        declare.declare("accessSync")?;
        declare.declare("mkdirSync")?;
        declare.declare("mkdtempSync")?;
        declare.declare("readdirSync")?;
        declare.declare("readFileSync")?;
        declare.declare("rmdirSync")?;
        declare.declare("rmSync")?;
        declare.declare("statSync")?;
        declare.declare("lstatSync")?;
        declare.declare("writeFileSync")?;
        declare.declare("constants")?;
        declare.declare("chmodSync")?;
        declare.declare("renameSync")?;
        declare.declare("symlinkSync")?;
        declare.declare("default")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        let globals = ctx.globals();
        Class::<Dirent>::define(&globals)?;
        Class::<FileHandle>::define(&globals)?;
        Class::<Stats>::define(&globals)?;

        export_default(ctx, exports, |default| {
            let promises = Object::new(ctx.clone())?;
            export_promises(ctx, &promises)?;
            export_constants(ctx, default)?;

            default.set("promises", promises)?;
            default.set("accessSync", Func::from(access_sync))?;
            default.set("mkdirSync", Func::from(mkdir_sync))?;
            default.set("mkdtempSync", Func::from(mkdtemp_sync))?;
            default.set("readdirSync", Func::from(read_dir_sync))?;
            default.set("readFileSync", Func::from(read_file_sync))?;
            default.set("rmdirSync", Func::from(rmdir_sync))?;
            default.set("rmSync", Func::from(rmfile_sync))?;
            default.set("statSync", Func::from(stat_fn_sync))?;
            default.set("lstatSync", Func::from(lstat_fn_sync))?;
            default.set("writeFileSync", Func::from(write_file_sync))?;
            default.set("chmodSync", Func::from(chmod_sync))?;
            default.set("renameSync", Func::from(rename_sync))?;
            default.set("symlinkSync", Func::from(symlink_sync))?;
            Ok(())
        })
    }
}

fn export_promises<'js>(ctx: &Ctx<'js>, exports: &Object<'js>) -> Result<()> {
    export_constants(ctx, exports)?;
    exports.set("access", Func::from(Async(access)))?;
    exports.set("open", Func::from(Async(open)))?;
    exports.set("readFile", Func::from(Async(read_file)))?;
    exports.set("writeFile", Func::from(Async(write_file)))?;
    exports.set("rename", Func::from(Async(rename)))?;
    exports.set("readdir", Func::from(Async(read_dir)))?;
    exports.set("mkdir", Func::from(Async(mkdir)))?;
    exports.set("mkdtemp", Func::from(Async(mkdtemp)))?;
    exports.set("rm", Func::from(Async(rmfile)))?;
    exports.set("rmdir", Func::from(Async(rmdir)))?;
    exports.set("stat", Func::from(Async(stat_fn)))?;
    exports.set("lstat", Func::from(Async(lstat_fn)))?;
    exports.set("chmod", Func::from(Async(chmod)))?;
    exports.set("symlink", Func::from(Async(symlink)))?;
    Ok(())
}

fn export_constants<'js>(ctx: &Ctx<'js>, exports: &Object<'js>) -> Result<()> {
    let constants = Object::new(ctx.clone())?;
    constants.set("F_OK", CONSTANT_F_OK)?;
    constants.set("R_OK", CONSTANT_R_OK)?;
    constants.set("W_OK", CONSTANT_W_OK)?;
    constants.set("X_OK", CONSTANT_X_OK)?;
    exports.set("constants", constants)?;
    Ok(())
}

impl From<FsModule> for ModuleInfo<FsModule> {
    fn from(val: FsModule) -> Self {
        ModuleInfo {
            name: "fs",
            module: val,
        }
    }
}
