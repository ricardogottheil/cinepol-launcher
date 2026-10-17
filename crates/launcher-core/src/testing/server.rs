//! Fakes for the CINEPOL server ports: config source, status probe, settings.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::server::{
    HttpsUrl, ServerConfig, ServerConfigError, ServerConfigSource, ServerStatus, ServerStatusError,
    ServerStatusProbe,
};
use crate::domain::server_list::ServerAddress;
use crate::domain::settings::{AppSettings, SettingsError, SettingsRepository};

/// Serves configs by URL; unknown URLs are unavailable (offline).
#[derive(Debug, Default)]
pub struct FakeServerConfigSource {
    configs: HashMap<String, Result<ServerConfig, ServerConfigError>>,
}

impl FakeServerConfigSource {
    pub fn serving(url: &str, config: Result<ServerConfig, ServerConfigError>) -> Self {
        Self {
            configs: HashMap::from([(url.to_owned(), config)]),
        }
    }
}

impl ServerConfigSource for FakeServerConfigSource {
    async fn fetch(&self, url: &HttpsUrl) -> Result<ServerConfig, ServerConfigError> {
        self.configs
            .get(url.as_str())
            .cloned()
            .unwrap_or_else(|| Err(ServerConfigError::Unavailable(format!("{url} is offline"))))
    }
}

/// Answers every ping with the same result and records the addresses pinged.
#[derive(Debug)]
pub struct FakeServerStatusProbe {
    pub answer: Result<ServerStatus, ServerStatusError>,
    pinged: Mutex<Vec<String>>,
}

impl FakeServerStatusProbe {
    pub fn answering(answer: Result<ServerStatus, ServerStatusError>) -> Self {
        Self {
            answer,
            pinged: Mutex::default(),
        }
    }

    pub fn pinged(&self) -> Vec<String> {
        self.pinged.lock().unwrap().clone()
    }
}

impl ServerStatusProbe for FakeServerStatusProbe {
    async fn status(&self, address: &ServerAddress) -> Result<ServerStatus, ServerStatusError> {
        self.pinged.lock().unwrap().push(address.to_string());
        self.answer.clone()
    }
}

#[derive(Debug, Default)]
pub struct InMemorySettingsRepository {
    settings: Mutex<Option<AppSettings>>,
}

impl SettingsRepository for InMemorySettingsRepository {
    async fn load(&self) -> Result<AppSettings, SettingsError> {
        Ok(self.settings.lock().unwrap().clone().unwrap_or_default())
    }

    async fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        *self.settings.lock().unwrap() = Some(settings.clone());
        Ok(())
    }
}
