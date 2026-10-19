//! Instances: isolated game directories with their own version, loader,
//! Java selection and launch settings.

mod names;
mod ports;
mod settings;

pub use names::{InstanceId, InstanceIdError, InstanceName, InstanceNameError};
pub use ports::{InstanceRepository, InstanceRepositoryError};
pub use settings::{
    EnvVars, GameVersion, GameVersionError, JavaSelection, JvmArgs, MAX_JVM_ARGS, MAX_MEMORY_MB,
    MIN_MEMORY_MB, MemorySettings, SettingsError, WindowSize,
};

use crate::domain::account::AccountId;
use crate::domain::loader::LoaderSpec;
use crate::domain::modpack::ModpackRef;

/// Who keeps an instance in sync besides the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceOrigin {
    /// Created by the player from a version and a loader.
    Custom,
    /// The "Play on CINEPOL" instance; the launcher moves it to the recommended version.
    CinepolServer,
    /// Installed from a modpack, which may be updated later.
    Modpack(ModpackRef),
}

/// Launch settings an instance may override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchSettings {
    pub java: JavaSelection,
    pub memory: MemorySettings,
    pub jvm_args: JvmArgs,
    /// `None` uses the game's default window.
    pub window: Option<WindowSize>,
    pub env: EnvVars,
    /// packwiz instances: sync the pack from its source before every launch.
    pub update_pack_on_launch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub id: InstanceId,
    pub name: InstanceName,
    pub game_version: GameVersion,
    pub loader: LoaderSpec,
    pub origin: InstanceOrigin,
    pub settings: LaunchSettings,
    /// Account to launch with when the player does not pick one.
    pub default_account: Option<AccountId>,
    /// Total time played, in seconds.
    pub playtime_secs: u64,
}

impl Instance {
    /// A new instance with default launch settings and no playtime.
    pub fn new(
        id: InstanceId,
        name: InstanceName,
        game_version: GameVersion,
        loader: LoaderSpec,
        origin: InstanceOrigin,
    ) -> Self {
        Self {
            id,
            name,
            game_version,
            loader,
            origin,
            settings: LaunchSettings::default(),
            default_account: None,
            playtime_secs: 0,
        }
    }

    /// Adds a finished session to the playtime, saturating instead of overflowing.
    pub fn record_playtime(&mut self, session_secs: u64) {
        self.playtime_secs = self.playtime_secs.saturating_add(session_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_instances_start_with_defaults_and_accumulate_playtime() {
        let mut instance = Instance::new(
            InstanceId::parse("cinepol").unwrap(),
            InstanceName::parse("CINEPOL").unwrap(),
            GameVersion::parse("26.2").unwrap(),
            LoaderSpec::vanilla(),
            InstanceOrigin::CinepolServer,
        );
        assert_eq!(instance.settings, LaunchSettings::default());
        assert_eq!(instance.settings.memory.max_mb(), 4096);
        instance.record_playtime(90);
        instance.record_playtime(u64::MAX);
        assert_eq!(instance.playtime_secs, u64::MAX);
    }
}
