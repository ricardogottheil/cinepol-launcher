//! Mojang's Java runtime index and per-runtime file manifests.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use super::marker::{RuntimeMarker, io_error, remove_marker, write_marker};
use super::provider::{ManagedJavaRuntimes, download_error, is_file, reusable, safe_segment};
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::java::{JavaProbe, JavaRequirement, JavaRuntimeError};
use crate::domain::launch::{OsName, Platform, RelativePath};
use crate::domain::ports::{
    DownloadRequest, Downloader, HttpClient, ProgressReporter, ProgressStage, StagedReporter,
};
use crate::infrastructure::archive::create_link_inside;

pub const MOJANG_RUNTIME_INDEX_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

const SOURCE: &str = "mojang";

/// `all.json`: platform key -> component -> builds (the first one is current).
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub(super) struct RuntimeIndex(HashMap<String, HashMap<String, Vec<RuntimeEntry>>>);

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RuntimeEntry {
    pub manifest: ManifestRef,
    pub version: RuntimeVersionName,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ManifestRef {
    pub sha1: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RuntimeVersionName {
    pub name: String,
}

impl RuntimeIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// The current build of `component` for `platform`; `None` when Mojang ships none.
    pub fn entry(&self, platform: &str, component: &str) -> Option<&RuntimeEntry> {
        self.0.get(platform)?.get(component)?.first()
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: BTreeMap<String, ManifestFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ManifestFile {
    Directory,
    File {
        downloads: FileDownloads,
        #[serde(default)]
        executable: bool,
    },
    Link {
        target: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: RawDownload,
}

#[derive(Debug, Deserialize)]
struct RawDownload {
    sha1: String,
    size: u64,
    url: String,
}

/// What a manifest asks for, with every key already validated.
#[derive(Debug, Default)]
struct InstallPlan {
    directories: Vec<PathBuf>,
    downloads: Vec<DownloadRequest>,
    executables: Vec<PathBuf>,
    /// `(key, target)`, created after every file is in place.
    links: Vec<(String, String)>,
}

impl InstallPlan {
    fn new(manifest: &RuntimeManifest, install_dir: &Path) -> Result<Self, JavaRuntimeError> {
        let mut plan = Self::default();
        for (key, file) in &manifest.files {
            let relative = RelativePath::parse(key).map_err(|_| {
                JavaRuntimeError::Invalid(format!("unsafe runtime manifest entry {key:?}"))
            })?;
            let path = relative.under(install_dir);
            match file {
                ManifestFile::Directory => plan.directories.push(path),
                ManifestFile::File {
                    downloads,
                    executable,
                } => {
                    let checksum = Checksum::parse(HashAlgorithm::Sha1, &downloads.raw.sha1)
                        .map_err(|error| {
                            JavaRuntimeError::Invalid(format!("runtime file {key:?}: {error}"))
                        })?;
                    if *executable {
                        plan.executables.push(path.clone());
                    }
                    plan.downloads.push(DownloadRequest {
                        url: downloads.raw.url.clone(),
                        destination: path,
                        checksum: Some(checksum),
                        size: Some(downloads.raw.size),
                    });
                }
                ManifestFile::Link { target } => {
                    plan.links
                        .push((relative.as_str().to_owned(), target.clone()));
                }
                ManifestFile::Unknown => {}
            }
        }
        Ok(plan)
    }

    /// Blocking: executable bits, then links (which must resolve inside).
    fn finish(self, install_dir: &Path) -> Result<(), JavaRuntimeError> {
        for path in &self.executables {
            make_executable(path)?;
        }
        for (key, target) in &self.links {
            create_link_inside(install_dir, key, target).map_err(|error| {
                JavaRuntimeError::Invalid(format!("runtime link {key:?}: {error}"))
            })?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), JavaRuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| io_error(path, &error))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), JavaRuntimeError> {
    Ok(())
}

/// `jre.bundle/Contents/Home/bin/java` on macOS, `bin/java[w.exe]` elsewhere.
fn mojang_executable(install_dir: &Path, platform: &Platform) -> PathBuf {
    let home = match platform.os {
        OsName::MacOs => install_dir.join("jre.bundle").join("Contents").join("Home"),
        OsName::Windows | OsName::Linux => install_dir.to_path_buf(),
    };
    home.join("bin").join(platform.java_executable(false))
}

impl<H: HttpClient, D: Downloader, P: JavaProbe> ManagedJavaRuntimes<H, D, P> {
    /// The executable of an installed Mojang runtime; `Ok(None)` when Mojang
    /// has no build of the component for this platform.
    pub(super) async fn ensure_mojang<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        platform: &Platform,
        platform_key: &str,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<Option<PathBuf>, JavaRuntimeError> {
        let component = safe_segment(&requirement.component)?;
        let install_dir = self
            .runtimes_dir
            .join(SOURCE)
            .join(component)
            .join(platform_key);
        let executable = mojang_executable(&install_dir, platform);

        let index = match self.http.get_bytes(MOJANG_RUNTIME_INDEX_URL).await {
            Ok(bytes) => RuntimeIndex::parse(&bytes).map_err(|error| {
                JavaRuntimeError::Download(format!("invalid Java runtime index: {error}"))
            })?,
            Err(error) => {
                // Offline: any completed install of this component will do.
                let wanted = marker(component, "", "");
                if reusable(&install_dir, &executable, |found| {
                    found.same_runtime(&wanted)
                })
                .await
                {
                    return Ok(Some(executable));
                }
                return Err(JavaRuntimeError::Download(error.to_string()));
            }
        };
        let Some(entry) = index.entry(platform_key, component) else {
            return Ok(None);
        };

        let expected = marker(component, &entry.version.name, &entry.manifest.sha1);
        if reusable(&install_dir, &executable, |found| *found == expected).await {
            return Ok(Some(executable));
        }
        self.install_mojang(entry, &install_dir, reporter, cancel)
            .await?;
        if !is_file(&executable).await {
            return Err(JavaRuntimeError::Invalid(format!(
                "the runtime has no Java executable at {}",
                executable.display()
            )));
        }
        write_marker(&install_dir, &expected).await?;
        Ok(Some(executable))
    }

    async fn install_mojang<R: ProgressReporter>(
        &self,
        entry: &RuntimeEntry,
        install_dir: &Path,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<(), JavaRuntimeError> {
        let bytes = self
            .http
            .get_bytes(&entry.manifest.url)
            .await
            .map_err(|error| JavaRuntimeError::Download(error.to_string()))?;
        let actual = hex::encode(Sha1::digest(&bytes));
        if !actual.eq_ignore_ascii_case(&entry.manifest.sha1) {
            return Err(JavaRuntimeError::Download(format!(
                "the runtime manifest {} does not match its sha1 (expected {}, got {actual})",
                entry.manifest.url, entry.manifest.sha1
            )));
        }
        let manifest: RuntimeManifest = serde_json::from_slice(&bytes).map_err(|error| {
            JavaRuntimeError::Invalid(format!("invalid runtime manifest: {error}"))
        })?;
        let mut plan = InstallPlan::new(&manifest, install_dir)?;
        if cancel.is_cancelled() {
            return Err(JavaRuntimeError::Cancelled);
        }

        remove_marker(install_dir).await;
        for directory in &plan.directories {
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(|error| io_error(directory, &error))?;
        }
        let downloads = std::mem::take(&mut plan.downloads);
        if !downloads.is_empty() {
            let reporter = StagedReporter::new(reporter, ProgressStage::Java);
            self.downloader
                .download_all(downloads, &reporter, cancel)
                .await
                .map_err(download_error)?;
        }
        let install_dir = install_dir.to_path_buf();
        tokio::task::spawn_blocking(move || plan.finish(&install_dir))
            .await
            .map_err(|error| JavaRuntimeError::Invalid(error.to_string()))?
    }
}

fn marker(component: &str, version: &str, manifest_sha1: &str) -> RuntimeMarker {
    RuntimeMarker {
        source: SOURCE.to_owned(),
        id: component.to_owned(),
        version: version.to_owned(),
        revision: manifest_sha1.to_ascii_lowercase(),
    }
}
