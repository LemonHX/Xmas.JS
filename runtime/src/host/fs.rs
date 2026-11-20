//! File system. All asynchronous, implemented on smol's blocking thread pool.
//!
//! When moving fs in modules, this layer is the only thing it should touch.

use std::io;
use std::path::Path;

use futures_lite::StreamExt;

pub use smol::fs::File;
pub use smol::fs::OpenOptions;

/// The small subset returned by stat is enough for node's fs.stat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub is_file: bool,
    pub is_dir: bool,
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Fs;

impl Fs {
    pub async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        smol::fs::read(path).await
    }

    pub async fn read_to_string(&self, path: impl AsRef<Path>) -> io::Result<String> {
        smol::fs::read_to_string(path).await
    }

    pub async fn write(&self, path: impl AsRef<Path>, data: &[u8]) -> io::Result<()> {
        smol::fs::write(path, data).await
    }

    pub async fn write_string(&self, path: impl AsRef<Path>, text: &str) -> io::Result<()> {
        smol::fs::write(path, text.as_bytes()).await
    }

    pub async fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        smol::fs::create_dir_all(path).await
    }

    pub async fn remove_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        smol::fs::remove_file(path).await
    }

    pub async fn remove_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        smol::fs::remove_dir_all(path).await
    }

    pub async fn rename(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        smol::fs::rename(from, to).await
    }

    pub async fn canonicalize(&self, path: impl AsRef<Path>) -> io::Result<std::path::PathBuf> {
        smol::fs::canonicalize(path).await
    }

    pub async fn metadata(&self, path: impl AsRef<Path>) -> io::Result<Meta> {
        let meta = smol::fs::metadata(path).await?;
        Ok(Meta {
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            len: meta.len(),
        })
    }

    pub async fn read_dir(&self, path: impl AsRef<Path>) -> io::Result<Vec<DirItem>> {
        let mut dir = smol::fs::read_dir(path).await?;
        let mut items = Vec::new();
        while let Some(entry) = dir.next().await {
            let entry = entry?;
            let meta = entry.metadata().await?;
            items.push(DirItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                is_file: meta.is_file(),
            });
        }
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(items)
    }

    /// Open a file for reading, for places that need to stream-read large files.
    pub async fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        File::open(path).await
    }

    /// Open a file for writing (truncate), same as above.
    pub async fn create(&self, path: impl AsRef<Path>) -> io::Result<File> {
        File::create(path).await
    }
}
