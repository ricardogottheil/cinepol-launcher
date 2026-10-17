//! Reads the launcher settings (defaults when none were saved).

use crate::domain::settings::{AppSettings, SettingsError, SettingsRepository};

pub async fn get_settings<S: SettingsRepository>(
    settings: &S,
) -> Result<AppSettings, SettingsError> {
    settings.load().await
}
