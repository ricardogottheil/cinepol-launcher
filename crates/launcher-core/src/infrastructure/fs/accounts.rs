//! `accounts.json`: account metadata and the active selection. Never tokens.

use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::write_atomic;
use crate::domain::account::{
    Account, AccountId, AccountKind, AccountRepository, AccountRepositoryError, Username,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    active: Option<Uuid>,
    #[serde(default)]
    accounts: Vec<AccountRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountRecord {
    id: Uuid,
    kind: String,
    username: String,
    profile_uuid: Uuid,
}

impl From<&Account> for AccountRecord {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.as_uuid(),
            kind: match account.kind {
                AccountKind::Microsoft => "microsoft",
                AccountKind::Offline => "offline",
            }
            .to_owned(),
            username: account.username.to_string(),
            profile_uuid: account.profile_uuid,
        }
    }
}

impl AccountRecord {
    fn into_account(self) -> Option<Account> {
        let kind = match self.kind.as_str() {
            "microsoft" => AccountKind::Microsoft,
            "offline" => AccountKind::Offline,
            _ => return None,
        };
        Some(Account {
            id: AccountId::from_uuid(self.id),
            kind,
            username: Username::parse(&self.username).ok()?,
            profile_uuid: self.profile_uuid,
        })
    }
}

/// Account metadata in one JSON file. A lock serializes read-modify-write cycles.
#[derive(Debug)]
pub struct JsonAccountRepository {
    file: PathBuf,
    lock: Mutex<()>,
}

impl JsonAccountRepository {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            lock: Mutex::new(()),
        }
    }

    async fn load(&self) -> Result<AccountsFile, AccountRepositoryError> {
        match tokio::fs::read(&self.file).await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => Ok(data),
                Err(error) => self.set_aside_corrupt_file(&error.to_string()).await,
            },
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(AccountsFile::default()),
            Err(e) => Err(AccountRepositoryError::Storage(e.to_string())),
        }
    }

    /// A corrupt file would block every account forever. Keep it next to the
    /// original for recovery and start over with an empty list.
    async fn set_aside_corrupt_file(
        &self,
        reason: &str,
    ) -> Result<AccountsFile, AccountRepositoryError> {
        let stamp = uuid::Uuid::new_v4().simple();
        let backup = self.file.with_file_name(format!(
            "{}.corrupt-{stamp}",
            self.file
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default()
        ));
        tokio::fs::rename(&self.file, &backup).await.map_err(|e| {
            AccountRepositoryError::Storage(format!("{reason}; could not set it aside: {e}"))
        })?;
        tracing::warn!(backup = %backup.display(), %reason, "accounts file was unreadable and was set aside");
        Ok(AccountsFile::default())
    }

    async fn store(&self, mut data: AccountsFile) -> Result<(), AccountRepositoryError> {
        data.schema_version = SCHEMA_VERSION;
        let bytes = serde_json::to_vec_pretty(&data)
            .map_err(|e| AccountRepositoryError::Storage(e.to_string()))?;
        write_atomic(&self.file, &bytes)
            .await
            .map_err(|e| AccountRepositoryError::Storage(e.to_string()))
    }
}

impl AccountRepository for JsonAccountRepository {
    async fn list(&self) -> Result<Vec<Account>, AccountRepositoryError> {
        let _guard = self.lock.lock().await;
        Ok(self
            .load()
            .await?
            .accounts
            .into_iter()
            .filter_map(AccountRecord::into_account)
            .collect())
    }

    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let _guard = self.lock.lock().await;
        let mut data = self.load().await?;
        let record = AccountRecord::from(account);
        match data
            .accounts
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            Some(existing) => *existing = record,
            None => data.accounts.push(record),
        }
        self.store(data).await
    }

    async fn remove(&self, id: AccountId) -> Result<(), AccountRepositoryError> {
        let _guard = self.lock.lock().await;
        let mut data = self.load().await?;
        data.accounts.retain(|record| record.id != id.as_uuid());
        if data.active == Some(id.as_uuid()) {
            data.active = None;
        }
        self.store(data).await
    }

    async fn active(&self) -> Result<Option<AccountId>, AccountRepositoryError> {
        let _guard = self.lock.lock().await;
        Ok(self.load().await?.active.map(AccountId::from_uuid))
    }

    async fn set_active(&self, id: Option<AccountId>) -> Result<(), AccountRepositoryError> {
        let _guard = self.lock.lock().await;
        let mut data = self.load().await?;
        data.active = id.map(|id| id.as_uuid());
        self.store(data).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_accounts_and_the_active_selection() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonAccountRepository::new(dir.path().join("accounts.json"));
        assert_eq!(repo.list().await.unwrap(), vec![]);
        let steve = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let alex = Account::offline(AccountId::new_random(), Username::parse("Alex").unwrap());
        repo.save(&steve).await.unwrap();
        repo.save(&alex).await.unwrap();
        repo.set_active(Some(alex.id)).await.unwrap();

        let reopened = JsonAccountRepository::new(dir.path().join("accounts.json"));
        assert_eq!(
            reopened.list().await.unwrap(),
            vec![steve.clone(), alex.clone()]
        );
        assert_eq!(reopened.active().await.unwrap(), Some(alex.id));

        reopened.remove(alex.id).await.unwrap();
        assert_eq!(
            reopened.active().await.unwrap(),
            None,
            "removing the active account clears the selection"
        );
        assert_eq!(reopened.list().await.unwrap(), vec![steve]);
    }

    #[tokio::test]
    async fn never_writes_anything_that_looks_like_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("accounts.json");
        let repo = JsonAccountRepository::new(&file);
        repo.save(&Account::offline(
            AccountId::new_random(),
            Username::parse("Steve").unwrap(),
        ))
        .await
        .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        for forbidden in ["token", "Token", "refresh", "access"] {
            assert!(
                !text.contains(forbidden),
                "{forbidden} found in accounts.json"
            );
        }
    }

    #[tokio::test]
    async fn a_corrupt_file_is_set_aside_instead_of_blocking_every_account() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("accounts.json");
        std::fs::write(&file, b"{ not json").unwrap();
        let repo = JsonAccountRepository::new(&file);

        assert_eq!(repo.list().await.unwrap(), vec![]);
        let steve = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        repo.save(&steve).await.unwrap();
        assert_eq!(repo.list().await.unwrap(), vec![steve]);

        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.starts_with("accounts.json.corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "the unreadable file is kept for recovery");
        assert_eq!(
            std::fs::read(dir.path().join(&backups[0])).unwrap(),
            b"{ not json"
        );
    }
}
