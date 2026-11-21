use color_eyre::{
    eyre::{eyre, ContextCompat, Result},
    Report,
};
use compact_str::{CompactString, ToCompactString};
use indexmap::IndexMap;
use itertools::Itertools;
use node_semver::Version;
use owo_colors::OwoColorize;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::MAIN_SEPARATOR,
    sync::{Arc, LazyLock},
};
use std::{fmt::Debug, io::Read};

use flate2::read::GzDecoder;
use tar::Archive;

use crate::{
    cache::Cache,
    config::{read_config, Registry},
    net,
    package::{Dist, PackageInfo, PackageMetadata, PackageSpecifier},
    progress::{log_progress, log_verbose},
    util::{decode_json, retry, ArcResult, VersionSpecifier},
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct RegistryResponse {
    #[serde(rename = "dist-tags")]
    pub dist_tags: FxHashMap<CompactString, CompactString>,
    pub versions: IndexMap<Version, PackageMetadata>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct PlatformMap(BTreeSet<CompactString>);

impl PlatformMap {
    pub fn allowed(&self) -> impl Iterator<Item = &str> {
        self.0
            .iter()
            .filter(|x| !x.starts_with('!'))
            .map(|x| x.as_str())
    }

    pub fn blocked(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|x| x.strip_prefix('!'))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_supported(&self, platform: &str) -> bool {
        if self.is_empty() {
            true
        } else {
            self.allowed().any(|o| o == platform) && !self.blocked().any(|o| o == platform)
        }
    }
}

fn select_registry(name: &str) -> Result<Registry> {
    for registry in read_config()?.registry {
        if let Some(scope) = &registry.scope {
            if name.starts_with(scope) {
                return Ok(registry);
            }
        } else {
            return Ok(registry);
        }
    }

    Ok(Registry {
        url: "https://registry.npmjs.org".into(),
        scope: None,
        auth: None,
    })
}

#[tracing::instrument]
pub fn fetch_package(name: &str) -> Result<Arc<RegistryResponse>> {
    #[tracing::instrument]
    fn fetch_package(name: &str) -> Result<RegistryResponse> {
        let selected_registry = select_registry(name)?;

        retry(|| {
            let body = net::get(
                &format!("{}/{name}", selected_registry.url),
                selected_registry.auth.as_ref(),
            )?;
            decode_json(&body).map_err(|e| eyre!("[{name}] {e}"))
        })
    }

    static CACHE: LazyLock<Cache<CompactString, ArcResult<Arc<RegistryResponse>>>> =
        LazyLock::new(|| {
            Cache::new(|key: &CompactString| fetch_package(key).map(Arc::new).map_err(Arc::new))
        });

    CACHE.get(name.to_compact_string()).map_err(Report::msg)
}

#[tracing::instrument]
pub fn fetch_versioned_package(d: PackageSpecifier) -> Result<(Version, Arc<PackageInfo>)> {
    log_progress(&format!("Fetched {}", d.name.bright_blue()));

    match &d.version {
        VersionSpecifier::Other(tag) => {
            let res = fetch_package(&d.name)?;
            let tag = res
                .dist_tags
                .get(tag)
                .wrap_err_with(|| eyre!("Version cannot be satisfied: {} {}", d.name, d.version))?;
            let version = Version::parse(tag)?;
            let package = res.versions.get(&version).wrap_err_with(|| {
                eyre!(
                    "Tag refers to a version that does not exist: {} - {} refers to {}",
                    d.name,
                    d.version,
                    version
                )
            })?;

            Ok((version, Arc::new(package.clone().info())))
        }
        VersionSpecifier::Range(_) => {
            let res = fetch_package(&d.name)?;
            let (version, package) = res
                .versions
                .iter()
                .sorted_by_key(|(v, _)| !v.is_prerelease())
                .rfind(|(v, _)| d.version.satisfies(v))
                .wrap_err_with(|| {
                    eyre!(
                        "Version cannot be satisfied: expected {} {}",
                        d.name,
                        d.version
                    )
                })?;

            Ok((version.clone(), Arc::new(package.clone().info())))
        }
        VersionSpecifier::DirectUrl(url) => {
            log_verbose(&format!(
                "Downloading metadata for {}@{}",
                d.name, d.version
            ));

            // A tarball URL given as the version: the package's own package.json is inside it.
            let tarball = net::get(url.as_str(), None)?;
            let mut archive = Archive::new(GzDecoder::new(&tarball[..]));
            let mut entries = archive.entries()?;

            while let Some(mut entry) = entries.next().transpose()? {
                if entry.path()?.to_str() == Some("package/package.json") {
                    let mut buf = String::new();
                    entry.read_to_string(&mut buf)?;

                    let mut package: PackageMetadata = serde_json::from_str(&buf)?;
                    let version = package.version.clone().wrap_err_with(|| {
                        format!("Package from {url} does not specify a version")
                    })?;

                    package.dist.tarball = url.to_compact_string();

                    return Ok((version, Arc::new(package.info())));
                }
            }

            Err(eyre!("Package from {url} does not contain package.json"))
        }
        VersionSpecifier::Prefixed(prefixed) => match prefixed.prefix.as_str() {
            "npm" => {
                let (actual_name, actual_req) = prefixed
                    .rest
                    .rsplit_once('@')
                    .ok_or_else(|| eyre!("Invalid prefixed version: {prefixed}"))?;

                let actual_req = VersionSpecifier::Range(actual_req.parse()?);

                let inner_req = PackageSpecifier {
                    name: actual_name.to_compact_string(),
                    version: actual_req,
                    optional: d.optional,
                };

                let (inner_version, mut inner_pkg) = fetch_versioned_package(inner_req)?;
                Arc::make_mut(&mut inner_pkg).name = d.name;

                Ok((inner_version, inner_pkg))
            }
            _ => Err(eyre!("Unsupported version prefix")),
        },
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct DependencyTree {
    #[serde(flatten)]
    pub root: Dependency,
    pub children: FxHashMap<CompactString, DependencyTree>,
}

impl DependencyTree {
    pub fn filter(&self, exclude: &FxHashSet<Dependency>) -> Self {
        Self {
            root: self.root.clone(),
            children: self
                .children
                .iter()
                .filter_map(|(name, tree)| {
                    if !exclude.contains(&tree.root) {
                        Some((name.clone(), tree.filter(exclude)))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: CompactString,
    pub version: Version,
    pub dist: Dist,
    pub bins: BTreeMap<CompactString, CompactString>,
    pub scripts: BTreeMap<CompactString, CompactString>,
}

impl Dependency {
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version).replace(MAIN_SEPARATOR, "!")
    }
}
