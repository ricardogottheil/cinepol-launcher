//! Changes what the player may edit on an instance: its name, default
//! account and launch settings (Java, memory, JVM arguments, window, env).
//! The game version and the loader are not editable here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::domain::account::{AccountId, AccountRepository, AccountRepositoryError};
use crate::domain::instance::{
    EnvVars, Instance, InstanceId, InstanceName, InstanceNameError, InstanceRepository,
    InstanceRepositoryError, JavaSelection, JvmArgs, LaunchSettings, MemorySettings, SettingsError,
    WindowSize,
};
use crate::domain::java::JavaProbe;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateInstanceSettingsError {
    #[error(transparent)]
    InvalidName(#[from] InstanceNameError),
    #[error(transparent)]
    InvalidSettings(#[from] SettingsError),
    #[error("{path} is not a usable Java: {reason}")]
    InvalidJava { path: String, reason: String },
    #[error("account {0:?} does not exist")]
    UnknownAccount(AccountId),
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Accounts(#[from] AccountRepositoryError),
}

/// Which Java the instance runs with, as the player chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaChoice {
    Managed,
    /// An absolute path to a `java` executable; probed before it is saved.
    Custom(PathBuf),
}

/// The complete set of editable values; every field replaces the stored one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceSettingsChange {
    pub name: String,
    pub default_account: Option<AccountId>,
    pub java: JavaChoice,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    /// `None` keeps the game's own window size.
    pub window: Option<(u32, u32)>,
    pub env: BTreeMap<String, String>,
}

pub struct UpdateInstanceSettingsDeps<'a, I, A, J> {
    pub instances: &'a I,
    pub accounts: &'a A,
    pub java: &'a J,
}

/// Validates every value before anything is saved, so a rejected change
/// leaves the instance exactly as it was.
pub async fn update_instance_settings<I, A, J>(
    id: &InstanceId,
    change: InstanceSettingsChange,
    deps: &UpdateInstanceSettingsDeps<'_, I, A, J>,
) -> Result<Instance, UpdateInstanceSettingsError>
where
    I: InstanceRepository,
    A: AccountRepository,
    J: JavaProbe,
{
    // Fails early for an unknown instance; the record is read again below.
    deps.instances.get(id).await?;
    let name = InstanceName::parse(&change.name)?;
    let memory = MemorySettings::new(change.min_memory_mb, change.max_memory_mb)?;
    let jvm_args = JvmArgs::parse(change.jvm_args)?;
    let env = EnvVars::parse(change.env)?;
    let window = change
        .window
        .map(|(width, height)| WindowSize::new(width, height))
        .transpose()?;
    let java = match change.java {
        JavaChoice::Managed => JavaSelection::Managed,
        JavaChoice::Custom(path) => {
            let invalid = |reason: String| UpdateInstanceSettingsError::InvalidJava {
                path: path.display().to_string(),
                reason,
            };
            if !path.is_absolute() {
                return Err(invalid("the path must be absolute".into()));
            }
            deps.java
                .probe(&path)
                .await
                .map_err(|error| invalid(error.to_string()))?;
            JavaSelection::Custom(path)
        }
    };
    if let Some(account) = change.default_account {
        let known = deps.accounts.list().await?;
        if !known.iter().any(|candidate| candidate.id == account) {
            return Err(UpdateInstanceSettingsError::UnknownAccount(account));
        }
    }

    // Re-read after the awaits above (the Java probe starts a process): an
    // update or a playtime save that landed meanwhile is kept, and only the
    // editable fields change.
    let mut instance = deps.instances.get(id).await?;
    instance.name = name;
    instance.default_account = change.default_account;
    instance.settings = LaunchSettings {
        java,
        memory,
        jvm_args,
        window,
        env,
    };
    deps.instances.save(&instance).await?;
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::domain::instance::{GameVersion, InstanceOrigin};
    use crate::domain::java::JavaVersion;
    use crate::domain::loader::LoaderSpec;
    use crate::testing::{FakeJavaProbe, InMemoryAccountRepository, InMemoryInstanceRepository};

    const JAVA: &str = "/opt/jdk-25/bin/java";

    struct World {
        instances: InMemoryInstanceRepository,
        accounts: InMemoryAccountRepository,
        java: FakeJavaProbe,
        account: AccountId,
    }

    async fn world() -> World {
        let account = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let instances = InMemoryInstanceRepository::default();
        instances
            .save(&Instance::new(
                id(),
                InstanceName::parse("Survival").unwrap(),
                GameVersion::parse("26.2").unwrap(),
                LoaderSpec::vanilla(),
                InstanceOrigin::Custom,
            ))
            .await
            .unwrap();
        World {
            instances,
            account: account.id,
            accounts: InMemoryAccountRepository::with(vec![account]),
            java: FakeJavaProbe::with([(
                PathBuf::from(JAVA),
                JavaVersion {
                    major: 25,
                    full: "25.0.1".into(),
                    is_64_bit: true,
                },
            )]),
        }
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn change(world: &World) -> InstanceSettingsChange {
        InstanceSettingsChange {
            name: "  Survival 2  ".into(),
            default_account: Some(world.account),
            java: JavaChoice::Custom(PathBuf::from(JAVA)),
            min_memory_mb: 2048,
            max_memory_mb: 6144,
            jvm_args: vec!["-XX:+UseZGC".into()],
            window: Some((1280, 720)),
            env: BTreeMap::from([("MESA_GL_VERSION_OVERRIDE".into(), "4.6".into())]),
        }
    }

    async fn apply(
        world: &World,
        change: InstanceSettingsChange,
    ) -> Result<Instance, UpdateInstanceSettingsError> {
        update_instance_settings(
            &id(),
            change,
            &UpdateInstanceSettingsDeps {
                instances: &world.instances,
                accounts: &world.accounts,
                java: &world.java,
            },
        )
        .await
    }

    #[tokio::test]
    async fn saves_every_editable_value() {
        let world = world().await;
        let updated = apply(&world, change(&world)).await.unwrap();

        assert_eq!(updated.name.as_str(), "Survival 2");
        assert_eq!(updated.default_account, Some(world.account));
        assert_eq!(
            updated.settings.java,
            JavaSelection::Custom(PathBuf::from(JAVA))
        );
        assert_eq!(
            updated.settings.memory,
            MemorySettings::new(2048, 6144).unwrap()
        );
        assert_eq!(updated.settings.jvm_args.as_slice(), ["-XX:+UseZGC"]);
        assert_eq!(
            updated.settings.window,
            Some(WindowSize::new(1280, 720).unwrap())
        );
        assert_eq!(
            updated.settings.env.iter().collect::<Vec<_>>(),
            [("MESA_GL_VERSION_OVERRIDE", "4.6")]
        );
        // The id, version, loader and playtime never change here.
        assert_eq!(updated.id, id());
        assert_eq!(updated.game_version.as_str(), "26.2");
        assert_eq!(world.instances.get(&id()).await.unwrap(), updated);
    }

    #[tokio::test]
    async fn managed_java_and_the_default_window_clear_the_overrides() {
        let world = world().await;
        apply(&world, change(&world)).await.unwrap();
        let updated = apply(
            &world,
            InstanceSettingsChange {
                java: JavaChoice::Managed,
                window: None,
                default_account: None,
                env: BTreeMap::new(),
                jvm_args: vec![],
                ..change(&world)
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.settings.java, JavaSelection::Managed);
        assert_eq!(updated.settings.window, None);
        assert_eq!(updated.default_account, None);
        assert!(updated.settings.jvm_args.as_slice().is_empty());
    }

    #[tokio::test]
    async fn rejected_changes_leave_the_instance_as_it_was() {
        let world = world().await;
        let before = world.instances.get(&id()).await.unwrap();
        let unknown = AccountId::new_random();
        let cases = [
            (
                InstanceSettingsChange {
                    name: " ".into(),
                    ..change(&world)
                },
                "name",
            ),
            (
                InstanceSettingsChange {
                    min_memory_mb: 8192,
                    max_memory_mb: 1024,
                    ..change(&world)
                },
                "memory",
            ),
            (
                InstanceSettingsChange {
                    jvm_args: vec!["-Xmx64G".into()],
                    ..change(&world)
                },
                "reserved jvm arg",
            ),
            (
                InstanceSettingsChange {
                    window: Some((0, 720)),
                    ..change(&world)
                },
                "window",
            ),
            (
                InstanceSettingsChange {
                    env: BTreeMap::from([("BAD=NAME".into(), "1".into())]),
                    ..change(&world)
                },
                "env",
            ),
            (
                InstanceSettingsChange {
                    java: JavaChoice::Custom(PathBuf::from("/usr/bin/not-java")),
                    ..change(&world)
                },
                "java",
            ),
            (
                InstanceSettingsChange {
                    java: JavaChoice::Custom(PathBuf::from("bin/java")),
                    ..change(&world)
                },
                "relative java",
            ),
            (
                InstanceSettingsChange {
                    default_account: Some(unknown),
                    ..change(&world)
                },
                "account",
            ),
        ];
        for (bad, label) in cases {
            assert!(apply(&world, bad).await.is_err(), "{label}");
            assert_eq!(world.instances.get(&id()).await.unwrap(), before, "{label}");
        }
        assert_eq!(
            apply(
                &world,
                InstanceSettingsChange {
                    default_account: Some(unknown),
                    ..change(&world)
                }
            )
            .await,
            Err(UpdateInstanceSettingsError::UnknownAccount(unknown))
        );
    }

    #[tokio::test]
    async fn reports_an_unknown_instance() {
        let world = world().await;
        let result = update_instance_settings(
            &InstanceId::parse("missing").unwrap(),
            change(&world),
            &UpdateInstanceSettingsDeps {
                instances: &world.instances,
                accounts: &world.accounts,
                java: &world.java,
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(UpdateInstanceSettingsError::Instance(
                InstanceRepositoryError::NotFound(_)
            ))
        ));
    }

    /// Answers like a real Java, but a modpack update lands while it runs.
    struct UpdatingProbe<'a> {
        instances: &'a InMemoryInstanceRepository,
    }

    impl JavaProbe for UpdatingProbe<'_> {
        async fn probe(
            &self,
            _executable: &std::path::Path,
        ) -> Result<JavaVersion, crate::domain::java::JavaProbeError> {
            let mut instance = self.instances.get(&id()).await.unwrap();
            instance.game_version = GameVersion::parse("26.3").unwrap();
            instance.record_playtime(60);
            self.instances.save(&instance).await.unwrap();
            Ok(JavaVersion {
                major: 25,
                full: "25.0.1".into(),
                is_64_bit: true,
            })
        }
    }

    #[tokio::test]
    async fn keeps_what_others_saved_while_the_change_was_checked() {
        let world = world().await;
        let probe = UpdatingProbe {
            instances: &world.instances,
        };
        let updated = update_instance_settings(
            &id(),
            change(&world),
            &UpdateInstanceSettingsDeps {
                instances: &world.instances,
                accounts: &world.accounts,
                java: &probe,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name.as_str(), "Survival 2");
        assert_eq!(updated.game_version.as_str(), "26.3");
        assert_eq!(updated.playtime_secs, 60);
        assert_eq!(world.instances.get(&id()).await.unwrap(), updated);
    }
}
