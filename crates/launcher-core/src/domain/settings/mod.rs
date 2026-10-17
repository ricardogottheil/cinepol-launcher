//! Launcher-wide settings the player can change.

use std::future::Future;

use thiserror::Error;

use crate::domain::server::HttpsUrl;

/// Where the launcher reads the CINEPOL server configuration by default.
pub const DEFAULT_SERVER_CONFIG_URL: &str = "https://cinepol.policarpiers.com/server-config.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub server_config_url: HttpsUrl,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            server_config_url: HttpsUrl::from_trusted(DEFAULT_SERVER_CONFIG_URL),
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("settings storage failed: {0}")]
    Storage(String),
}

/// Persists `AppSettings`; a missing file means the defaults.
pub trait SettingsRepository: Send + Sync {
    fn load(&self) -> impl Future<Output = Result<AppSettings, SettingsError>> + Send;
    fn save(
        &self,
        settings: &AppSettings,
    ) -> impl Future<Output = Result<(), SettingsError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_point_at_the_cinepol_website() {
        assert_eq!(
            AppSettings::default().server_config_url.as_str(),
            DEFAULT_SERVER_CONFIG_URL
        );
    }
}
