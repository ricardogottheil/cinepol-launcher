//! Where the launcher keeps shared and per-instance files.

use std::path::{Path, PathBuf};

use super::RelativePath;
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

    /// `versions/<id>/<id>.json`; `None` unless `version_id` is one safe path segment.
    pub fn version_json(&self, version_id: &str) -> Option<PathBuf> {
        self.version_file(version_id, "json")
    }

    /// `versions/<id>/<id>.jar`; `None` unless `version_id` is one safe path segment.
    pub fn client_jar(&self, version_id: &str) -> Option<PathBuf> {
        self.version_file(version_id, "jar")
    }

    fn version_file(&self, version_id: &str, extension: &str) -> Option<PathBuf> {
        // Ids also name a directory, so they must not traverse or carry separators,
        // drive prefixes or NUL, whatever layer validated them before.
        let segment = RelativePath::parse(version_id)
            .ok()
            .filter(|_| !version_id.contains('/'))?;
        Some(
            self.versions_dir()
                .join(segment.as_str())
                .join(format!("{version_id}.{extension}")),
        )
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

    /// Launcher settings the player can change.
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Account metadata (never tokens).
    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    /// Servers the player added next to CINEPOL.
    pub fn saved_servers_file(&self) -> PathBuf {
        self.root.join("servers.json")
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
            Some(
                Path::new("data")
                    .join("versions")
                    .join("26.2")
                    .join("26.2.jar")
            )
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

    #[test]
    fn version_paths_refuse_ids_that_are_not_a_single_safe_segment() {
        let paths = LauncherPaths::new(PathBuf::from("data"));
        for id in ["", ".", "..", "../evil", "a/b", "a\\b", "C:x", "nul\0"] {
            assert_eq!(paths.version_json(id), None, "{id:?}");
            assert_eq!(paths.client_jar(id), None, "{id:?}");
        }
        assert_eq!(
            paths.version_json("fabric-loader-0.17.2-26.2"),
            Some(
                Path::new("data")
                    .join("versions")
                    .join("fabric-loader-0.17.2-26.2")
                    .join("fabric-loader-0.17.2-26.2.json")
            )
        );
    }
}
