//! Downloads everything an instance needs to launch: the version JSON, client
//! jar, libraries, natives jars, logging config, assets and the Java runtime.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::instance::{
    InstanceId, InstanceRepository, InstanceRepositoryError, JavaSelection,
};
use crate::domain::java::{JavaRequirement, JavaRuntimeError, JavaRuntimeProvider};
use crate::domain::launch::{
    Artifact, Features, LauncherPaths, Platform, VersionSource, VersionSourceError, plan_libraries,
};
use crate::domain::ports::{
    DownloadError, DownloadRequest, DownloadSummary, Downloader, ProgressReporter, ProgressStage,
    ProgressUpdate, StagedReporter,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Version(#[from] VersionSourceError),
    #[error(transparent)]
    Download(DownloadError),
    #[error(transparent)]
    Java(JavaRuntimeError),
    #[error("the installation was cancelled")]
    Cancelled,
}

impl From<DownloadError> for InstallError {
    fn from(error: DownloadError) -> Self {
        if error == DownloadError::Cancelled {
            Self::Cancelled
        } else {
            Self::Download(error)
        }
    }
}

impl From<JavaRuntimeError> for InstallError {
    fn from(error: JavaRuntimeError) -> Self {
        if error == JavaRuntimeError::Cancelled {
            Self::Cancelled
        } else {
            Self::Java(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallSummary {
    pub files: DownloadSummary,
    pub assets: DownloadSummary,
    /// Major version of the managed Java runtime made available, if any.
    pub java_major: Option<u32>,
}

/// Everything the installer talks to.
pub struct InstallDeps<'a, I, V, D, J> {
    pub instances: &'a I,
    pub versions: &'a V,
    pub downloader: &'a D,
    pub java: &'a J,
    pub paths: &'a LauncherPaths,
    pub platform: &'a Platform,
}

fn request(root: &std::path::Path, artifact: &Artifact) -> Option<DownloadRequest> {
    // Libraries with no URL are produced locally (loader processors), not downloaded.
    (!artifact.url.is_empty()).then(|| DownloadRequest {
        url: artifact.url.clone(),
        destination: artifact.path.under(root),
        checksum: artifact.checksum.clone(),
        size: artifact.size,
    })
}

/// Installing is idempotent: files that are already present and verified are reused.
pub async fn install_instance<I, V, D, J, R>(
    id: &InstanceId,
    deps: &InstallDeps<'_, I, V, D, J>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstallSummary, InstallError>
where
    I: InstanceRepository,
    V: VersionSource,
    D: Downloader,
    J: JavaRuntimeProvider,
    R: ProgressReporter,
{
    let metadata = StagedReporter::new(reporter, ProgressStage::Metadata);
    metadata.report(ProgressUpdate::new(0, Some(2)));
    let instance = deps.instances.get(id).await?;
    let spec = deps
        .versions
        .version(instance.game_version.as_str())
        .await?;
    let asset_index = match &spec.asset_index {
        Some(index) => Some(deps.versions.asset_index(index).await?),
        None => None,
    };
    metadata.report(ProgressUpdate::new(2, Some(2)));
    if cancel.is_cancelled() {
        return Err(InstallError::Cancelled);
    }

    let libraries_dir = deps.paths.libraries_dir();
    let assets_dir = deps.paths.assets_dir();
    let plan = plan_libraries(&spec, deps.platform, &Features::new());
    let mut files: Vec<DownloadRequest> = spec
        .client_jar
        .iter()
        .filter_map(|jar| request(&deps.paths.versions_dir(), jar))
        .collect();
    files.extend(
        plan.downloads()
            .filter_map(|artifact| request(&libraries_dir, artifact)),
    );
    files.extend(
        spec.logging
            .iter()
            .filter_map(|logging| request(&assets_dir, &logging.file)),
    );
    let files = deps
        .downloader
        .download_all(
            files,
            &StagedReporter::new(reporter, ProgressStage::Libraries),
            cancel,
        )
        .await?;

    let asset_requests: Vec<DownloadRequest> = asset_index
        .iter()
        .flat_map(|index| index.objects.iter())
        .filter_map(|object| request(&assets_dir, &object.artifact()))
        .collect();
    let assets = deps
        .downloader
        .download_all(
            asset_requests,
            &StagedReporter::new(reporter, ProgressStage::Assets),
            cancel,
        )
        .await?;

    let java_major = match instance.settings.java {
        JavaSelection::Managed => {
            let requirement = spec.java.clone().unwrap_or_else(JavaRequirement::legacy);
            let runtime = deps
                .java
                .ensure(
                    &requirement,
                    deps.platform,
                    &StagedReporter::new(reporter, ProgressStage::Java),
                    cancel,
                )
                .await?;
            Some(runtime.version.major)
        }
        JavaSelection::Custom(_) => None,
    };

    Ok(InstallSummary {
        files,
        assets,
        java_major,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::java::{JavaRuntime, JavaSource, JavaVersion};
    use crate::domain::launch::{Arch, OsName, VersionManifest};
    use crate::domain::loader::LoaderSpec;
    use crate::testing::{
        FakeDownloader, FakeJavaRuntimes, FakeVersionSource, InMemoryInstanceRepository,
        RecordingReporter, sample_asset_index, sample_version_spec,
    };

    struct World {
        instances: InMemoryInstanceRepository,
        versions: FakeVersionSource,
        downloader: FakeDownloader,
        java: FakeJavaRuntimes,
        paths: LauncherPaths,
        platform: Platform,
        dir: tempfile::TempDir,
    }

    impl World {
        async fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let instances = InMemoryInstanceRepository::default();
            instances
                .save(&Instance::new(
                    InstanceId::parse("cinepol").unwrap(),
                    InstanceName::parse("CINEPOL").unwrap(),
                    GameVersion::parse("26.2").unwrap(),
                    LoaderSpec::vanilla(),
                    InstanceOrigin::CinepolServer,
                ))
                .await
                .unwrap();
            let versions = FakeVersionSource {
                manifest: VersionManifest {
                    latest_release: GameVersion::parse("26.2").unwrap(),
                    latest_snapshot: GameVersion::parse("26.2").unwrap(),
                    versions: vec![],
                },
                versions: HashMap::from([("26.2".to_owned(), sample_version_spec("26.2"))]),
                asset_indexes: HashMap::from([("32".to_owned(), sample_asset_index())]),
            };
            let java = FakeJavaRuntimes::providing(JavaRuntime {
                executable: PathBuf::from("/runtimes/java"),
                version: JavaVersion {
                    major: 25,
                    full: "25.0.1".into(),
                    is_64_bit: true,
                },
                source: JavaSource::Managed,
            });
            let paths = LauncherPaths::new(dir.path());
            Self {
                instances,
                versions,
                downloader: FakeDownloader::default(),
                java,
                paths,
                platform: Platform::new(OsName::MacOs, Arch::Arm64),
                dir,
            }
        }

        fn deps(
            &self,
        ) -> InstallDeps<
            '_,
            InMemoryInstanceRepository,
            FakeVersionSource,
            FakeDownloader,
            FakeJavaRuntimes,
        > {
            InstallDeps {
                instances: &self.instances,
                versions: &self.versions,
                downloader: &self.downloader,
                java: &self.java,
                paths: &self.paths,
                platform: &self.platform,
            }
        }
    }

    fn id() -> InstanceId {
        InstanceId::parse("cinepol").unwrap()
    }

    #[tokio::test]
    async fn downloads_client_libraries_logging_assets_and_java() {
        let world = World::new().await;
        let reporter = RecordingReporter::default();
        let summary = install_instance(&id(), &world.deps(), &reporter, &CancellationToken::new())
            .await
            .unwrap();

        let destinations: Vec<PathBuf> = world
            .downloader
            .requests()
            .into_iter()
            .map(|r| r.destination)
            .collect();
        let root = world.dir.path();
        assert_eq!(
            destinations,
            vec![
                root.join("versions").join("26.2").join("26.2.jar"),
                root.join("libraries")
                    .join("org")
                    .join("lwjgl")
                    .join("lwjgl")
                    .join("3.4.1")
                    .join("lwjgl-3.4.1.jar"),
                root.join("assets")
                    .join("log_configs")
                    .join("client-1.21.2.xml"),
                root.join("assets")
                    .join("objects")
                    .join("ee")
                    .join("e".repeat(40)),
                root.join("assets")
                    .join("objects")
                    .join("ff")
                    .join("f".repeat(40)),
            ]
        );
        assert!(
            world
                .downloader
                .requests()
                .iter()
                .all(|r| r.checksum.is_some()),
            "every declared hash is verified"
        );
        assert_eq!(
            (
                summary.files.downloaded,
                summary.assets.downloaded,
                summary.java_major
            ),
            (3, 2, Some(25))
        );
        assert_eq!(world.java.requested(), vec![25]);
    }

    #[tokio::test]
    async fn reports_progress_by_stage() {
        let world = World::new().await;
        let reporter = RecordingReporter::default();
        install_instance(&id(), &world.deps(), &reporter, &CancellationToken::new())
            .await
            .unwrap();
        let stages: Vec<_> = reporter
            .updates()
            .into_iter()
            .filter_map(|u| u.stage)
            .collect();
        for stage in [
            ProgressStage::Metadata,
            ProgressStage::Libraries,
            ProgressStage::Assets,
            ProgressStage::Java,
        ] {
            assert!(stages.contains(&stage), "{stage:?} missing");
        }
        assert!(reporter.updates().iter().all(|u| u.stage.is_some()));
    }

    #[tokio::test]
    async fn uses_java_8_when_the_version_declares_no_runtime() {
        let mut world = World::new().await;
        world.versions.versions.get_mut("26.2").unwrap().java = None;
        install_instance(
            &id(),
            &world.deps(),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(world.java.requested(), vec![8]);
    }

    #[tokio::test]
    async fn skips_managed_java_for_a_custom_runtime() {
        let world = World::new().await;
        let mut instance = world.instances.get(&id()).await.unwrap();
        instance.settings.java = JavaSelection::Custom(PathBuf::from("/opt/java/bin/java"));
        world.instances.save(&instance).await.unwrap();
        let summary = install_instance(
            &id(),
            &world.deps(),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(summary.java_major, None);
        assert!(world.java.requested().is_empty());
    }

    #[tokio::test]
    async fn stops_when_cancelled_and_reports_unknown_instances_and_versions() {
        let world = World::new().await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            install_instance(&id(), &world.deps(), &RecordingReporter::default(), &cancel).await,
            Err(InstallError::Cancelled)
        );
        assert!(world.downloader.requests().is_empty());

        let missing = InstanceId::parse("missing").unwrap();
        assert!(matches!(
            install_instance(
                &missing,
                &world.deps(),
                &RecordingReporter::default(),
                &CancellationToken::new()
            )
            .await,
            Err(InstallError::Instance(InstanceRepositoryError::NotFound(_)))
        ));

        let mut unknown = World::new().await;
        unknown.versions.versions.clear();
        assert!(matches!(
            install_instance(
                &id(),
                &unknown.deps(),
                &RecordingReporter::default(),
                &CancellationToken::new()
            )
            .await,
            Err(InstallError::Version(VersionSourceError::UnknownVersion(_)))
        ));
    }
}
