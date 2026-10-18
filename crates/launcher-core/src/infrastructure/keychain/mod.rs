//! The OS keychain: where account tokens live (never plain files).

mod keyring_store;

pub use keyring_store::KeyringSecretStore;
