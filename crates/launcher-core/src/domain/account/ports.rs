use std::future::Future;

use thiserror::Error;

use super::{Account, AccountId, Username};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountRepositoryError {
    #[error("account storage failed: {0}")]
    Storage(String),
}

/// Persists account metadata (never tokens) and the active selection.
pub trait AccountRepository: Send + Sync {
    fn list(&self) -> impl Future<Output = Result<Vec<Account>, AccountRepositoryError>> + Send;
    fn save(
        &self,
        account: &Account,
    ) -> impl Future<Output = Result<(), AccountRepositoryError>> + Send;
    /// Removing the active account also clears the active selection.
    fn remove(
        &self,
        id: AccountId,
    ) -> impl Future<Output = Result<(), AccountRepositoryError>> + Send;
    fn active(
        &self,
    ) -> impl Future<Output = Result<Option<AccountId>, AccountRepositoryError>> + Send;
    fn set_active(
        &self,
        id: Option<AccountId>,
    ) -> impl Future<Output = Result<(), AccountRepositoryError>> + Send;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PremiumNameLookupError {
    #[error("the profile lookup failed: {0}")]
    Unavailable(String),
}

/// Whether a name belongs to a premium (Microsoft) account. Servers that also
/// accept offline players usually demand premium sign-in for those names.
pub trait PremiumNames: Send + Sync {
    fn is_premium(
        &self,
        name: &Username,
    ) -> impl Future<Output = Result<bool, PremiumNameLookupError>> + Send;
}
