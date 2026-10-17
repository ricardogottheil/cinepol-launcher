//! Where the launcher keeps shared and per-instance files.

use std::path::{Path, PathBuf};

use crate::domain::instance::InstanceId;

/// The launcher's data directory layout. The root comes from the desktop
/// shell (Tauri `app_data_dir`, `CinepolLauncher`); nothing here touches disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherPaths {
    root: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Shared Maven-layout libraries.
    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join("libraries")
    }

    /// Shared assets: `indexes/`, `objects/`, `log_configs/`.
    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// Version JSONs and client jars: `versions/<id>/<id>.json|.jar`.
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_json(&self, version_id: &str) -> PathBuf {
        self.versions_dir()
            .join(version_id)
            .join(format!("{version_id}.json"))
    }

    pub fn client_jar(&self, version_id: &str) -> PathBuf {
        self.versions_dir()
            .join(version_id)
            .join(format!("{version_id}.jar"))
    }

    /// Managed Java runtimes.
    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn instance_dir(&self, id: &InstanceId) -> PathBuf {
        self.instances_dir().join(id.as_str())
    }

    /// The `.minecraft` of an instance (saves, mods, config, servers.dat).
    pub fn game_dir(&self, id: &InstanceId) -> PathBuf {
        self.instance_dir(id).join("minecraft")
    }

    /// Extracted natives, recreated on every launch.
    pub fn natives_dir(&self, id: &InstanceId) -> PathBuf {
        self.instance_dir(id).join("natives")
    }

    /// Account metadata (never tokens).
    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    /// Scratch space on the same volume as the data, so renames are atomic.
    pub fn temp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_shared_and_instance_directories() {
        let paths = LauncherPaths::new(PathBuf::from("data"));
        let id = InstanceId::parse("cinepol").unwrap();
        assert_eq!(
            paths.client_jar("26.2"),
            Path::new("data")
                .join("versions")
                .join("26.2")
                .join("26.2.jar")
        );
        assert_eq!(
            paths.game_dir(&id),
            Path::new("data")
                .join("instances")
                .join("cinepol")
                .join("minecraft")
        );
        assert_eq!(
            paths.natives_dir(&id),
            Path::new("data")
                .join("instances")
                .join("cinepol")
                .join("natives")
        );
    }
}
