//! Removes an account and, for a Microsoft account, its tokens in the keychain.

use thiserror::Error;

use crate::domain::account::{
    AccountId, AccountRepository, AccountRepositoryError, minecraft_token_key, refresh_token_key,
};
use crate::domain::ports::{SecretStore, SecretStoreError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoveAccountError {
    #[error("no account with that id")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
    #[error(transparent)]
    Keychain(#[from] SecretStoreError),
}

/// Tokens go first: an account whose tokens could not be removed stays listed,
/// so the player can try again instead of leaving secrets in the keychain.
/// Removing the active account clears the selection.
pub async fn remove_account<A: AccountRepository, K: SecretStore>(
    id: AccountId,
    accounts: &A,
    secrets: &K,
) -> Result<(), RemoveAccountError> {
    let account = accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or(RemoveAccountError::NotFound)?;
    if account.is_premium() {
        secrets.delete(&refresh_token_key(id)).await?;
        secrets.delete(&minecraft_token_key(id)).await?;
    }
    accounts.remove(id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::domain::ports::Secret;
    use crate::testing::{InMemoryAccountRepository, InMemorySecretStore, sample_profile};

    #[tokio::test]
    async fn removes_a_microsoft_account_with_its_tokens_and_the_selection() {
        let premium = Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428");
        let accounts = InMemoryAccountRepository::with(vec![premium.clone()]);
        accounts.set_active(Some(premium.id)).await.unwrap();
        let secrets = InMemorySecretStore::default();
        secrets
            .set(&refresh_token_key(premium.id), &Secret::new("r"))
            .await
            .unwrap();
        secrets
            .set(&minecraft_token_key(premium.id), &Secret::new("1:t"))
            .await
            .unwrap();

        remove_account(premium.id, &accounts, &secrets)
            .await
            .unwrap();

        assert!(accounts.list().await.unwrap().is_empty());
        assert_eq!(accounts.active().await.unwrap(), None);
        assert!(secrets.is_empty());
    }

    #[tokio::test]
    async fn removes_an_offline_account_and_refuses_unknown_ids() {
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![offline.clone()]);
        let secrets = InMemorySecretStore::default();
        remove_account(offline.id, &accounts, &secrets)
            .await
            .unwrap();
        assert!(accounts.list().await.unwrap().is_empty());
        assert_eq!(
            remove_account(offline.id, &accounts, &secrets).await,
            Err(RemoveAccountError::NotFound)
        );
    }
}
