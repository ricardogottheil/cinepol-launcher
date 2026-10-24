//! Shows what installing a mod would bring in, before the player confirms:
//! the version picked and the required dependencies that come with it, or
//! why it cannot be installed. Nothing is written.

use thiserror::Error;

use crate::application::install_mod::MODS_DIR;
use crate::application::mod_inventory::read_mod_inventory;
use crate::application::plan_mod_install::plan_mod_install;
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::{ModInstallPlan, ModPlanError, ModRegistryStore, ModSource, ModTarget};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreviewModInstallError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error(transparent)]
    Plan(#[from] ModPlanError),
}

/// Everything previewing an install talks to.
pub struct PreviewModInstallDeps<'a, I, S, P, M> {
    pub instances: &'a I,
    pub source: &'a S,
    pub files: &'a P,
    pub registry: &'a M,
    pub paths: &'a LauncherPaths,
}

/// The plan [`install_mod`](crate::application::install_mod::install_mod)
/// would follow for `project`, against the mods the instance has now.
pub async fn preview_mod_install<I, S, P, M>(
    deps: &PreviewModInstallDeps<'_, I, S, P, M>,
    id: &InstanceId,
    project: &str,
) -> Result<ModInstallPlan, PreviewModInstallError>
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
    Ok(plan_mod_install(deps.source, &target, &inventory.registry, project).await?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::domain::loader::LoaderKind::Fabric;
    use crate::domain::mods::DependencyKind::Required;
    use crate::testing::{
        FakeModSource, FakePackFiles, InMemoryInstanceRepository, InMemoryModRegistryStore,
        depends, fabric_instance, mod_jar, mod_version,
    };

    #[tokio::test]
    async fn lists_the_dependencies_still_missing_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        let instances = InMemoryInstanceRepository::default();
        let id = InstanceId::parse("survival").unwrap();
        fabric_instance(&instances, &id).await;
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris",
                vec![depends(
                    depends(
                        mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                        Required,
                        Some("sodium"),
                        None,
                    ),
                    Required,
                    Some("indium"),
                    None,
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version("sodium", "sodium-2", "1.21.1", &[Fabric])],
            )
            .with(
                "indium",
                "Indium",
                vec![mod_version("indium", "indium-1", "1.21.1", &[Fabric])],
            );
        // Sodium is already there, put in by hand.
        let mods_dir = paths.game_dir(&id).join(MODS_DIR);
        std::fs::create_dir_all(&mods_dir).unwrap();
        std::fs::write(mods_dir.join("sodium.jar"), mod_jar("sodium-2")).unwrap();
        let registry = InMemoryModRegistryStore::default();
        let deps = PreviewModInstallDeps {
            instances: &instances,
            source: &source,
            files: &FakePackFiles::default(),
            registry: &registry,
            paths: &paths,
        };
        let plan = preview_mod_install(&deps, &id, "iris").await.unwrap();
        let titles: Vec<&str> = plan
            .mods
            .iter()
            .map(|planned| planned.title.as_str())
            .collect();
        assert_eq!(titles, ["Iris", "Indium"]);
        assert_eq!(plan.present, [("sodium".to_owned(), "iris".to_owned())]);
        assert!(registry.registries.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&mods_dir).unwrap().count(), 1);

        assert_eq!(
            preview_mod_install(&deps, &id, "sodium").await,
            Err(PreviewModInstallError::Plan(
                ModPlanError::AlreadyInstalled {
                    title: "Sodium".into()
                }
            ))
        );
    }
}
