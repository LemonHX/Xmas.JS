use color_eyre::eyre::{Context, Result};
use color_eyre::Report;
use compact_str::{CompactString, ToCompactString};
use node_semver::{Range, Version};
use serde::de::DeserializeOwned;
use serde::{de::Error, Deserialize, Serialize};
use serde_json::Value;
use std::fs::{read_to_string, File};
use std::future::Future;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::{
    env::consts::{ARCH, OS},
    fmt::Display,
};
use tracing::instrument;
use url::Url;

use crate::package::PackageMetadata;
use crate::progress::log_warning;
use crate::resolve::{Graph, Lockfile};

pub fn decode_json<T: DeserializeOwned>(
    x: &[u8],
) -> Result<T, serde_path_to_error::Error<serde_json::Error>> {
    let jd = &mut serde_json::Deserializer::from_slice(x);

    serde_path_to_error::deserialize(jd)
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct VersionSpecifierPrefixed {
    pub prefix: CompactString,
    pub rest: CompactString,
}

impl Display for VersionSpecifierPrefixed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.prefix, self.rest)
    }
}

impl Serialize for VersionSpecifierPrefixed {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VersionSpecifierPrefixed {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let (prefix, rest) = s
            .split_once(':')
            .ok_or_else(|| D::Error::custom("missing :"))?;
        if prefix.starts_with("http") {
            return Err(D::Error::custom("http not allowed"));
        }
        Ok(Self {
            prefix: prefix.to_compact_string(),
            rest: rest.to_compact_string(),
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[serde(untagged)]
pub enum VersionSpecifier {
    Range(Range),
    Prefixed(VersionSpecifierPrefixed),
    DirectUrl(Url),
    Other(CompactString),
}

impl VersionSpecifier {
    pub fn satisfies(&self, v: &Version) -> bool {
        match self {
            VersionSpecifier::Range(r) => r.satisfies(v),
            VersionSpecifier::Prefixed(_) => true,
            VersionSpecifier::DirectUrl(_) => true,
            VersionSpecifier::Other(_) => false,
        }
    }
}

impl Display for VersionSpecifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionSpecifier::Range(a) => a.fmt(f),
            VersionSpecifier::Prefixed(a) => a.fmt(f),
            VersionSpecifier::DirectUrl(a) => a.fmt(f),
            VersionSpecifier::Other(a) => a.fmt(f),
        }
    }
}

pub fn get_node_os() -> &'static str {
    match OS {
        "linux" => "linux",
        "macos" => "darwin",
        "freebsd" => "freebsd",
        "openbsd" => "openbsd",
        "windows" => "win32",
        _ => unreachable!(),
    }
}

pub fn get_node_cpu() -> &'static str {
    match ARCH {
        "x86_64" => "x64",
        x => x,
    }
}

const RETRY_LIMIT: usize = 3;

pub async fn retry_async<T, F, Fut>(mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last = None;
    for _ in 0..RETRY_LIMIT {
        match f().await {
            Ok(x) => return Ok(x),
            Err(e) => {
                log_warning(&format!("Retrying {e}"));
                last = Some(e);
            }
        }
    }
    Err(last.unwrap()).wrap_err("Failed all retries")
}

pub fn read_package() -> Result<PackageMetadata> {
    read_json("package.json")
}

pub fn read_package_or_default<T: DeserializeOwned>() -> Result<T> {
    let s = match read_to_string("package.json") {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => "{}".into(),
        r => r?,
    };
    Ok(serde_json::from_str(&s)?)
}

pub fn save_package(package: &Value) -> Result<()> {
    write_json("package.json", package)
}

#[instrument]
pub fn read_json<T: DeserializeOwned>(path: impl AsRef<Path> + std::fmt::Debug) -> Result<T> {
    Ok(serde_json::from_str(&read_to_string(path)?)?)
}

pub fn write_json<T: Serialize>(path: impl AsRef<Path>, data: T) -> Result<()> {
    let mut file = File::create(path)?;

    file.write_all(serde_json::to_string_pretty(&data)?.as_bytes())?;

    file.flush()?;

    Ok(())
}

pub fn load_graph_from_lockfile() -> Graph {
    let lockfile: Lockfile = read_json("xmas.lock").unwrap_or_default();
    lockfile.into_graph()
}

pub type ArcResult<T, E = Report> = Result<T, Arc<E>>;
