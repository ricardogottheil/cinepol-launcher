//! Duplicates an instance: a copy of its whole folder (mods, configs, worlds,
//! server list, modpack manifest) under a new name and id.
//!
//! A copy of the CINEPOL instance is an ordinary custom instance, so Play on
//! CINEPOL keeps managing only the original. A modpack copy keeps its origin
//! and manifest, so it can still be updated. The copy starts with no playtime.

use thiserror::Error;

use crate::application::create_instance::free_id;
use crate::domain::instance::{
    Instance, InstanceId, InstanceName, InstanceNameError, InstanceOrigin, InstanceRepository,
    InstanceRepositoryError,
};
use crate::domain::launch::LauncherPaths;
use crate::domain::ports::{FileSystem, FileSystemError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DuplicateInstanceError {
    #[error(transparent)]
    InvalidName(#[from] InstanceNameError),
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error("could not copy the instance folder: {0}")]
    Copy(#[from] FileSystemError),
}

pub async fn duplicate_instance<R: InstanceRepository, F: FileSystem>(
    source: &InstanceId,
    name: &str,
    instances: &R,
    fs: &F,
    paths: &LauncherPaths,
) -> Result<Instance, DuplicateInstanceError> {
    let original = instances.get(source).await?;
    let name = InstanceName::parse(name)?;
    let id = free_id(&name, instances).await?;
    let target = paths.instance_dir(&id);
    fs.copy_dir_all(&paths.instance_dir(source), &target)
        .await?;
    let copy = Instance {
        id,
        name,
        origin: match original.origin {
            InstanceOrigin::CinepolServer => InstanceOrigin::Custom,
            origin => origin,
        },
        playtime_secs: 0,
        ..original
    };
    // The copied instance.json still names the original; saving replaces it.
    if let Err(error) = instances.save(&copy).await {
        if let Err(cleanup) = fs.remove_dir_all(&target).await {
            tracing::warn!(instance = %copy.id, %cleanup, "could not remove a failed copy");
        }
        return Err(error.into());
    }
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::AccountId;
    use crate::domain::instance::{GameVersion, MemorySettings};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::domain::modpack::{ModpackRef, ModpackSourceKind};
    use crate::testing::{FakeFileSystem, InMemoryInstanceRepository};

    fn source(origin: InstanceOrigin) -> Instance {
        let mut instance = Instance::new(
            InstanceId::parse("survival").unwrap(),
            InstanceName::parse("Survival").unwrap(),
            GameVersion::parse("26.2").unwrap(),
            LoaderSpec::new(
                LoaderKind::Fabric,
                Some(LoaderVersion::parse("0.19.5").unwrap()),
            )
            .unwrap(),
            origin,
        );
        instance.settings.memory = MemorySettings::new(1024, 8192).unwrap();
        instance.default_account = Some(AccountId::new_random());
        instance.playtime_secs = 3_600;
        instance
    }

    struct World {
        instances: InMemoryInstanceRepository,
        fs: FakeFileSystem,
        paths: LauncherPaths,
    }

    async fn world(origin: InstanceOrigin) -> World {
        let paths = LauncherPaths::new("/data");
        let instances = InMemoryInstanceRepository::default();
        let original = source(origin);
        instances.save(&original).await.unwrap();
        let fs = FakeFileSystem::with([paths
            .instance_dir(&original.id)
            .join("minecraft/saves/World/level.dat")]);
        World {
            instances,
            fs,
            paths,
        }
    }

    async fn duplicate(world: &World, name: &str) -> Result<Instance, DuplicateInstanceError> {
        duplicate_instance(
            &InstanceId::parse("survival").unwrap(),
            name,
            &world.instances,
            &world.fs,
            &world.paths,
        )
        .await
    }

    #[tokio::test]
    async fn copies_the_folder_and_settings_under_a_new_name() {
        let world = world(InstanceOrigin::Custom).await;
        let original = world
            .instances
            .get(&InstanceId::parse("survival").unwrap())
            .await
            .unwrap();
        let copy = duplicate(&world, "Survival (copy)").await.unwrap();

        assert_eq!(copy.id.as_str(), "survival-copy");
        assert_eq!(copy.name.as_str(), "Survival (copy)");
        assert_eq!(copy.game_version, original.game_version);
        assert_eq!(copy.loader, original.loader);
        assert_eq!(copy.settings, original.settings);
        assert_eq!(copy.default_account, original.default_account);
        assert_eq!(copy.origin, InstanceOrigin::Custom);
        assert_eq!(copy.playtime_secs, 0);
        assert_eq!(world.instances.get(&copy.id).await.unwrap(), copy);
        assert_eq!(
            world.fs.copies(),
            [(
                world.paths.instance_dir(&original.id),
                world.paths.instance_dir(&copy.id)
            )]
        );
        assert!(
            world.fs.contains(
                &world
                    .paths
                    .instance_dir(&copy.id)
                    .join("minecraft/saves/World/level.dat")
            )
        );
        // The original is untouched.
        assert_eq!(world.instances.get(&original.id).await.unwrap(), original);
    }

    #[tokio::test]
    async fn a_copy_of_the_cinepol_instance_is_a_custom_instance() {
        let world = world(InstanceOrigin::CinepolServer).await;
        let copy = duplicate(&world, "CINEPOL (copy)").await.unwrap();
        assert_eq!(copy.origin, InstanceOrigin::Custom);
    }

    #[tokio::test]
    async fn a_modpack_copy_keeps_its_origin() {
        let origin = InstanceOrigin::Modpack(ModpackRef {
            source: ModpackSourceKind::Modrinth,
            project: "1KVo5zza".into(),
            version: "uQxD0ENB".into(),
        });
        let world = world(origin.clone()).await;
        let copy = duplicate(&world, "FO (copy)").await.unwrap();
        assert_eq!(copy.origin, origin);
    }

    #[tokio::test]
    async fn never_copies_into_a_taken_folder() {
        let world = world(InstanceOrigin::Custom).await;
        world
            .instances
            .occupy(InstanceId::parse("survival-copy").unwrap());
        let copy = duplicate(&world, "Survival (copy)").await.unwrap();
        assert_eq!(copy.id.as_str(), "survival-copy-2");
    }

    #[tokio::test]
    async fn rejects_a_blank_name_and_an_unknown_source() {
        let world = world(InstanceOrigin::Custom).await;
        assert!(matches!(
            duplicate(&world, "  ").await,
            Err(DuplicateInstanceError::InvalidName(_))
        ));
        let missing = duplicate_instance(
            &InstanceId::parse("missing").unwrap(),
            "Copy",
            &world.instances,
            &world.fs,
            &world.paths,
        )
        .await;
        assert!(matches!(
            missing,
            Err(DuplicateInstanceError::Instance(
                InstanceRepositoryError::NotFound(_)
            ))
        ));
        assert!(world.fs.copies().is_empty());
    }
}
