//! `.cinepol-runtime.json`: written last into an install directory, so its
//! presence means the install completed. It records what was installed, so a
//! runtime is reused only while the source still offers the same build.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::java::JavaRuntimeError;

pub(super) const MARKER_FILE: &str = ".cinepol-runtime.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RuntimeMarker {
    /// `mojang` or `adoptium`.
    pub source: String,
    /// Mojang component (`java-runtime-epsilon`) or Adoptium major (`25`).
    pub id: String,
    /// Mojang version name or Adoptium release name.
    pub version: String,
    /// Mojang manifest sha1 or Adoptium package name.
    pub revision: String,
}

impl RuntimeMarker {
    /// Same source and runtime, whatever build: enough to reuse when offline.
    pub fn same_runtime(&self, other: &Self) -> bool {
        self.source == other.source && self.id == other.id
    }
}

fn marker_path(install_dir: &Path) -> PathBuf {
    install_dir.join(MARKER_FILE)
}

/// `None` when missing or unreadable (an unreadable marker means "reinstall").
pub(super) async fn read_marker(install_dir: &Path) -> Option<RuntimeMarker> {
    let bytes = tokio::fs::read(marker_path(install_dir)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(super) async fn write_marker(
    install_dir: &Path,
    marker: &RuntimeMarker,
) -> Result<(), JavaRuntimeError> {
    let path = marker_path(install_dir);
    let bytes = serde_json::to_vec_pretty(marker)
        .map_err(|error| JavaRuntimeError::Invalid(error.to_string()))?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|error| io_error(&path, &error))
}

pub(super) async fn remove_marker(install_dir: &Path) {
    // A missing marker is the goal; any other failure surfaces when it is rewritten.
    let _ = tokio::fs::remove_file(marker_path(install_dir)).await;
}

pub(super) fn io_error(path: &Path, error: &std::io::Error) -> JavaRuntimeError {
    JavaRuntimeError::Download(format!("could not write {}: {error}", path.display()))
}
