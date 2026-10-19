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
