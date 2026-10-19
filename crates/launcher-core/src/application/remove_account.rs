//! Removes an account and, for a Microsoft account, its tokens in the keychain.

use thiserror::Error;

use crate::domain::account::{
    AccountId, AccountRepository, AccountRepositoryError, minecraft_token_key, refresh_token_key,
};
use crate::domain::instance::{InstanceRepository, InstanceRepositoryError};
use crate::domain::ports::{SecretStore, SecretStoreError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoveAccountError {
    #[error("no account with that id")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
    #[error(transparent)]
    Keychain(#[from] SecretStoreError),
    #[error(transparent)]
    Instances(#[from] InstanceRepositoryError),
}

/// Tokens go first: an account whose tokens could not be removed stays listed,
/// so the player can try again instead of leaving secrets in the keychain.
/// Removing the active account clears the selection, and instances pinned to
/// it go back to launching with the active account.
pub async fn remove_account<A: AccountRepository, K: SecretStore, I: InstanceRepository>(
    id: AccountId,
    accounts: &A,
    secrets: &K,
    instances: &I,
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
    for mut instance in instances.list().await? {
        if instance.default_account == Some(id) {
            instance.default_account = None;
            instances.save(&instance).await?;
        }
    }
    accounts.remove(id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::domain::ports::Secret;
    use crate::testing::{
        InMemoryAccountRepository, InMemoryInstanceRepository, InMemorySecretStore, sample_profile,
    };

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

        remove_account(
            premium.id,
            &accounts,
            &secrets,
            &InMemoryInstanceRepository::default(),
        )
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
        remove_account(
            offline.id,
            &accounts,
            &secrets,
            &InMemoryInstanceRepository::default(),
        )
        .await
        .unwrap();
        assert!(accounts.list().await.unwrap().is_empty());
        assert_eq!(
            remove_account(
                offline.id,
                &accounts,
                &secrets,
                &InMemoryInstanceRepository::default()
            )
            .await,
            Err(RemoveAccountError::NotFound)
        );
    }

    #[tokio::test]
    async fn instances_pinned_to_the_account_fall_back_to_the_active_one() {
        use crate::domain::instance::{
            GameVersion, Instance, InstanceId, InstanceName, InstanceOrigin, InstanceRepository,
        };
        use crate::domain::loader::LoaderSpec;
        let removed = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let kept = Account::offline(AccountId::new_random(), Username::parse("Alex").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![removed.clone(), kept.clone()]);
        let instances = InMemoryInstanceRepository::default();
        for (id, pin) in [("pinned", removed.id), ("other", kept.id)] {
            let mut instance = Instance::new(
                InstanceId::parse(id).unwrap(),
                InstanceName::parse(id).unwrap(),
                GameVersion::parse("26.2").unwrap(),
                LoaderSpec::vanilla(),
                InstanceOrigin::Custom,
            );
            instance.default_account = Some(pin);
            instances.save(&instance).await.unwrap();
        }

        remove_account(
            removed.id,
            &accounts,
            &InMemorySecretStore::default(),
            &instances,
        )
        .await
        .unwrap();

        let pin = |id: &str| {
            let id = InstanceId::parse(id).unwrap();
            let instances = &instances;
            async move { instances.get(&id).await.unwrap().default_account }
        };
        assert_eq!(pin("pinned").await, None);
        assert_eq!(pin("other").await, Some(kept.id));
    }
}
