//! Signs in with a Microsoft account: the device code flow, then Xbox Live,
//! XSTS and Minecraft Services, the ownership check and the profile. Tokens go
//! to the OS keychain; the account (name, UUID, xuid, skin) to the repository.

use std::time::Duration;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::account::{
    Account, AccountId, AccountRepository, AccountRepositoryError, AuthError, DeviceCode,
    DevicePoll, MicrosoftAuth, MsaTokens, StoredMinecraftToken, minecraft_token_key,
    refresh_token_key,
};
use crate::domain::ports::{Clock, Delay, SecretStore, SecretStoreError, unix_seconds};

/// Extra wait Microsoft asks for with `slow_down`.
const SLOW_DOWN_SECS: u64 = 5;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoginError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("the sign-in was cancelled")]
    Cancelled,
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
    #[error(transparent)]
    Keychain(#[from] SecretStoreError),
}

pub struct LoginDeps<'a, M, A, K, C, D> {
    pub auth: &'a M,
    pub accounts: &'a A,
    pub secrets: &'a K,
    pub clock: &'a C,
    pub delay: &'a D,
}

/// `on_code` receives the code to show the player as soon as it exists.
/// Signing in again with the same profile updates that account instead of
/// adding a second one; a new account becomes active when none is.
pub async fn login_microsoft<M, A, K, C, D>(
    deps: &LoginDeps<'_, M, A, K, C, D>,
    on_code: impl FnOnce(&DeviceCode) + Send,
    cancel: &CancellationToken,
) -> Result<Account, LoginError>
where
    M: MicrosoftAuth,
    A: AccountRepository,
    K: SecretStore,
    C: Clock,
    D: Delay,
{
    let code = deps.auth.start_device_code().await?;
    on_code(&code);
    let tokens = wait_for_approval(deps, &code, cancel).await?;
    if cancel.is_cancelled() {
        return Err(LoginError::Cancelled);
    }
    finish_sign_in(deps, tokens).await
}

/// Polls until the player approves, declines, the code expires or the
/// sign-in is cancelled. Expiry counts the waited time, not the clock.
async fn wait_for_approval<M, A, K, C, D>(
    deps: &LoginDeps<'_, M, A, K, C, D>,
    code: &DeviceCode,
    cancel: &CancellationToken,
) -> Result<MsaTokens, LoginError>
where
    M: MicrosoftAuth,
    D: Delay,
{
    let mut interval = code.interval_secs.max(1);
    let mut waited = 0;
    loop {
        if waited + interval > code.expires_in_secs {
            return Err(AuthError::CodeExpired.into());
        }
        cancel
            .run_until_cancelled(deps.delay.wait(Duration::from_secs(interval)))
            .await
            .ok_or(LoginError::Cancelled)?;
        waited += interval;
        match deps.auth.poll_device_code(&code.device_code).await? {
            DevicePoll::Pending => {}
            DevicePoll::SlowDown => interval += SLOW_DOWN_SECS,
            DevicePoll::Granted(tokens) => return Ok(tokens),
        }
    }
}

async fn finish_sign_in<M, A, K, C, D>(
    deps: &LoginDeps<'_, M, A, K, C, D>,
    tokens: MsaTokens,
) -> Result<Account, LoginError>
where
    M: MicrosoftAuth,
    A: AccountRepository,
    K: SecretStore,
    C: Clock,
{
    let login = deps.auth.minecraft_login(&tokens.access_token).await?;
    let game_token = &login.token.access_token;
    if !deps.auth.owns_game(game_token).await? {
        return Err(AuthError::NotEntitled.into());
    }
    let profile = deps.auth.profile(game_token).await?;

    let existing = deps
        .accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.is_premium() && account.profile_uuid == profile.id);
    let id = existing
        .as_ref()
        .map_or_else(AccountId::new_random, |account| account.id);
    let account = Account::microsoft(id, &profile, login.xuid);

    let refresh_key = refresh_token_key(id);
    let token_key = minecraft_token_key(id);
    let stored = StoredMinecraftToken {
        access_token: game_token.clone(),
        expires_at_unix: unix_seconds(deps.clock.now()).saturating_add(login.token.expires_in_secs),
    };
    deps.secrets
        .set(&refresh_key, &tokens.refresh_token)
        .await?;
    deps.secrets.set(&token_key, &stored.encode()).await?;
    if let Err(error) = deps.accounts.save(&account).await {
        // A new account that could not be saved must not leave tokens behind.
        if existing.is_none() {
            let _ = deps.secrets.delete(&refresh_key).await;
            let _ = deps.secrets.delete(&token_key).await;
        }
        return Err(error.into());
    }
    if deps.accounts.active().await?.is_none() {
        // The account is usable without being active, so this is not fatal.
        if let Err(error) = deps.accounts.set_active(Some(id)).await {
            tracing::warn!(%error, "could not make the new account active");
        }
    }
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::AccountKind;
    use crate::domain::ports::Secret;
    use crate::testing::{
        FakeMicrosoftAuth, FixedClock, InMemoryAccountRepository, InMemorySecretStore,
        InstantDelay, sample_profile, tokens,
    };

    struct World {
        auth: FakeMicrosoftAuth,
        accounts: InMemoryAccountRepository,
        secrets: InMemorySecretStore,
        clock: FixedClock,
        delay: InstantDelay,
    }

    impl World {
        fn new(auth: FakeMicrosoftAuth) -> Self {
            Self {
                auth,
                accounts: InMemoryAccountRepository::default(),
                secrets: InMemorySecretStore::default(),
                clock: FixedClock::at_unix(1_000),
                delay: InstantDelay::default(),
            }
        }

        async fn login(
            &self,
            cancel: &CancellationToken,
        ) -> (Result<Account, LoginError>, Vec<String>) {
            let deps = LoginDeps {
                auth: &self.auth,
                accounts: &self.accounts,
                secrets: &self.secrets,
                clock: &self.clock,
                delay: &self.delay,
            };
            let shown = std::sync::Mutex::new(Vec::new());
            let result = login_microsoft(
                &deps,
                |code| shown.lock().unwrap().push(code.user_code.clone()),
                cancel,
            )
            .await;
            (result, shown.into_inner().unwrap())
        }
    }

    #[tokio::test]
    async fn signs_in_stores_the_tokens_in_the_keychain_and_activates_the_account() {
        let world = World::new(FakeMicrosoftAuth::approving());
        let (result, shown) = world.login(&CancellationToken::new()).await;
        let account = result.unwrap();

        assert_eq!(shown, ["ABCD-EFGH"]);
        assert_eq!(
            world.auth.calls(),
            [
                "start",
                "poll:device-code",
                "login:msa-access-1",
                "owns:minecraft-token",
                "profile:minecraft-token",
            ]
        );
        let profile = sample_profile();
        assert_eq!(account.username, profile.name);
        assert_eq!(account.profile_uuid, profile.id);
        assert!(
            matches!(&account.kind, AccountKind::Microsoft(m) if m.xuid == "2535428" && m.skin == profile.skin)
        );
        assert_eq!(
            world.accounts.list().await.unwrap(),
            std::slice::from_ref(&account)
        );
        assert_eq!(world.accounts.active().await.unwrap(), Some(account.id));
        assert_eq!(
            world
                .secrets
                .get(&refresh_token_key(account.id))
                .await
                .unwrap(),
            Some(Secret::new("msa-refresh-1"))
        );
        let cached = world
            .secrets
            .get(&minecraft_token_key(account.id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            StoredMinecraftToken::decode(&cached),
            Some(StoredMinecraftToken {
                access_token: Secret::new("minecraft-token"),
                expires_at_unix: 1_000 + 86_400,
            })
        );
    }

    #[tokio::test]
    async fn polls_at_the_given_interval_and_backs_off_on_slow_down() {
        let auth = FakeMicrosoftAuth::approving().with_polls(vec![
            Ok(DevicePoll::Pending),
            Ok(DevicePoll::SlowDown),
            Ok(DevicePoll::Granted(tokens(1))),
        ]);
        let world = World::new(auth);
        let (result, _) = world.login(&CancellationToken::new()).await;
        assert!(result.is_ok());
        assert_eq!(
            world.delay.waits(),
            [
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(10)
            ]
        );
    }

    #[tokio::test]
    async fn a_code_nobody_approves_expires_without_saving_anything() {
        let world = World::new(FakeMicrosoftAuth::approving().with_polls(vec![]));
        let (result, _) = world.login(&CancellationToken::new()).await;
        assert_eq!(result, Err(LoginError::Auth(AuthError::CodeExpired)));
        let waited: Duration = world.delay.waits().iter().sum();
        assert!(waited <= Duration::from_secs(900), "{waited:?}");
        assert!(world.accounts.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelling_stops_polling_and_saves_nothing() {
        let world = World::new(FakeMicrosoftAuth::approving().with_polls(vec![]));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (result, _) = world.login(&cancel).await;
        assert_eq!(result, Err(LoginError::Cancelled));
        assert!(
            !world
                .auth
                .calls()
                .iter()
                .any(|call| call.starts_with("poll"))
        );
        assert!(world.accounts.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_account_without_the_game_is_refused_and_nothing_is_kept() {
        let mut auth = FakeMicrosoftAuth::approving();
        auth.owns_game = Ok(false);
        let world = World::new(auth);
        let (result, _) = world.login(&CancellationToken::new()).await;
        assert_eq!(result, Err(LoginError::Auth(AuthError::NotEntitled)));
        assert!(world.accounts.list().await.unwrap().is_empty());
        assert!(
            !world
                .auth
                .calls()
                .iter()
                .any(|call| call.starts_with("profile"))
        );
    }

    #[tokio::test]
    async fn sign_in_failures_pass_through_without_saving_anything() {
        for error in [
            AuthError::Declined,
            AuthError::AppNotApproved,
            AuthError::NoXboxAccount,
        ] {
            let mut auth = FakeMicrosoftAuth::approving();
            if error == AuthError::Declined {
                auth = auth.with_polls(vec![Err(AuthError::Declined)]);
            } else {
                auth.login = Err(error.clone());
            }
            let world = World::new(auth);
            let (result, _) = world.login(&CancellationToken::new()).await;
            assert_eq!(result, Err(LoginError::Auth(error)));
            assert!(world.accounts.list().await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn signing_in_again_updates_the_same_account() {
        let world = World::new(FakeMicrosoftAuth::approving());
        let first = world.login(&CancellationToken::new()).await.0.unwrap();
        *world.auth.polls.lock().unwrap() = [Ok(DevicePoll::Granted(tokens(3)))].into();
        let second = world.login(&CancellationToken::new()).await.0.unwrap();
        assert_eq!(second.id, first.id);
        assert_eq!(world.accounts.list().await.unwrap().len(), 1);
        assert_eq!(
            world
                .secrets
                .get(&refresh_token_key(first.id))
                .await
                .unwrap(),
            Some(Secret::new("msa-refresh-3"))
        );
    }

    #[tokio::test]
    async fn a_failed_save_leaves_no_tokens_behind() {
        let world = World::new(FakeMicrosoftAuth::approving());
        world.accounts.fail_writes();
        let (result, _) = world.login(&CancellationToken::new()).await;
        assert!(matches!(result, Err(LoginError::Storage(_))));
        assert!(world.secrets.is_empty(), "no orphaned keychain entries");
    }
}
