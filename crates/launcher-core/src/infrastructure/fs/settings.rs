//! `settings.json`: launcher settings. Unknown or invalid values fall back to
//! the defaults instead of blocking the launcher.

use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::write_atomic;
use crate::domain::server::HttpsUrl;
use crate::domain::settings::{AppSettings, SettingsError, SettingsRepository};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    server_config_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JsonSettingsRepository {
    file: PathBuf,
}

impl JsonSettingsRepository {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }
}

impl SettingsRepository for JsonSettingsRepository {
    async fn load(&self) -> Result<AppSettings, SettingsError> {
        let file: SettingsFile = match tokio::fs::read(&self.file).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, "settings.json is unreadable; using the defaults");
                SettingsFile::default()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => SettingsFile::default(),
            Err(e) => return Err(SettingsError::Storage(e.to_string())),
        };
        let defaults = AppSettings::default();
        let server_config_url = file
            .server_config_url
            .and_then(|raw| HttpsUrl::parse(&raw).ok())
            .unwrap_or(defaults.server_config_url);
        Ok(AppSettings { server_config_url })
    }

    async fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        let file = SettingsFile {
            schema_version: SCHEMA_VERSION,
            server_config_url: Some(settings.server_config_url.to_string()),
        };
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|e| SettingsError::Storage(e.to_string()))?;
        write_atomic(&self.file, &bytes)
            .await
            .map_err(|e| SettingsError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_unreadable_or_invalid_files_mean_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let repo = JsonSettingsRepository::new(&file);
        assert_eq!(repo.load().await.unwrap(), AppSettings::default());
        std::fs::write(&file, b"{ broken").unwrap();
        assert_eq!(repo.load().await.unwrap(), AppSettings::default());
        std::fs::write(
            &file,
            br#"{"schemaVersion":1,"serverConfigUrl":"http://insecure.example/config.json"}"#,
        )
        .unwrap();
        assert_eq!(repo.load().await.unwrap(), AppSettings::default());
    }

    #[tokio::test]
    async fn round_trips_a_custom_config_url() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonSettingsRepository::new(dir.path().join("settings.json"));
        let settings = AppSettings {
            server_config_url: HttpsUrl::parse("https://example.org/cinepol.json").unwrap(),
        };
        repo.save(&settings).await.unwrap();
        assert_eq!(repo.load().await.unwrap(), settings);
    }
}
