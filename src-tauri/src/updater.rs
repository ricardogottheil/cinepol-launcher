//! `AppUpdater` over `tauri-plugin-updater`. The endpoint lives in
//! `tauri.conf.json`; the minisign public key is injected at build time
//! (`CINEPOL_UPDATER_PUBKEY`), so builds without it report updates as not
//! configured instead of failing.

use std::sync::Mutex;

use launcher_core::domain::ports::{ProgressReporter, ProgressUpdate};
use launcher_core::domain::update::{AppUpdate, AppUpdateError, AppUpdater};
use tauri::{AppHandle, Wry};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use tokio_util::sync::CancellationToken;

/// The public half of the key that signs releases; empty in development builds.
pub const PUBKEY: &str = match option_env!("CINEPOL_UPDATER_PUBKEY") {
    Some(key) => key,
    None => "",
};

pub struct TauriAppUpdater {
    app: AppHandle<Wry>,
    /// The verified download waiting for `apply`.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
}

impl TauriAppUpdater {
    pub fn new(app: AppHandle<Wry>) -> Self {
        Self {
            app,
            ready: Mutex::new(None),
        }
    }

    fn updater(&self) -> Result<Updater, AppUpdateError> {
        self.app
            .updater_builder()
            .pubkey(PUBKEY)
            .build()
            .map_err(|error| AppUpdateError::Check(error.to_string()))
    }

    async fn find(&self) -> Result<Option<Update>, AppUpdateError> {
        self.updater()?
            .check()
            .await
            .map_err(|error| AppUpdateError::Check(error.to_string()))
    }
}

impl AppUpdater for TauriAppUpdater {
    fn is_configured(&self) -> bool {
        !PUBKEY.trim().is_empty()
    }

    async fn check(&self) -> Result<Option<AppUpdate>, AppUpdateError> {
        Ok(self.find().await?.map(|update| AppUpdate {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
        }))
    }

    async fn download<R: ProgressReporter>(
        &self,
        version: &str,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<(), AppUpdateError> {
        let update = match self.find().await? {
            Some(update) if update.version == version => update,
            _ => return Err(AppUpdateError::Changed(version.to_owned())),
        };
        let mut done = 0u64;
        // `download` verifies the signature before it returns the bytes.
        let fetch = update.download(
            |chunk, total| {
                done += chunk as u64;
                reporter.report(ProgressUpdate::bytes(done, total));
            },
            || {},
        );
        let bytes = cancel
            .run_until_cancelled(fetch)
            .await
            .ok_or(AppUpdateError::Cancelled)?
            .map_err(|error| AppUpdateError::Download(error.to_string()))?;
        *self
            .ready
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((update, bytes));
        Ok(())
    }

    async fn apply(&self) -> Result<(), AppUpdateError> {
        let ready = self
            .ready
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some((update, bytes)) = ready else {
            return Err(AppUpdateError::Install("nothing was downloaded".into()));
        };
        // On Windows this starts the installer and exits the launcher.
        update
            .install(bytes)
            .map_err(|error| AppUpdateError::Install(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    fn updater_config() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let config: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        config["plugins"]["updater"].clone()
    }

    #[test]
    fn signatures_must_carry_the_announced_version() {
        // Without it, an edited latest.json could offer an older signed release.
        assert_eq!(updater_config()["requireSignedVersion"], true);
        assert_ne!(updater_config()["allowDowngrades"], true);
    }

    #[test]
    fn updates_come_only_from_the_releases_repository_over_https() {
        let endpoints = updater_config()["endpoints"].clone();
        assert_eq!(
            endpoints,
            serde_json::json!([
                "https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest/download/latest.json"
            ])
        );
        assert_ne!(updater_config()["dangerousInsecureTransportProtocol"], true);
    }

    #[test]
    fn the_repository_holds_no_public_key() {
        // The key is a build-time setting (CINEPOL_UPDATER_PUBKEY); see docs/release.md.
        assert_eq!(updater_config()["pubkey"], "");
    }
}
