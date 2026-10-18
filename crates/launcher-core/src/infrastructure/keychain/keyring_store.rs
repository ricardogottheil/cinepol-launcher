//! [`SecretStore`] backed by the OS keychain through the `keyring` crate:
//! macOS Keychain, Windows Credential Manager or the Linux Secret Service.
//!
//! There is deliberately no fallback: when the keychain is unavailable the
//! store reports [`SecretStoreError::Unavailable`] instead of writing files.

use crate::domain::ports::{Secret, SecretKey, SecretStore, SecretStoreError};

/// Stores secrets as keychain entries named `<service>` / `<key>`.
#[derive(Debug, Clone)]
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    /// `service` names the application in the keychain, for example
    /// `com.policarpiers.cinepol-launcher`.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Runs a blocking keychain call off the async runtime.
    async fn run<T, F>(&self, key: &SecretKey, operation: F) -> Result<T, SecretStoreError>
    where
        T: Send + 'static,
        F: FnOnce(keyring::Entry) -> Result<T, SecretStoreError> + Send + 'static,
    {
        let service = self.service.clone();
        let user = key.as_str().to_owned();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &user).map_err(map_error)?;
            operation(entry)
        })
        .await
        .map_err(|error| SecretStoreError::Access(format!("keychain task failed: {error}")))?
    }
}

impl SecretStore for KeyringSecretStore {
    async fn get(&self, key: &SecretKey) -> Result<Option<Secret>, SecretStoreError> {
        self.run(key, |entry| get_result(entry.get_password()))
            .await
    }

    async fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), SecretStoreError> {
        let value = secret.clone();
        self.run(key, move |entry| {
            entry.set_password(value.expose()).map_err(map_error)
        })
        .await
    }

    async fn delete(&self, key: &SecretKey) -> Result<(), SecretStoreError> {
        self.run(key, |entry| delete_result(entry.delete_credential()))
            .await
    }
}

/// A missing entry is not an error when reading.
fn get_result(result: keyring::Result<String>) -> Result<Option<Secret>, SecretStoreError> {
    match result {
        Ok(value) => Ok(Some(Secret::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(map_error(error)),
    }
}

/// Deleting a missing entry succeeds, so `delete` is idempotent.
fn delete_result(result: keyring::Result<()>) -> Result<(), SecretStoreError> {
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

/// Maps a keychain error. Messages are written here and never copy the data
/// an error carries (`BadEncoding` and `BadDataFormat` hold the stored
/// bytes), so they can never include a secret value.
fn map_error(error: keyring::Error) -> SecretStoreError {
    use SecretStoreError::{Access, Unavailable};
    use keyring::Error as E;
    match error {
        E::NoDefaultStore => {
            Unavailable("no keychain backend could be initialized on this system".into())
        }
        E::NoStorageAccess(platform) => {
            Unavailable(format!("cannot reach the keychain: {platform}"))
        }
        E::NotSupportedByStore(reason) => Unavailable(format!(
            "the keychain does not support this operation: {reason}"
        )),
        E::Invalid(name, reason) if name == "platform" => {
            Unavailable(format!("unsupported platform: {reason}"))
        }
        E::PlatformFailure(platform) => Access(format!("keychain operation failed: {platform}")),
        E::NoEntry => Access("the keychain entry does not exist".into()),
        E::BadEncoding(_) => Access("the stored value is not valid UTF-8".into()),
        E::BadDataFormat(_, _) => Access("the stored value is malformed".into()),
        E::BadStoreFormat(_) => Access("the keychain data is malformed".into()),
        E::TooLong(name, limit) => Access(format!(
            "the keychain {name} exceeds the platform limit of {limit} characters"
        )),
        E::Invalid(name, reason) => Access(format!("invalid keychain {name}: {reason}")),
        E::Ambiguous(entries) => Access(format!("{} keychain entries match", entries.len())),
        _ => Access("unexpected keychain error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_error(message: &str) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(std::io::Error::other(message.to_owned()))
    }

    #[test]
    fn get_maps_a_missing_entry_to_none() {
        assert_eq!(get_result(Err(keyring::Error::NoEntry)), Ok(None));
    }

    #[test]
    fn get_returns_the_stored_secret() {
        let secret = get_result(Ok("M.C507_value".to_owned()));
        assert_eq!(secret, Ok(Some(Secret::new("M.C507_value"))));
    }

    #[test]
    fn delete_of_a_missing_entry_succeeds() {
        assert_eq!(delete_result(Err(keyring::Error::NoEntry)), Ok(()));
        assert_eq!(delete_result(Ok(())), Ok(()));
    }

    #[test]
    fn a_missing_platform_store_is_unavailable() {
        assert!(matches!(
            map_error(keyring::Error::NoDefaultStore),
            SecretStoreError::Unavailable(_)
        ));
        let error = keyring::Error::Invalid("platform".into(), "must be macOS".into());
        assert!(matches!(map_error(error), SecretStoreError::Unavailable(_)));
    }

    #[test]
    fn a_locked_or_unreachable_store_is_unavailable() {
        let error = keyring::Error::NoStorageAccess(platform_error("no Secret Service"));
        assert_eq!(
            map_error(error),
            SecretStoreError::Unavailable("cannot reach the keychain: no Secret Service".into())
        );
        let error = keyring::Error::NotSupportedByStore("read-only".into());
        assert!(matches!(map_error(error), SecretStoreError::Unavailable(_)));
    }

    #[test]
    fn other_failures_are_access_errors() {
        let error = keyring::Error::PlatformFailure(platform_error("OSStatus -25293"));
        assert_eq!(
            map_error(error),
            SecretStoreError::Access("keychain operation failed: OSStatus -25293".into())
        );
        let error = keyring::Error::TooLong("password".into(), 2560);
        assert!(matches!(map_error(error), SecretStoreError::Access(_)));
        let error = keyring::Error::Invalid("user".into(), "empty".into());
        assert!(matches!(map_error(error), SecretStoreError::Access(_)));
    }

    #[test]
    fn error_messages_never_include_the_stored_bytes() {
        let value = b"eyJsecret.refresh.token".to_vec();
        let errors = [
            keyring::Error::BadEncoding(value.clone()),
            keyring::Error::BadDataFormat(value, platform_error("bad blob")),
        ];
        for error in errors {
            let mapped = map_error(error);
            assert!(matches!(mapped, SecretStoreError::Access(_)));
            assert!(!mapped.to_string().contains("eyJsecret"), "{mapped}");
        }
    }

    // Touches the real OS keychain (it may show a system prompt, and leaves an
    // entry behind if it fails midway), so it only runs on demand:
    // `cargo test -p cinepol-launcher-core -- --ignored keychain_round_trip`.
    #[tokio::test]
    #[ignore = "touches the real OS keychain"]
    async fn keychain_round_trip() {
        let store = KeyringSecretStore::new("com.policarpiers.cinepol-launcher.test");
        let key = SecretKey::new("round-trip-test");
        let secret = Secret::new("M.C507_round-trip-value");

        store.set(&key, &secret).await.expect("set");
        assert_eq!(store.get(&key).await.expect("get"), Some(secret));
        store.delete(&key).await.expect("delete");
        assert_eq!(store.get(&key).await.expect("get after delete"), None);
        store
            .delete(&key)
            .await
            .expect("second delete is idempotent");
    }
}
