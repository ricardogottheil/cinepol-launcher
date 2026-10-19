//! Downloads, verifies and installs the launcher release the player accepted.
//! Installing restarts the launcher, so it never happens while a game is
//! installing or running.

use tokio_util::sync::CancellationToken;

use crate::domain::launch::RunningGames;
use crate::domain::ports::ProgressReporter;
use crate::domain::update::{AppUpdateError, AppUpdater};

pub async fn install_app_update<U: AppUpdater, G: RunningGames, R: ProgressReporter>(
    version: &str,
    updater: &U,
    games: &G,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<(), AppUpdateError> {
    if !updater.is_configured() {
        return Err(AppUpdateError::NotConfigured);
    }
    if games.any_busy() {
        return Err(AppUpdateError::GameRunning);
    }
    updater.download(version, reporter, cancel).await?;
    if cancel.is_cancelled() {
        return Err(AppUpdateError::Cancelled);
    }
    // A game may have started while the update downloaded. Freezing checks
    // that and stops new sessions in one step, until the launcher restarts.
    if !games.freeze() {
        return Err(AppUpdateError::GameRunning);
    }
    let applied = updater.apply().await;
    if applied.is_err() {
        games.unfreeze();
    }
    applied
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::instance::InstanceId;
    use crate::testing::{FakeAppUpdater, InMemoryRunningGames, RecordingReporter};

    async fn install(
        version: &str,
        updater: &FakeAppUpdater,
        games: &InMemoryRunningGames,
    ) -> Result<(), AppUpdateError> {
        install_app_update(
            version,
            updater,
            games,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    fn survival() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    #[tokio::test]
    async fn downloads_then_installs_the_accepted_version() {
        let updater = FakeAppUpdater::offering("0.2.0");
        let reporter = RecordingReporter::default();
        install_app_update(
            "0.2.0",
            &updater,
            &InMemoryRunningGames::default(),
            &reporter,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(updater.downloaded(), ["0.2.0"]);
        assert_eq!(updater.applied(), 1);
        assert!(!reporter.updates().is_empty());
    }

    #[tokio::test]
    async fn refuses_while_a_game_is_busy() {
        let updater = FakeAppUpdater::offering("0.2.0");
        let games = InMemoryRunningGames::default();
        games.claim(&survival(), &CancellationToken::new());
        assert_eq!(
            install("0.2.0", &updater, &games).await,
            Err(AppUpdateError::GameRunning)
        );
        assert!(updater.downloaded().is_empty());
        assert_eq!(updater.applied(), 0);
    }

    #[tokio::test]
    async fn a_game_started_during_the_download_stops_the_install() {
        let games = Arc::new(InMemoryRunningGames::default());
        let during = Arc::clone(&games);
        let updater = FakeAppUpdater {
            during_download: Some(Box::new(move || {
                during.claim(&survival(), &CancellationToken::new());
            })),
            ..FakeAppUpdater::offering("0.2.0")
        };
        assert_eq!(
            install("0.2.0", &updater, &games).await,
            Err(AppUpdateError::GameRunning)
        );
        assert_eq!(updater.applied(), 0);
    }

    #[tokio::test]
    async fn no_game_can_start_while_the_update_installs() {
        let games = Arc::new(InMemoryRunningGames::default());
        let updater = FakeAppUpdater::offering("0.2.0");
        install("0.2.0", &updater, &games).await.unwrap();
        assert_eq!(updater.applied(), 1);
        // The launcher restarts after a successful install; until then, no session starts.
        assert!(!games.claim(&survival(), &CancellationToken::new()));
    }

    #[tokio::test]
    async fn a_failed_install_lets_games_start_again() {
        let games = InMemoryRunningGames::default();
        let updater = FakeAppUpdater {
            apply_error: Some(AppUpdateError::Install("disk full".into())),
            ..FakeAppUpdater::offering("0.2.0")
        };
        assert_eq!(
            install("0.2.0", &updater, &games).await,
            Err(AppUpdateError::Install("disk full".into()))
        );
        assert!(games.claim(&survival(), &CancellationToken::new()));
    }

    #[tokio::test]
    async fn a_version_other_than_the_available_one_is_not_installed() {
        let updater = FakeAppUpdater::offering("0.3.0");
        assert_eq!(
            install("0.2.0", &updater, &InMemoryRunningGames::default()).await,
            Err(AppUpdateError::Changed("0.2.0".into()))
        );
        assert_eq!(updater.applied(), 0);
    }

    #[tokio::test]
    async fn an_unconfigured_build_or_a_failed_download_installs_nothing() {
        let games = InMemoryRunningGames::default();
        let unconfigured = FakeAppUpdater {
            configured: false,
            ..FakeAppUpdater::offering("0.2.0")
        };
        assert_eq!(
            install("0.2.0", &unconfigured, &games).await,
            Err(AppUpdateError::NotConfigured)
        );
        let bad_signature = FakeAppUpdater {
            download_error: Some(AppUpdateError::Download("signature mismatch".into())),
            ..FakeAppUpdater::offering("0.2.0")
        };
        assert_eq!(
            install("0.2.0", &bad_signature, &games).await,
            Err(AppUpdateError::Download("signature mismatch".into()))
        );
        assert_eq!(unconfigured.applied() + bad_signature.applied(), 0);
    }

    #[tokio::test]
    async fn cancelling_before_the_install_installs_nothing() {
        let updater = FakeAppUpdater::offering("0.2.0");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = install_app_update(
            "0.2.0",
            &updater,
            &InMemoryRunningGames::default(),
            &RecordingReporter::default(),
            &cancel,
        )
        .await;
        assert_eq!(result, Err(AppUpdateError::Cancelled));
        assert_eq!(updater.applied(), 0);
    }
}
