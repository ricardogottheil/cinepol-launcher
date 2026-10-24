//! Finds the installed mods that have a newer version for the instance's
//! game version and loader. Nothing is written.

use thiserror::Error;

use crate::application::install_mod::MODS_DIR;
use crate::application::mod_inventory::read_mod_inventory;
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{ModpackSourceError, PackFileError, PackFiles};
use crate::domain::mods::{ModRegistryStore, ModSource, ModTarget, ModVersion, available_update};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CheckModUpdatesError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
}

/// A newer version of an installed mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModUpdate {
    pub project: String,
    pub title: String,
    /// The installed jar's name and version label.
    pub filename: String,
    pub current: String,
    pub version: ModVersion,
}

/// What an update check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModUpdates {
    /// Sorted by title.
    pub updates: Vec<ModUpdate>,
    /// Titles of the mods the catalog could not answer for, sorted: they may
    /// have an update too.
    pub unchecked: Vec<String>,
}

/// Everything checking for mod updates talks to.
pub struct CheckModUpdatesDeps<'a, I, S, P, M> {
    pub instances: &'a I,
    pub source: &'a S,
    pub files: &'a P,
    pub registry: &'a M,
    pub paths: &'a LauncherPaths,
}

/// The updates for the mods the launcher knows (installed or recognised),
/// sorted by title. A mod the catalog cannot answer for is listed as
/// unchecked; only when it answers for none is its error returned.
pub async fn check_mod_updates<I, S, P, M>(
    deps: &CheckModUpdatesDeps<'_, I, S, P, M>,
    id: &InstanceId,
) -> Result<ModUpdates, CheckModUpdatesError>
where
    I: InstanceRepository,
    S: ModSource,
    P: PackFiles,
    M: ModRegistryStore,
{
    let instance = deps.instances.get(id).await?;
    let target = ModTarget {
        game_version: instance.game_version.clone(),
        loader: instance.loader.kind(),
    };
    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    let saved = deps.registry.load(id).await?;
    let inventory = read_mod_inventory(deps.source, deps.files, &mods_dir, &saved).await?;
    let mut checked = ModUpdates::default();
    let (mut answered, mut failure) = (false, None);
    for installed in inventory.registry.mods() {
        let versions = match deps.source.versions(&installed.project, &target).await {
            Ok(versions) => {
                answered = true;
                versions
            }
            // Gone from the catalog: nothing to update to.
            Err(ModpackSourceError::NotFound(_)) => {
                answered = true;
                continue;
            }
            Err(error) => {
                tracing::warn!(project = %installed.project, %error, "could not check a mod for updates");
                failure.get_or_insert(error);
                checked.unchecked.push(installed.title.clone());
                continue;
            }
        };
        if let Some(version) = available_update(&versions, &target, &installed.version) {
            checked.updates.push(ModUpdate {
                project: installed.project.clone(),
                title: installed.title.clone(),
                filename: installed.filename.clone(),
                current: installed.version_number.clone(),
                version: version.clone(),
            });
        }
    }
    if let (false, Some(error)) = (answered, failure) {
        return Err(error.into());
    }
    checked
        .updates
        .sort_by_cached_key(|update| update.title.to_lowercase());
    checked
        .unchecked
        .sort_by_cached_key(|title| title.to_lowercase());
    Ok(checked)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::loader::LoaderKind::Fabric;
    use crate::domain::mods::{InstalledMod, ModRegistry, ReleaseChannel};
    use crate::testing::{
        FakeModSource, FakePackFiles, InMemoryInstanceRepository, InMemoryModRegistryStore,
        fabric_instance, mod_jar, mod_version, sha512_hex,
    };

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn installed(project: &str, version: &str) -> InstalledMod {
        InstalledMod {
            project: project.into(),
            title: project.to_uppercase(),
            version: version.into(),
            version_number: version.into(),
            filename: format!("{version}.jar"),
            sha512: sha512_hex(&mod_jar(version)),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: BTreeSet::new(),
        }
    }

    /// Sodium 3 is out over the installed 2; the player runs a Lithium beta
    /// newer than its latest release; Iris is current; one mod is gone from
    /// the catalog. Versions of the `failing` projects cannot be read.
    fn catalog(failing: &[&str]) -> FakeModSource {
        let mut lithium_beta = mod_version("lithium", "lithium-4b", "1.21.1", &[Fabric]);
        lithium_beta.channel = ReleaseChannel::Beta;
        let mut source = FakeModSource::default()
            .with(
                "sodium",
                "Sodium",
                vec![
                    mod_version("sodium", "sodium-3", "1.21.1", &[Fabric]),
                    mod_version("sodium", "sodium-2", "1.21.1", &[Fabric]),
                ],
            )
            .with(
                "lithium",
                "Lithium",
                vec![
                    lithium_beta,
                    mod_version("lithium", "lithium-3", "1.21.1", &[Fabric]),
                ],
            )
            .with(
                "iris",
                "Iris",
                vec![mod_version("iris", "iris-2", "1.21.1", &[Fabric])],
            );
        source.versions_fail_for = failing
            .iter()
            .map(|project| (*project).to_owned())
            .collect();
        source
    }

    async fn check(failing: &[&str]) -> Result<ModUpdates, CheckModUpdatesError> {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        let instances = InMemoryInstanceRepository::default();
        fabric_instance(&instances, &id()).await;
        let mods_dir = paths.game_dir(&id()).join(MODS_DIR);
        std::fs::create_dir_all(&mods_dir).unwrap();
        let registry = InMemoryModRegistryStore::default();
        let mut records = Vec::new();
        for (project, version) in [
            ("sodium", "sodium-2"),
            ("lithium", "lithium-4b"),
            ("iris", "iris-2"),
            ("gone", "gone-1"),
        ] {
            std::fs::write(mods_dir.join(format!("{version}.jar")), mod_jar(version)).unwrap();
            records.push(installed(project, version));
        }
        registry
            .save(&id(), &ModRegistry::new(records))
            .await
            .unwrap();
        let source = catalog(failing);
        let deps = CheckModUpdatesDeps {
            instances: &instances,
            source: &source,
            files: &FakePackFiles::default(),
            registry: &registry,
            paths: &paths,
        };
        check_mod_updates(&deps, &id()).await
    }

    #[tokio::test]
    async fn lists_newer_versions_and_never_offers_an_older_release() {
        let checked = check(&[]).await.unwrap();
        assert!(checked.unchecked.is_empty());
        let updates = checked.updates;
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].project, "sodium");
        assert_eq!(updates[0].current, "sodium-2");
        assert_eq!(updates[0].filename, "sodium-2.jar");
        assert_eq!(updates[0].version.id, "sodium-3");
    }

    #[tokio::test]
    async fn one_mod_the_catalog_cannot_answer_for_does_not_hide_the_others() {
        let checked = check(&["lithium", "iris"]).await.unwrap();
        assert_eq!(checked.updates.len(), 1);
        // The mods that could not be checked are named, never shown as current.
        assert_eq!(checked.unchecked, ["IRIS", "LITHIUM"]);
        assert!(matches!(
            check(&["sodium", "lithium", "iris", "gone"]).await,
            Err(CheckModUpdatesError::Source(ModpackSourceError::Network(_)))
        ));
    }
}
