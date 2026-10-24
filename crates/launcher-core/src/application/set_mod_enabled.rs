//! Turns one mod jar in an instance's `mods/` folder on or off, by renaming
//! it to or from `<name>.disabled`, the way other launchers do.

use thiserror::Error;

use crate::application::install_mod::MODS_DIR;
use crate::application::mod_inventory::{checked_jar, set_jar_enabled};
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::integrity::HashAlgorithm;
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::DISABLED_SUFFIX;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SetModEnabledError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error("{0:?} is not the name of a mod jar")]
    NotAMod(String),
    #[error("mods/{0} was not found")]
    NotFound(String),
}

/// Everything turning a mod on or off talks to.
pub struct SetModEnabledDeps<'a, I, P> {
    pub instances: &'a I,
    pub files: &'a P,
    pub paths: &'a LauncherPaths,
}

/// Enables or disables `filename` (its enabled name, `x.jar`). Asking for
/// the state it is already in changes nothing.
pub async fn set_mod_enabled<I: InstanceRepository, P: PackFiles>(
    deps: &SetModEnabledDeps<'_, I, P>,
    id: &InstanceId,
    filename: &str,
    enabled: bool,
) -> Result<(), SetModEnabledError> {
    deps.instances.get(id).await?;
    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    let path =
        checked_jar(filename).ok_or_else(|| SetModEnabledError::NotAMod(filename.to_owned()))?;
    let enabled_jar = mods_dir.join(path.as_str());
    let disabled_jar = mods_dir.join(format!("{filename}{DISABLED_SUFFIX}"));
    let mut found = false;
    for jar in [enabled_jar, disabled_jar] {
        found |= deps
            .files
            .digest(&jar, HashAlgorithm::Sha512)
            .await?
            .is_some();
    }
    if !found {
        return Err(SetModEnabledError::NotFound(filename.to_owned()));
    }
    set_jar_enabled(deps.files, &mods_dir, filename, enabled).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::testing::{FakePackFiles, InMemoryInstanceRepository, fabric_instance};

    struct World {
        _dir: tempfile::TempDir,
        paths: LauncherPaths,
        instances: InMemoryInstanceRepository,
        files: FakePackFiles,
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let instances = InMemoryInstanceRepository::default();
        fabric_instance(&instances, &id()).await;
        World {
            paths: LauncherPaths::new(dir.path()),
            _dir: dir,
            instances,
            files: FakePackFiles::default(),
        }
    }

    async fn set(world: &World, filename: &str, enabled: bool) -> Result<(), SetModEnabledError> {
        let deps = SetModEnabledDeps {
            instances: &world.instances,
            files: &world.files,
            paths: &world.paths,
        };
        set_mod_enabled(&deps, &id(), filename, enabled).await
    }

    fn mods_dir(world: &World) -> std::path::PathBuf {
        world.paths.game_dir(&id()).join(MODS_DIR)
    }

    #[tokio::test]
    async fn disables_and_enables_a_jar_by_renaming_it() {
        let world = world().await;
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        std::fs::write(mods_dir(&world).join("sodium.jar"), b"sodium").unwrap();

        set(&world, "sodium.jar", false).await.unwrap();
        assert!(!mods_dir(&world).join("sodium.jar").exists());
        assert!(mods_dir(&world).join("sodium.jar.disabled").exists());
        // Already disabled.
        set(&world, "sodium.jar", false).await.unwrap();

        set(&world, "sodium.jar", true).await.unwrap();
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium.jar")).unwrap(),
            b"sodium"
        );
        assert!(!mods_dir(&world).join("sodium.jar.disabled").exists());
    }

    #[tokio::test]
    async fn refuses_to_replace_another_copy_under_the_target_name() {
        let world = world().await;
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        std::fs::write(mods_dir(&world).join("sodium.jar"), b"new").unwrap();
        std::fs::write(mods_dir(&world).join("sodium.jar.disabled"), b"old").unwrap();
        assert!(matches!(
            set(&world, "sodium.jar", false).await,
            Err(SetModEnabledError::Files(_))
        ));
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium.jar.disabled")).unwrap(),
            b"old"
        );
    }

    #[tokio::test]
    async fn refuses_names_that_are_not_mod_jars_and_missing_jars() {
        let world = world().await;
        for name in [
            "../escape.jar",
            "sub/a.jar",
            "a.jar.disabled",
            "notes.txt",
            "",
        ] {
            assert_eq!(
                set(&world, name, false).await,
                Err(SetModEnabledError::NotAMod(name.to_owned())),
                "{name:?}"
            );
        }
        assert_eq!(
            set(&world, "ghost.jar", true).await,
            Err(SetModEnabledError::NotFound("ghost.jar".into()))
        );
        let other = InstanceId::parse("other").unwrap();
        let deps = SetModEnabledDeps {
            instances: &world.instances,
            files: &world.files,
            paths: &world.paths,
        };
        assert!(matches!(
            set_mod_enabled(&deps, &other, "a.jar", true).await,
            Err(SetModEnabledError::Instance(_))
        ));
    }
}
