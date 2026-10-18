//! Keeps a Microsoft account's game session usable: the cached Minecraft
//! token when it is still fresh, otherwise a silent refresh with the stored
//! refresh token. Runs before every premium launch.

use std::future::Future;

use thiserror::Error;

use crate::domain::account::{
    Account, AuthError, MicrosoftAuth, StoredMinecraftToken, minecraft_token_key, refresh_token_key,
};
use crate::domain::ports::{Clock, Secret, SecretStore, SecretStoreError, unix_seconds};

/// Refresh this long before the token expires, so a launch never starts
/// with a token about to run out.
pub const REFRESH_MARGIN_SECS: u64 = 300;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    /// No stored session or Microsoft refused it: the player signs in again.
    #[error("the Microsoft session expired; sign in again")]
    Expired,
    #[error(transparent)]
    Auth(AuthError),
    #[error(transparent)]
    Keychain(#[from] SecretStoreError),
}

impl From<AuthError> for SessionError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::SessionExpired => Self::Expired,
            other => Self::Auth(other),
        }
    }
}

/// A valid game token for a premium account.
pub trait PremiumSessions: Send + Sync {
    fn session_token(
        &self,
        account: &Account,
    ) -> impl Future<Output = Result<Secret, SessionError>> + Send;
}

/// Sessions backed by Microsoft sign-in and the OS keychain.
pub struct MicrosoftSessions<'a, M, K, C> {
    pub auth: &'a M,
    pub secrets: &'a K,
    pub clock: &'a C,
}

impl<M: MicrosoftAuth, K: SecretStore, C: Clock> PremiumSessions
    for MicrosoftSessions<'_, M, K, C>
{
    async fn session_token(&self, account: &Account) -> Result<Secret, SessionError> {
        let now = unix_seconds(self.clock.now());
        let token_key = minecraft_token_key(account.id);
        let cached = self
            .secrets
            .get(&token_key)
            .await?
            .and_then(|stored| StoredMinecraftToken::decode(&stored))
            .filter(|token| token.is_fresh(now, REFRESH_MARGIN_SECS));
        if let Some(token) = cached {
            return Ok(token.access_token);
        }

        let refresh_key = refresh_token_key(account.id);
        let refresh_token = self
            .secrets
            .get(&refresh_key)
            .await?
            .ok_or(SessionError::Expired)?;
        let tokens = self.auth.refresh(&refresh_token).await?;
        // The used refresh token is spent: keep the new one before anything else.
        self.secrets
            .set(&refresh_key, &tokens.refresh_token)
            .await?;
        let login = self.auth.minecraft_login(&tokens.access_token).await?;
        let stored = StoredMinecraftToken {
            access_token: login.token.access_token.clone(),
            expires_at_unix: now.saturating_add(login.token.expires_in_secs),
        };
        self.secrets.set(&token_key, &stored.encode()).await?;
        Ok(login.token.access_token)
    }
}

/// Never answers a token: for launchers built without Microsoft sign-in.
pub struct NoPremiumSessions;

impl PremiumSessions for NoPremiumSessions {
    async fn session_token(&self, _account: &Account) -> Result<Secret, SessionError> {
        Err(SessionError::Expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{AccountId, MinecraftLogin, MinecraftToken};
    use crate::testing::{FakeMicrosoftAuth, FixedClock, InMemorySecretStore, sample_profile};

    fn account() -> Account {
        Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428")
    }

    async fn stored(secrets: &InMemorySecretStore, account: &Account, token: &str, expires: u64) {
        let entry = StoredMinecraftToken {
            access_token: Secret::new(token),
            expires_at_unix: expires,
        };
        secrets
            .set(&minecraft_token_key(account.id), &entry.encode())
            .await
            .unwrap();
    }

    fn at(unix: u64) -> FixedClock {
        FixedClock::at_unix(unix)
    }

    #[tokio::test]
    async fn a_fresh_cached_token_is_used_without_any_request() {
        let auth = FakeMicrosoftAuth::approving();
        let secrets = InMemorySecretStore::default();
        let account = account();
        stored(&secrets, &account, "cached", 10_000).await;
        let sessions = MicrosoftSessions {
            auth: &auth,
            secrets: &secrets,
            clock: &at(1_000),
        };

        assert_eq!(
            sessions.session_token(&account).await,
            Ok(Secret::new("cached"))
        );
        assert!(auth.calls().is_empty());
    }

    #[tokio::test]
    async fn a_stale_token_is_refreshed_and_both_new_tokens_are_kept() {
        let mut auth = FakeMicrosoftAuth::approving();
        auth.login = Ok(MinecraftLogin {
            token: MinecraftToken {
                access_token: Secret::new("fresh"),
                expires_in_secs: 86_400,
            },
            xuid: "2535428".into(),
        });
        let secrets = InMemorySecretStore::default();
        let account = account();
        stored(&secrets, &account, "old", 1_100).await;
        secrets
            .set(
                &refresh_token_key(account.id),
                &Secret::new("msa-refresh-1"),
            )
            .await
            .unwrap();
        let sessions = MicrosoftSessions {
            auth: &auth,
            secrets: &secrets,
            clock: &at(1_000),
        };

        assert_eq!(
            sessions.session_token(&account).await,
            Ok(Secret::new("fresh"))
        );
        assert_eq!(
            auth.calls(),
            ["refresh:msa-refresh-1", "login:msa-access-2"]
        );
        assert_eq!(
            secrets.get(&refresh_token_key(account.id)).await.unwrap(),
            Some(Secret::new("msa-refresh-2")),
            "the rotated refresh token replaces the used one"
        );
        let cached = secrets
            .get(&minecraft_token_key(account.id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            StoredMinecraftToken::decode(&cached),
            Some(StoredMinecraftToken {
                access_token: Secret::new("fresh"),
                expires_at_unix: 1_000 + 86_400,
            })
        );
    }

    #[tokio::test]
    async fn without_a_refresh_token_or_when_microsoft_refuses_it_the_session_expired() {
        let auth = FakeMicrosoftAuth::approving();
        let secrets = InMemorySecretStore::default();
        let account = account();
        let sessions = MicrosoftSessions {
            auth: &auth,
            secrets: &secrets,
            clock: &at(1_000),
        };
        assert_eq!(
            sessions.session_token(&account).await,
            Err(SessionError::Expired)
        );

        let mut refused = FakeMicrosoftAuth::approving();
        refused.refresh = Err(AuthError::SessionExpired);
        secrets
            .set(&refresh_token_key(account.id), &Secret::new("revoked"))
            .await
            .unwrap();
        let sessions = MicrosoftSessions {
            auth: &refused,
            secrets: &secrets,
            clock: &at(1_000),
        };
        assert_eq!(
            sessions.session_token(&account).await,
            Err(SessionError::Expired)
        );
    }

    #[tokio::test]
    async fn network_failures_are_reported_as_such_not_as_an_expired_session() {
        let mut auth = FakeMicrosoftAuth::approving();
        auth.refresh = Err(AuthError::Network("offline".into()));
        let secrets = InMemorySecretStore::default();
        let account = account();
        secrets
            .set(
                &refresh_token_key(account.id),
                &Secret::new("msa-refresh-1"),
            )
            .await
            .unwrap();
        let sessions = MicrosoftSessions {
            auth: &auth,
            secrets: &secrets,
            clock: &at(1_000),
        };
        assert_eq!(
            sessions.session_token(&account).await,
            Err(SessionError::Auth(AuthError::Network("offline".into())))
        );
        assert_eq!(
            secrets.get(&refresh_token_key(account.id)).await.unwrap(),
            Some(Secret::new("msa-refresh-1")),
            "a failed refresh keeps the stored token for the next try"
        );
    }
}
