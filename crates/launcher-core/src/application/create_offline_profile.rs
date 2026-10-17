//! Creates a username-only (offline) account with a vanilla offline UUID.

use thiserror::Error;

use crate::domain::account::{
    Account, AccountId, AccountKind, AccountRepository, AccountRepositoryError, OfflinePolicy,
    OfflinePolicyError, Username, UsernameError,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CreateOfflineProfileError {
    #[error(transparent)]
    InvalidUsername(#[from] UsernameError),
    #[error(transparent)]
    PremiumRequired(#[from] OfflinePolicyError),
    #[error("an offline profile named {0} already exists")]
    AlreadyExists(String),
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
}

/// Validates the name, applies `OfflinePolicy`, refuses a duplicate offline
/// name (case-insensitive, like Minecraft names), saves the account and makes
/// it active when no account is active yet.
pub async fn create_offline_profile<R: AccountRepository>(
    raw_username: &str,
    policy: OfflinePolicy,
    accounts: &R,
) -> Result<Account, CreateOfflineProfileError> {
    let username = Username::parse(raw_username.trim())?;
    let existing = accounts.list().await?;
    policy.check_can_create_offline(&existing)?;

    let duplicate = existing.iter().any(|account| {
        account.kind == AccountKind::Offline
            && account
                .username
                .as_str()
                .eq_ignore_ascii_case(username.as_str())
    });
    if duplicate {
        return Err(CreateOfflineProfileError::AlreadyExists(
            username.to_string(),
        ));
    }

    let account = Account::offline(AccountId::new_random(), username);
    accounts.save(&account).await?;
    if accounts.active().await?.is_none() {
        accounts.set_active(Some(account.id)).await?;
    }
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::offline_uuid;
    use crate::testing::InMemoryAccountRepository;

    #[tokio::test]
    async fn creates_saves_and_activates_the_first_profile() {
        let repo = InMemoryAccountRepository::default();
        let account = create_offline_profile("  polispol1 ", OfflinePolicy::default(), &repo)
            .await
            .unwrap();

        assert_eq!(account.username.as_str(), "polispol1");
        assert_eq!(account.kind, AccountKind::Offline);
        assert_eq!(account.profile_uuid, offline_uuid(&account.username));
        assert_eq!(repo.list().await.unwrap(), vec![account.clone()]);
        assert_eq!(repo.active().await.unwrap(), Some(account.id));
    }

    #[tokio::test]
    async fn keeps_the_current_active_account() {
        let repo = InMemoryAccountRepository::default();
        let first = create_offline_profile("Steve", OfflinePolicy::default(), &repo)
            .await
            .unwrap();
        create_offline_profile("Alex", OfflinePolicy::default(), &repo)
            .await
            .unwrap();
        assert_eq!(repo.active().await.unwrap(), Some(first.id));
    }

    #[tokio::test]
    async fn rejects_invalid_names_duplicates_and_gated_creation() {
        let repo = InMemoryAccountRepository::default();
        assert!(matches!(
            create_offline_profile("no spaces", OfflinePolicy::default(), &repo).await,
            Err(CreateOfflineProfileError::InvalidUsername(_))
        ));

        create_offline_profile("Steve", OfflinePolicy::default(), &repo)
            .await
            .unwrap();
        assert_eq!(
            create_offline_profile("STEVE", OfflinePolicy::default(), &repo).await,
            Err(CreateOfflineProfileError::AlreadyExists("STEVE".into()))
        );

        let gated = OfflinePolicy {
            require_premium_for_offline: true,
        };
        assert_eq!(
            create_offline_profile("Alex", gated, &repo).await,
            Err(CreateOfflineProfileError::PremiumRequired(
                OfflinePolicyError::PremiumRequired
            ))
        );
        assert_eq!(repo.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reports_storage_failures() {
        let repo = InMemoryAccountRepository::default();
        repo.fail_writes();
        assert!(matches!(
            create_offline_profile("Steve", OfflinePolicy::default(), &repo).await,
            Err(CreateOfflineProfileError::Storage(_))
        ));
    }
}
