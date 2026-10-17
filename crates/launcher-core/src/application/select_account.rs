//! Makes an existing account the active one.

use thiserror::Error;

use crate::domain::account::{AccountId, AccountRepository, AccountRepositoryError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SelectAccountError {
    #[error("account not found")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
}

pub async fn select_account<R: AccountRepository>(
    id: AccountId,
    accounts: &R,
) -> Result<(), SelectAccountError> {
    if !accounts
        .list()
        .await?
        .iter()
        .any(|account| account.id == id)
    {
        return Err(SelectAccountError::NotFound);
    }
    accounts.set_active(Some(id)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::testing::InMemoryAccountRepository;

    #[tokio::test]
    async fn activates_known_accounts_only() {
        let steve = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let repo = InMemoryAccountRepository::with(vec![steve.clone()]);
        assert_eq!(
            select_account(AccountId::new_random(), &repo).await,
            Err(SelectAccountError::NotFound)
        );
        assert_eq!(repo.active().await.unwrap(), None);
        select_account(steve.id, &repo).await.unwrap();
        assert_eq!(repo.active().await.unwrap(), Some(steve.id));
    }
}
