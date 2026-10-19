use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

use crate::domain::ports::{ProgressReporter, ProgressUpdate};
use crate::domain::update::{AppUpdate, AppUpdateError, AppUpdater};

type Hook = Box<dyn Fn() + Send + Sync>;

/// An update source announcing `available`; records downloads and installs.
#[derive(Default)]
pub struct FakeAppUpdater {
    pub configured: bool,
    pub available: Option<AppUpdate>,
    pub check_error: Option<AppUpdateError>,
    pub download_error: Option<AppUpdateError>,
    pub apply_error: Option<AppUpdateError>,
    /// Runs while downloading (for example, a game starts meanwhile).
    pub during_download: Option<Hook>,
    pub(crate) downloaded: Mutex<Vec<String>>,
    pub(crate) applied: Mutex<usize>,
}

impl FakeAppUpdater {
    pub fn offering(version: &str) -> Self {
        Self {
            configured: true,
            available: Some(update(version)),
            ..Self::default()
        }
    }

    pub fn downloaded(&self) -> Vec<String> {
        self.downloaded.lock().unwrap().clone()
    }

    pub fn applied(&self) -> usize {
        *self.applied.lock().unwrap()
    }
}

pub fn update(version: &str) -> AppUpdate {
    AppUpdate {
        version: version.into(),
        current_version: "0.1.0".into(),
        notes: Some(format!("Release {version}")),
    }
}

impl AppUpdater for FakeAppUpdater {
    fn is_configured(&self) -> bool {
        self.configured
    }

    async fn check(&self) -> Result<Option<AppUpdate>, AppUpdateError> {
        match &self.check_error {
            Some(error) => Err(error.clone()),
            None => Ok(self.available.clone()),
        }
    }

    async fn download<R: ProgressReporter>(
        &self,
        version: &str,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<(), AppUpdateError> {
        if cancel.is_cancelled() {
            return Err(AppUpdateError::Cancelled);
        }
        if let Some(error) = &self.download_error {
            return Err(error.clone());
        }
        if self.available.as_ref().map(|u| u.version.as_str()) != Some(version) {
            return Err(AppUpdateError::Changed(version.to_owned()));
        }
        reporter.report(ProgressUpdate::bytes(10, Some(10)));
        if let Some(hook) = &self.during_download {
            hook();
        }
        self.downloaded.lock().unwrap().push(version.to_owned());
        Ok(())
    }

    async fn apply(&self) -> Result<(), AppUpdateError> {
        if let Some(error) = &self.apply_error {
            return Err(error.clone());
        }
        *self.applied.lock().unwrap() += 1;
        Ok(())
    }
}
