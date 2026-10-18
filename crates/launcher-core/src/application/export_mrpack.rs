//! Exports an instance as a Modrinth `.mrpack`.

use std::path::Path;

use thiserror::Error;

use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::mrpack::{EXPORT_ROOTS, MrpackIndex, is_exportable};
use crate::domain::modpack::sync::plan_export;
use crate::domain::modpack::{
    ModpackSource, ModpackSourceError, MrpackArchive, PackFileError, PackFiles,
};

/// The pack version written when the player gives none.
pub const DEFAULT_EXPORT_VERSION: &str = "1.0.0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportMrpackError {
    #[error("{0} is not a .mrpack file")]
    NotMrpack(String),
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    /// Without the catalog, every mod would end up copied into the pack.
    #[error("could not look up the files in the catalog: {0}")]
    Catalog(ModpackSourceError),
    #[error(transparent)]
    Files(#[from] PackFileError),
}

pub struct ExportMrpackRequest<'a> {
    pub instance: &'a InstanceId,
    /// The archive to write; must end in `.mrpack`.
    pub destination: &'a Path,
    /// The pack's version label; [`DEFAULT_EXPORT_VERSION`] when `None` or blank.
    pub version: Option<&'a str>,
}

/// Everything the exporter talks to.
pub struct ExportMrpackDeps<'a, I, C, A, P> {
    pub instances: &'a I,
    pub source: &'a C,
    pub archive: &'a A,
    pub files: &'a P,
    pub paths: &'a LauncherPaths,
}

/// How many files went into the index and how many were copied as overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportSummary {
    pub indexed: usize,
    pub overrides: usize,
}

/// Files the catalog can serve go into the index (by hash and download URL);
/// everything else under [`EXPORT_ROOTS`] is copied into `overrides/`.
pub async fn export_mrpack<I, C, A, P>(
    request: ExportMrpackRequest<'_>,
    deps: &ExportMrpackDeps<'_, I, C, A, P>,
) -> Result<ExportSummary, ExportMrpackError>
where
    I: InstanceRepository,
    C: ModpackSource,
    A: MrpackArchive,
    P: PackFiles,
{
    let destination = request.destination;
    let is_mrpack = destination
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mrpack"));
    if !is_mrpack {
        return Err(ExportMrpackError::NotMrpack(
            destination.display().to_string(),
        ));
    }
    let instance = deps.instances.get(request.instance).await?;
    let game_dir = deps.paths.game_dir(&instance.id);
    let local = deps.files.scan(&game_dir, &EXPORT_ROOTS).await?;

    let mut hashes: Vec<String> = local
        .iter()
        .filter(|file| is_exportable(&file.path))
        .map(|file| file.sha512.value().to_owned())
        .collect();
    hashes.sort();
    hashes.dedup();
    let known = if hashes.is_empty() {
        Default::default()
    } else {
        deps.source
            .known_files(&hashes)
            .await
            .map_err(ExportMrpackError::Catalog)?
    };
    let plan = plan_export(&local, &known);

    let version = request
        .version
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .unwrap_or(DEFAULT_EXPORT_VERSION);
    let index = MrpackIndex {
        name: instance.name.as_str().to_owned(),
        version_id: version.to_owned(),
        summary: None,
        game_version: instance.game_version.clone(),
        loader: instance.loader.clone(),
        files: plan.files,
    };
    let overrides: Vec<_> = plan
        .overrides
        .into_iter()
        .map(|path| {
            let source = path.under(&game_dir);
            (path, source)
        })
        .collect();
    deps.archive.write(destination, &index, &overrides).await?;
    Ok(ExportSummary {
        indexed: index.files.len(),
        overrides: overrides.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::application::search_modpacks::fixtures::catalog;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::domain::modpack::{ModpackPage, ModpackQuery, ModpackSourceKind, ModpackSummary};
    use crate::domain::modpack::{ModpackVersion, mrpack::EnvSupport};
    use crate::testing::{
        FakeMrpackArchive, FakePackFiles, InMemoryInstanceRepository, sha512_hex,
    };

    const KNOWN_URL: &str = "https://cdn.modrinth.com/data/sodium/sodium.jar";

    struct World {
        _dir: tempfile::TempDir,
        paths: LauncherPaths,
        instances: InMemoryInstanceRepository,
        archive: FakeMrpackArchive,
        files: FakePackFiles,
    }

    impl World {
        async fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = LauncherPaths::new(dir.path());
            let instances = InMemoryInstanceRepository::default();
            let instance = Instance::new(
                InstanceId::parse("my-pack").unwrap(),
                InstanceName::parse("My Pack").unwrap(),
                GameVersion::parse("26.2").unwrap(),
                LoaderSpec::new(
                    LoaderKind::Fabric,
                    Some(LoaderVersion::parse("0.19.5").unwrap()),
                )
                .unwrap(),
                InstanceOrigin::Custom,
            );
            instances.save(&instance).await.unwrap();
            let game = paths.game_dir(&instance.id);
            for (path, bytes) in [
                ("mods/sodium.jar", b"sodium".as_slice()),
                ("mods/custom.jar", b"custom"),
                ("config/sodium.json", b"{}"),
                ("saves/world/level.dat", b"world"),
            ] {
                let target = game.join(path);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(target, bytes).unwrap();
            }
            Self {
                _dir: dir,
                paths,
                instances,
                archive: FakeMrpackArchive::default(),
                files: FakePackFiles::default(),
            }
        }

        fn deps<'a, C>(
            &'a self,
            source: &'a C,
        ) -> ExportMrpackDeps<'a, InMemoryInstanceRepository, C, FakeMrpackArchive, FakePackFiles>
        {
            ExportMrpackDeps {
                instances: &self.instances,
                source,
                archive: &self.archive,
                files: &self.files,
                paths: &self.paths,
            }
        }

        fn game(&self) -> std::path::PathBuf {
            self.paths.game_dir(&InstanceId::parse("my-pack").unwrap())
        }
    }

    fn knowing_sodium() -> crate::testing::FakeModpackSource {
        let mut source = catalog();
        source.known = HashMap::from([(sha512_hex(b"sodium"), vec![KNOWN_URL.to_owned()])]);
        source
    }

    fn request<'a>(
        id: &'a InstanceId,
        destination: &'a Path,
        version: Option<&'a str>,
    ) -> ExportMrpackRequest<'a> {
        ExportMrpackRequest {
            instance: id,
            destination,
            version,
        }
    }

    #[tokio::test]
    async fn indexes_known_files_and_copies_the_rest_as_overrides() {
        let world = World::new().await;
        let source = knowing_sodium();
        let id = InstanceId::parse("my-pack").unwrap();
        let destination = Path::new("/exports/My Pack.mrpack");
        let summary = export_mrpack(request(&id, destination, Some("2.0")), &world.deps(&source))
            .await
            .unwrap();
        assert_eq!(
            summary,
            ExportSummary {
                indexed: 1,
                overrides: 2
            }
        );

        let written = world.archive.written();
        assert_eq!(written.len(), 1);
        let (archive, index, overrides) = &written[0];
        assert_eq!(archive, destination);
        assert_eq!(index.name, "My Pack");
        assert_eq!(index.version_id, "2.0");
        assert_eq!(index.game_version.as_str(), "26.2");
        assert_eq!(index.loader.kind(), LoaderKind::Fabric);
        assert_eq!(index.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(index.files.len(), 1);
        let sodium = &index.files[0];
        assert_eq!(sodium.path.as_str(), "mods/sodium.jar");
        assert_eq!(sodium.downloads, [KNOWN_URL]);
        assert_eq!(sodium.sha512.value(), sha512_hex(b"sodium"));
        assert_eq!(sodium.client, EnvSupport::Required);

        let game = world.game();
        let copied: Vec<(&str, std::path::PathBuf)> = overrides
            .iter()
            .map(|(path, source)| (path.as_str(), source.clone()))
            .collect();
        assert_eq!(
            copied,
            [
                ("mods/custom.jar", game.join("mods/custom.jar")),
                ("config/sodium.json", game.join("config/sodium.json")),
            ]
        );
    }

    #[tokio::test]
    async fn uses_a_default_version_label() {
        let world = World::new().await;
        let source = knowing_sodium();
        let id = InstanceId::parse("my-pack").unwrap();
        export_mrpack(
            request(&id, Path::new("/exports/a.MRPACK"), None),
            &world.deps(&source),
        )
        .await
        .unwrap();
        assert_eq!(
            world.archive.written()[0].1.version_id,
            DEFAULT_EXPORT_VERSION
        );
    }

    #[tokio::test]
    async fn refuses_a_destination_that_is_not_an_mrpack() {
        let world = World::new().await;
        let source = knowing_sodium();
        let id = InstanceId::parse("my-pack").unwrap();
        for destination in ["/exports/pack.zip", "/exports/pack"] {
            assert_eq!(
                export_mrpack(
                    request(&id, Path::new(destination), None),
                    &world.deps(&source)
                )
                .await,
                Err(ExportMrpackError::NotMrpack(destination.into()))
            );
        }
        assert!(world.archive.written().is_empty());
    }

    /// A catalog that cannot be reached.
    struct UnreachableCatalog;

    impl ModpackSource for UnreachableCatalog {
        fn kind(&self) -> ModpackSourceKind {
            ModpackSourceKind::Modrinth
        }

        async fn search(&self, _query: &ModpackQuery) -> Result<ModpackPage, ModpackSourceError> {
            Err(ModpackSourceError::Network("offline".into()))
        }

        async fn project(&self, _project: &str) -> Result<ModpackSummary, ModpackSourceError> {
            Err(ModpackSourceError::Network("offline".into()))
        }

        async fn versions(
            &self,
            _project: &str,
        ) -> Result<Vec<ModpackVersion>, ModpackSourceError> {
            Err(ModpackSourceError::Network("offline".into()))
        }

        async fn known_files(
            &self,
            _sha512: &[String],
        ) -> Result<HashMap<String, Vec<String>>, ModpackSourceError> {
            Err(ModpackSourceError::Network("offline".into()))
        }
    }

    #[tokio::test]
    async fn fails_instead_of_exporting_everything_when_the_catalog_is_unreachable() {
        let world = World::new().await;
        let id = InstanceId::parse("my-pack").unwrap();
        assert_eq!(
            export_mrpack(
                request(&id, Path::new("/exports/a.mrpack"), None),
                &world.deps(&UnreachableCatalog)
            )
            .await,
            Err(ExportMrpackError::Catalog(ModpackSourceError::Network(
                "offline".into()
            )))
        );
        assert!(world.archive.written().is_empty());
    }
}
