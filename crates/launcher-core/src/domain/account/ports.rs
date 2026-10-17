use std::future::Future;

use thiserror::Error;

use super::{Account, AccountId};

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
