//! Changes the URL the launcher reads the CINEPOL server config from.

use thiserror::Error;

use crate::domain::server::{HttpsUrl, ServerConfigInvalid};
use crate::domain::settings::{AppSettings, SettingsError, SettingsRepository};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateSettingsError {
    #[error(transparent)]
    InvalidUrl(#[from] ServerConfigInvalid),
    #[error(transparent)]
    Storage(#[from] SettingsError),
}

/// `None` restores the default URL.
pub async fn set_server_config_url<S: SettingsRepository>(
    raw_url: Option<&str>,
    settings: &S,
) -> Result<AppSettings, UpdateSettingsError> {
    let mut current = settings.load().await?;
    current.server_config_url = match raw_url {
        Some(raw) => HttpsUrl::parse(raw.trim())?,
        None => AppSettings::default().server_config_url,
    };
    settings.save(&current).await?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::DEFAULT_SERVER_CONFIG_URL;
    use crate::testing::InMemorySettingsRepository;

    #[tokio::test]
    async fn saves_https_urls_rejects_others_and_resets_to_the_default() {
        let repo = InMemorySettingsRepository::default();
        let saved = set_server_config_url(Some(" https://example.org/c.json "), &repo)
            .await
            .unwrap();
        assert_eq!(
            saved.server_config_url.as_str(),
            "https://example.org/c.json"
        );
        assert!(matches!(
            set_server_config_url(Some("http://example.org/c.json"), &repo).await,
            Err(UpdateSettingsError::InvalidUrl(_))
        ));
        assert_eq!(
            repo.load().await.unwrap(),
            saved,
            "a rejected URL changes nothing"
        );
        let reset = set_server_config_url(None, &repo).await.unwrap();
        assert_eq!(reset.server_config_url.as_str(), DEFAULT_SERVER_CONFIG_URL);
    }
}
