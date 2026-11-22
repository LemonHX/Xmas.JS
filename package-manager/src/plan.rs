use async_lock::Semaphore;
use color_eyre::{
    eyre::{eyre, Result},
    Report,
};
use compact_str::{CompactString, ToCompactString};
use flate2::read::GzDecoder;
use owo_colors::OwoColorize;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, exists, metadata, read_dir, remove_dir_all, File};
use std::{
    io,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tar::Archive;

use crate::{
    cache::Cache,
    config::read_config,
    net,
    npm::{Dependency, DependencyTree},
    package::PackageMetadata,
    pool::Pool,
    progress::{log_progress, log_verbose, log_warning},
    scoped_path::scoped_join,
    util::{retry_async, VersionSpecifier},
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Plan {
    #[serde(flatten)]
    pub trees: FxHashMap<CompactString, DependencyTree>,
}

impl Plan {
    pub fn new(trees: FxHashMap<CompactString, DependencyTree>) -> Self {
        Self { trees }
    }

    pub fn satisfies(&self, package: &PackageMetadata) -> bool {
        let map: FxHashMap<_, _> = self
            .trees
            .values()
            .map(|x| (x.root.name.to_compact_string(), x.root.version.clone()))
            .collect();
        package.iter_all().all(|req| {
            if let Some(version) = map.get(&req.name) {
                if let VersionSpecifier::Range(range) = req.version {
                    return range.satisfies(version);
                }
            }
            false
        })
    }
}

pub fn tree_size(trees: &FxHashMap<CompactString, DependencyTree>) -> usize {
    trees.len()
        + trees
            .values()
            .map(|x| tree_size(&x.children))
            .sum::<usize>()
}

/// Local work runs on blocking threads: unpacking and linking are CPU and disk, not sockets.
///
/// These permits are separate from the network ones (`net::IN_FLIGHT`) on purpose: a link that
/// takes a core must never hold up a download, and a download must never wait behind a link.
static LOCAL_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    Semaphore::new(cores.clamp(4, 32))
});

/// Fetch one package's tarball into the store, once.
#[tracing::instrument]
async fn download_package(dep: Dependency) -> Result<()> {
    let target_path = scoped_join(".xmas/store", dep.id())?;

    create_dir_all(&target_path)?;

    if metadata(target_path.join("_complete")).is_ok() {
        log_verbose(&format!("Skipped downloading {}", dep.id()));
        return Ok(());
    }

    log_verbose(&format!("Downloading {}@{}", dep.name, dep.version));

    let registry_auth = read_config()?
        .registry
        .into_iter()
        .find(|x| dep.dist.tarball.starts_with(&x.url))
        .and_then(|x| x.auth);

    // A tarball is small enough to hold whole: the archive is then unpacked straight from
    // memory, which is one read and no temporary file.
    let url = dep.dist.tarball.clone();
    let auth = registry_auth.map(Arc::new);
    let tarball = retry_async(|| {
        let url = url.clone();
        let auth = auth.clone();
        async move { net::get(&url, auth.as_deref()).await }
    })
    .await?;

    // Decompressing and writing out a package is a core and a disk, so it goes to a blocking
    // thread instead of standing on the one that drives the sockets.
    let unpacked = target_path.clone();
    let _permit = LOCAL_PERMITS.acquire().await;
    smol::unblock(move || -> Result<()> {
        let mut archive = Archive::new(GzDecoder::new(&tarball[..]));
        archive
            .unpack(&unpacked)
            .map_err(|e| eyre!("unpacking into {}: {e:?}", unpacked.display()))?;
        normalize_modes(&unpacked)?;
        File::create(unpacked.join("_complete"))?;
        Ok(())
    })
    .await?;

    log_progress(&format!("Downloaded {}", dep.id().bright_blue()));

    Ok(())
}

/// A tarball carries whatever modes its publisher last had, and a directory without a search bit
/// cannot be walked at all: a published package with a `0666` directory in it makes its own files
/// unreachable. The store is normalized to the two modes an install can use — `0755` for a
/// directory, `0644` or `0755` for a file, depending on whether it was executable.
#[cfg(unix)]
fn normalize_modes(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_symlink() {
        return Ok(());
    }

    if meta.is_dir() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        for entry in std::fs::read_dir(path)? {
            normalize_modes(&entry?.path())?;
        }
    } else {
        let executable = meta.permissions().mode() & 0o111 != 0;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn normalize_modes(_path: &Path) -> Result<()> {
    Ok(())
}

/// The same download, shared: a package two branches of the tree need is fetched once.
pub async fn download_package_shared(dep: Dependency) -> Result<()> {
    static CACHE: LazyLock<Cache<Dependency, Result<(), Arc<Report>>>> = LazyLock::new(|| {
        Cache::new(|key: Dependency| {
            Box::pin(async move { download_package(key).await.map_err(Arc::new) })
        })
    });

    CACHE.get(dep).await.map_err(Report::msg)
}

/// Put a package's files in `dst`.
///
/// On APFS the kernel clones a whole directory tree copy-on-write in one call: measured on this
/// tree, 1257 packages and 38k files in one second, against eleven for a hard link per file. Every
/// other filesystem gets the hard links, one syscall at a time. A clone shares blocks with the
/// store rather than the inode, so a tool that rewrites a file in `node_modules` cannot reach back
/// and change what the next install places.
#[cfg(target_os = "macos")]
fn place_dir(src: &Path, dst: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    if let (Ok(from), Ok(to)) = (
        CString::new(src.as_os_str().as_bytes()),
        CString::new(dst.as_os_str().as_bytes()),
    ) {
        // SAFETY: both pointers are valid NUL-terminated strings that outlive the call and
        // `clonefile` reads nothing else from this process.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return Ok(());
        }

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| eyre!("creating {}: {e}", parent.display()))?;
        }
    }

    log_verbose(&format!(
        "cloning {} did not work, linking file by file",
        src.display()
    ));

    hardlink_dir(src.to_path_buf(), dst.to_path_buf())
}

#[cfg(not(target_os = "macos"))]
fn place_dir(src: &Path, dst: &Path) -> Result<()> {
    hardlink_dir(src.to_path_buf(), dst.to_path_buf())
}

fn hardlink_dir(src: PathBuf, dst: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&dst).map_err(|e| eyre!("creating {}: {e}", dst.display()))?;
    let dir = std::fs::read_dir(&src).map_err(|e| eyre!("reading {}: {e}", src.display()))?;
    for entry in dir {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            hardlink_dir(entry.path(), target)?;
        } else {
            std::fs::hard_link(entry.path(), &target)
                .map_err(|e| eyre!("linking {} to {}: {e}", entry.path().display(), target.display()))?;
        }
    }
    Ok(())
}

fn get_package_src(src: &Path) -> Result<PathBuf> {
    let mut dir = read_dir(src)?;
    while let Some(entry) = dir.next().transpose()? {
        let ty = entry.file_type()?;
        if ty.is_dir() {
            return Ok(entry.path());
        }
    }
    Err(Report::msg("No package src found"))
}

#[tracing::instrument]
pub async fn install_package(prefix: &[CompactString], dep: &Dependency) -> Result<()> {
    download_package_shared(dep.clone()).await?;

    let mut target_path = PathBuf::new();

    for segment in prefix {
        target_path.push(segment.as_str());
        target_path.push("node_modules");
    }

    target_path.push(&*dep.name);

    log_verbose(&format!("Installing {}", target_path.to_string_lossy()));

    target_path = scoped_join("node_modules", target_path)?;

    let marker = format!(".installed!{}", dep.id());
    let install_marker = target_path.join(&marker);
    if exists(&install_marker)? {
        log_verbose(&format!(
            "Skipping installation for {}",
            dep.id().bright_blue()
        ));
        return Ok(());
    }

    let _ = remove_dir_all(&target_path);

    let src_path = scoped_join(".xmas/store", dep.id())?;
    let src = get_package_src(&src_path)?;

    // Placing a package is a syscall per file at worst: thousands of them, on a blocking thread.
    let dst = target_path.clone();
    let _permit = LOCAL_PERMITS.acquire().await;
    smol::unblock(move || -> Result<()> {
        place_dir(&src, &dst)?;
        File::create(dst.join(&marker))
            .map_err(|e| eyre!("marking {}: {e}", dst.display()))?;
        Ok(())
    })
    .await?;

    log_progress(&format!("Installed {}", dep.id().bright_blue()));

    Ok(())
}

/// Download and install every package in the plan.
///
/// One task per package: it installs its own package, then queues a task for each child. The pool
/// counts what is in flight, and the permits decide how much of that touches the network or a
/// core at once, so a wide tree is wide instead of queueing behind sixteen threads.
pub async fn execute_plan(plan: Plan) -> Result<()> {
    fn queue_install(pool: &Pool, tree: DependencyTree, prefix: Vec<CompactString>) {
        pool.spawn(move |pool| async move {
            install_package(&prefix, &tree.root).await?;

            for (_, dep) in tree.children {
                let mut prefix = prefix.clone();
                prefix.push(tree.root.name.clone());
                queue_install(&pool, dep, prefix);
            }

            Result::Ok(())
        });
    }

    /// Start every download in the tree now, so the store fills while the installs run.
    fn warmup_dep_tree(pool: &Pool, tree: &DependencyTree) {
        let root = tree.root.clone();
        pool.spawn(move |_| async move { download_package_shared(root).await });
        for child in tree.children.values() {
            warmup_dep_tree(pool, child);
        }
    }

    let pool = Pool::new();

    for tree in plan.trees.values() {
        warmup_dep_tree(&pool, tree);
    }

    for (_, tree) in plan.trees.into_iter() {
        queue_install(&pool, tree, vec![]);
    }

    pool.run().await
}

pub(crate) fn symlink(target: &str, path: &str, type_value: Option<String>) -> io::Result<()> {
    #[cfg(unix)]
    {
        _ = type_value;
        std::os::unix::fs::symlink(target, path)
    }
    #[cfg(windows)]
    {
        let type_str = match type_value.as_deref() {
            Some(t @ ("file" | "dir" | "junction")) => t,
            _ => {
                if std::fs::metadata(target)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    "dir"
                } else {
                    "file"
                }
            }
        };
        match type_str {
            "junction" | "dir" => junction::create(target, path),
            _ => std::os::windows::fs::symlink_file(target, path),
        }
    }
}

pub fn setup_bins(plan: &Plan) -> Result<()> {
    create_dir_all("node_modules/.bin")?;

    for tree in plan.trees.values() {
        let dep = &tree.root;
        for (cmd, path) in &dep.bins {
            let path = path.to_compact_string();
            let mut script_path = PathBuf::from("../").join(&*dep.name).join(&*path);
            if !exists(PathBuf::from("node_modules/.bin").join(&script_path))? {
                script_path.set_extension("js");
            }
            if !cmd.contains('/') {
                #[cfg(windows)]
                {
                    // 使用 cmd shim 代替符号链接，避免权限问题
                    let cmd_path = PathBuf::from("node_modules/.bin").join(format!("{}.cmd", cmd));
                    let shim_content = format!(
                        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\n\"%dp0%\\{}\" %*\r\n",
                        script_path.to_str().unwrap().replace('/', "\\")
                    );
                    if let Err(e) = std::fs::write(&cmd_path, &shim_content) {
                        if e.kind() != ErrorKind::AlreadyExists {
                            return Err(e.into());
                        }
                    }
                    // 同时创建 PowerShell shim
                    let ps1_path = PathBuf::from("node_modules/.bin").join(format!("{}.ps1", cmd));
                    let ps1_content = format!(
                        "#!/usr/bin/env pwsh\r\n$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\r\n\r\n$exe=\"\"\r\nif ($PSVersionTable.PSVersion -lt \"6.0\" -or $IsWindows) {{\r\n  $exe=\".exe\"\r\n}}\r\n& \"$basedir/{}\" $args\r\nexit $LASTEXITCODE\r\n",
                        script_path.to_str().unwrap()
                    );
                    let _ = std::fs::write(&ps1_path, &ps1_content);
                }
                #[cfg(unix)]
                {
                    let bin_path = PathBuf::from("node_modules/.bin").join(&**cmd);
                    if let Err(e) = symlink(
                        script_path.to_str().unwrap(),
                        bin_path.to_str().unwrap(),
                        None,
                    ) {
                        if e.kind() != ErrorKind::AlreadyExists {
                            return Err(e.into());
                        }
                    }
                }

                if read_config()?.disallow_install_scripts {
                    log_warning(
                        &format!("Package {} may require install scripts; consider disabling `disallow_install_scripts`", dep.id())
                    );
                }
            }
        }
    }

    Ok(())
}
