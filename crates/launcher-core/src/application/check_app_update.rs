//! Checks whether a newer launcher release is available.

use crate::domain::update::{AppUpdateError, AppUpdater, UpdateStatus};

pub async fn check_app_update<U: AppUpdater>(updater: &U) -> Result<UpdateStatus, AppUpdateError> {
    if !updater.is_configured() {
        return Ok(UpdateStatus::NotConfigured);
    }
    Ok(match updater.check().await? {
        Some(update) => UpdateStatus::Available(update),
        None => UpdateStatus::UpToDate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeAppUpdater, update};

    #[tokio::test]
    async fn reports_an_available_update() {
        let status = check_app_update(&FakeAppUpdater::offering("0.2.0")).await;
        assert_eq!(status, Ok(UpdateStatus::Available(update("0.2.0"))));
    }

    #[tokio::test]
    async fn reports_up_to_date_when_there_is_nothing_newer() {
        let updater = FakeAppUpdater {
            configured: true,
            ..FakeAppUpdater::default()
        };
        assert_eq!(check_app_update(&updater).await, Ok(UpdateStatus::UpToDate));
    }

    #[tokio::test]
    async fn an_unconfigured_build_does_not_check() {
        let updater = FakeAppUpdater {
            check_error: Some(AppUpdateError::Check("must not be called".into())),
            ..FakeAppUpdater::default()
        };
        assert_eq!(
            check_app_update(&updater).await,
            Ok(UpdateStatus::NotConfigured)
        );
    }

    #[tokio::test]
    async fn check_failures_are_errors() {
        let updater = FakeAppUpdater {
            configured: true,
            check_error: Some(AppUpdateError::Check("offline".into())),
            ..FakeAppUpdater::default()
        };
        assert_eq!(
            check_app_update(&updater).await,
            Err(AppUpdateError::Check("offline".into()))
        );
    }
}
