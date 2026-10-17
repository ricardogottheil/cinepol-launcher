use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use super::marker::{RuntimeMarker, read_marker, remove_marker};
use crate::domain::java::{
    JavaProbe, JavaRequirement, JavaRuntime, JavaRuntimeError, JavaRuntimeProvider, JavaSource,
};
use crate::domain::launch::{Platform, RelativePath};
use crate::domain::ports::{DownloadError, Downloader, HttpClient, ProgressReporter};

/// Java runtimes downloaded by the launcher into `runtimes_dir`:
///
/// - `mojang/<component>/<platform key>/` from Mojang's runtime manifest;
/// - `adoptium/<major>/<os>-<arch>/` from Adoptium (Temurin) when Mojang has
///   no build for the platform (for example Linux arm64);
/// - `tmp/` for downloaded archives and staging directories.
///
/// An install is reused while its marker matches what the source offers, or
/// whenever the source cannot be reached. Every runtime is probed before use.
pub struct ManagedJavaRuntimes<H, D, P> {
    pub(super) http: H,
    pub(super) downloader: D,
    pub(super) probe: P,
    pub(super) runtimes_dir: PathBuf,
}

impl<H, D, P> ManagedJavaRuntimes<H, D, P> {
    pub fn new(http: H, downloader: D, probe: P, runtimes_dir: PathBuf) -> Self {
        Self {
            http,
            downloader,
            probe,
            runtimes_dir,
        }
    }
}

impl<H: HttpClient, D: Downloader, P: JavaProbe> JavaRuntimeProvider
    for ManagedJavaRuntimes<H, D, P>
{
    async fn ensure<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        platform: &Platform,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<JavaRuntime, JavaRuntimeError> {
        if cancel.is_cancelled() {
            return Err(JavaRuntimeError::Cancelled);
        }
        let executable = self
            .install(requirement, platform, reporter, cancel)
            .await?;
        match self.check(executable, requirement).await {
            Ok(runtime) => Ok(runtime),
            Err(error) => {
                // A runtime that no longer runs (damaged files, interrupted update)
                // is installed again once: forgetting its marker makes the next
                // install re-verify every file and replace the broken ones.
                tracing::warn!(%error, "the managed Java runtime is broken; reinstalling it");
                self.forget_installs(requirement, platform).await;
                let executable = self
                    .install(requirement, platform, reporter, cancel)
                    .await?;
                self.check(executable, requirement).await
            }
        }
    }
}

impl<H: HttpClient, D: Downloader, P: JavaProbe> ManagedJavaRuntimes<H, D, P> {
    /// Mojang first, Adoptium when Mojang has nothing for the platform.
    async fn install<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        platform: &Platform,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<PathBuf, JavaRuntimeError> {
        let mojang = match platform.mojang_runtime_platform() {
            Some(key) => {
                self.ensure_mojang(requirement, platform, key, reporter, cancel)
                    .await?
            }
            None => None,
        };
        match mojang {
            Some(executable) => Ok(executable),
            None => {
                self.ensure_adoptium(requirement, platform, reporter, cancel)
                    .await
            }
        }
    }

    /// Runs the executable and checks it is the required major version.
    async fn check(
        &self,
        executable: PathBuf,
        requirement: &JavaRequirement,
    ) -> Result<JavaRuntime, JavaRuntimeError> {
        let version = self
            .probe
            .probe(&executable)
            .await
            .map_err(|error| JavaRuntimeError::Invalid(error.to_string()))?;
        if version.major != requirement.major {
            return Err(JavaRuntimeError::Invalid(format!(
                "{} is Java {}, expected Java {}",
                executable.display(),
                version.major,
                requirement.major
            )));
        }
        Ok(JavaRuntime {
            executable,
            version,
            source: JavaSource::Managed,
        })
    }

    /// Removes the markers of every install that could serve `requirement`.
    async fn forget_installs(&self, requirement: &JavaRequirement, platform: &Platform) {
        if let (Some(key), Ok(component)) = (
            platform.mojang_runtime_platform(),
            safe_segment(&requirement.component),
        ) {
            remove_marker(&self.runtimes_dir.join("mojang").join(component).join(key)).await;
        }
        let (os, arch) = platform.adoptium();
        remove_marker(
            &self
                .runtimes_dir
                .join("adoptium")
                .join(requirement.major.to_string())
                .join(format!("{os}-{arch}")),
        )
        .await;
    }
}

/// A single safe path segment (component names, package file names).
pub(super) fn safe_segment(raw: &str) -> Result<&str, JavaRuntimeError> {
    match RelativePath::parse(raw) {
        Ok(_) if !raw.contains('/') => Ok(raw),
        _ => Err(JavaRuntimeError::Invalid(format!(
            "unsafe runtime name {raw:?}"
        ))),
    }
}

pub(super) fn download_error(error: DownloadError) -> JavaRuntimeError {
    match error {
        DownloadError::Cancelled => JavaRuntimeError::Cancelled,
        other => JavaRuntimeError::Download(other.to_string()),
    }
}

pub(super) async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
}

/// True when `install_dir` holds a completed install accepted by `accept`
/// and its executable exists.
pub(super) async fn reusable(
    install_dir: &Path,
    executable: &Path,
    accept: impl FnOnce(&RuntimeMarker) -> bool,
) -> bool {
    match read_marker(install_dir).await {
        Some(found) => accept(&found) && is_file(executable).await,
        None => false,
    }
}
