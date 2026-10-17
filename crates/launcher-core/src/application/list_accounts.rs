//! Lists stored accounts and which one is active.

use crate::domain::account::{Account, AccountId, AccountRepository, AccountRepositoryError};

pub async fn list_accounts<R: AccountRepository>(
    accounts: &R,
) -> Result<(Vec<Account>, Option<AccountId>), AccountRepositoryError> {
    Ok((accounts.list().await?, accounts.active().await?))
}
