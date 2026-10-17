//! Eclipse Temurin builds from the Adoptium API, for platforms Mojang does not cover.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::marker::{RuntimeMarker, io_error, read_marker, remove_marker, write_marker};
use super::provider::{ManagedJavaRuntimes, download_error, safe_segment};
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::java::{JavaProbe, JavaRequirement, JavaRuntimeError};
use crate::domain::launch::{OsName, Platform};
use crate::domain::ports::{
    DownloadRequest, Downloader, HttpClient, HttpError, ProgressReporter, ProgressStage,
    StagedReporter,
};
use crate::infrastructure::archive::{extract_tar_gz, extract_zip};

const SOURCE: &str = "adoptium";
/// How deep below the install directory the runtime's home may sit.
const MAX_HOME_DEPTH: usize = 4;

/// `GET /v3/assets/latest/{major}/hotspot` for one OS, architecture and image type.
pub fn adoptium_assets_url(major: u32, os: &str, arch: &str, image_type: &str) -> String {
    format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={arch}&image_type={image_type}&os={os}&vendor=eclipse"
    )
}

#[derive(Debug, Clone, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    release_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Clone, Deserialize)]
struct AdoptiumPackage {
    link: String,
    checksum: String,
    name: String,
    #[serde(default)]
    size: Option<u64>,
}

impl<H: HttpClient, D: Downloader, P: JavaProbe> ManagedJavaRuntimes<H, D, P> {
    /// The executable of an installed Temurin runtime (JRE, or JDK when no JRE exists).
    pub(super) async fn ensure_adoptium<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        platform: &Platform,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<PathBuf, JavaRuntimeError> {
        let major = requirement.major;
        let (os, arch) = platform.adoptium();
        let install_dir = self
            .runtimes_dir
            .join(SOURCE)
            .join(major.to_string())
            .join(format!("{os}-{arch}"));
        let installed = read_marker(&install_dir).await;

        let asset = match self.latest_adoptium(major, os, arch).await {
            Ok(asset) => asset,
            Err(error) => {
                // Offline: any completed install of this major will do.
                let wanted = marker(major, "", "");
                if installed
                    .as_ref()
                    .is_some_and(|found| found.same_runtime(&wanted))
                    && let Some(executable) = locate_java(&install_dir, platform).await
                {
                    return Ok(executable);
                }
                return Err(error);
            }
        };
        let Some(asset) = asset else {
            return Err(JavaRuntimeError::Unavailable {
                major,
                platform: format!("{os}-{arch}"),
            });
        };

        let expected = marker(major, &asset.release_name, &asset.binary.package.name);
        if installed.as_ref() == Some(&expected)
            && let Some(executable) = locate_java(&install_dir, platform).await
        {
            return Ok(executable);
        }
        self.install_adoptium(&asset.binary.package, &install_dir, reporter, cancel)
            .await?;
        let executable = locate_java(&install_dir, platform).await.ok_or_else(|| {
            JavaRuntimeError::Invalid(format!(
                "{} contains no Java executable",
                asset.binary.package.name
            ))
        })?;
        write_marker(&install_dir, &expected).await?;
        Ok(executable)
    }

    /// The newest build: a JRE, else a JDK; `Ok(None)` when Adoptium has neither.
    async fn latest_adoptium(
        &self,
        major: u32,
        os: &str,
        arch: &str,
    ) -> Result<Option<AdoptiumAsset>, JavaRuntimeError> {
        for image_type in ["jre", "jdk"] {
            let url = adoptium_assets_url(major, os, arch, image_type);
            let assets: Vec<AdoptiumAsset> = match self.http.get_bytes(&url).await {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                    JavaRuntimeError::Download(format!("invalid Adoptium response: {error}"))
                })?,
                // Adoptium answers 404 when no build matches the query.
                Err(HttpError::NotFound(_)) => Vec::new(),
                Err(error) => return Err(JavaRuntimeError::Download(error.to_string())),
            };
            if let Some(asset) = assets.into_iter().next() {
                return Ok(Some(asset));
            }
        }
        Ok(None)
    }

    /// Downloads the package to `tmp/`, extracts it into a staging directory
    /// and moves that into place, so a failed install never leaves a half
    /// extracted runtime behind.
    async fn install_adoptium<R: ProgressReporter>(
        &self,
        package: &AdoptiumPackage,
        install_dir: &Path,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<(), JavaRuntimeError> {
        let name = safe_segment(&package.name)?;
        let checksum = Checksum::parse(HashAlgorithm::Sha256, &package.checksum)
            .map_err(|error| JavaRuntimeError::Invalid(format!("{name}: {error}")))?;
        let tmp = self.runtimes_dir.join("tmp");
        let archive = tmp.join(name);
        let staging = tmp.join(format!("{name}.staging"));
        tokio::fs::create_dir_all(&tmp)
            .await
            .map_err(|error| io_error(&tmp, &error))?;

        let reporter = StagedReporter::new(reporter, ProgressStage::Java);
        self.downloader
            .download_all(
                vec![DownloadRequest {
                    url: package.link.clone(),
                    destination: archive.clone(),
                    checksum: Some(checksum),
                    size: package.size,
                }],
                &reporter,
                cancel,
            )
            .await
            .map_err(download_error)?;
        if cancel.is_cancelled() {
            return Err(JavaRuntimeError::Cancelled);
        }

        remove_marker(install_dir).await;
        let is_zip = name.to_ascii_lowercase().ends_with(".zip");
        let install_dir = install_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let result = unpack(&archive, &staging, &install_dir, is_zip);
            // Best effort: leftovers in tmp/ are overwritten by the next install.
            let _ = std::fs::remove_file(&archive);
            if result.is_err() {
                let _ = std::fs::remove_dir_all(&staging);
            }
            result
        })
        .await
        .map_err(|error| JavaRuntimeError::Invalid(error.to_string()))?
    }
}

/// Blocking: extract `archive` into `staging`, then replace `install_dir` with it.
fn unpack(
    archive: &Path,
    staging: &Path,
    install_dir: &Path,
    is_zip: bool,
) -> Result<(), JavaRuntimeError> {
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|error| io_error(staging, &error))?;
    }
    let extracted = if is_zip {
        extract_zip(archive, staging, &[])
    } else {
        extract_tar_gz(archive, staging)
    };
    extracted.map_err(|error| {
        JavaRuntimeError::Invalid(format!("could not extract the runtime: {error}"))
    })?;
    if install_dir.exists() {
        std::fs::remove_dir_all(install_dir).map_err(|error| io_error(install_dir, &error))?;
    }
    if let Some(parent) = install_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    std::fs::rename(staging, install_dir).map_err(|error| io_error(install_dir, &error))
}

/// Finds `bin/java` (`bin/javaw.exe` on Windows, `Contents/Home/bin/java` in a
/// macOS bundle) at most [`MAX_HOME_DEPTH`] directories below `install_dir`.
async fn locate_java(install_dir: &Path, platform: &Platform) -> Option<PathBuf> {
    let install_dir = install_dir.to_path_buf();
    let platform = platform.clone();
    tokio::task::spawn_blocking(move || find_java(&install_dir, &platform))
        .await
        .ok()
        .flatten()
}

fn find_java(install_dir: &Path, platform: &Platform) -> Option<PathBuf> {
    let executable = platform.java_executable(false);
    let mut level = vec![install_dir.to_path_buf()];
    for _ in 0..=MAX_HOME_DEPTH {
        let mut next = Vec::new();
        for dir in level {
            if platform.os == OsName::MacOs {
                let bundled = dir.join("Contents").join("Home").join("bin").join("java");
                if bundled.is_file() {
                    return Some(bundled);
                }
            }
            let candidate = dir.join("bin").join(executable);
            if candidate.is_file() {
                return Some(candidate);
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut children: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                // `DirEntry::file_type` does not follow symlinks.
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect();
            children.sort();
            next.extend(children);
        }
        level = next;
    }
    None
}

fn marker(major: u32, release_name: &str, package_name: &str) -> RuntimeMarker {
    RuntimeMarker {
        source: SOURCE.to_owned(),
        id: major.to_string(),
        version: release_name.to_owned(),
        revision: package_name.to_owned(),
    }
}
