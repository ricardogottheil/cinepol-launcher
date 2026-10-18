//! Accounts: Microsoft (premium) and offline profiles.
//!
//! Tokens never live in these types; they go to the `SecretStore` port.

mod offline_uuid;
mod policy;
mod ports;
mod username;

pub use offline_uuid::offline_uuid;
pub use policy::{OfflinePolicy, OfflinePolicyError};
pub use ports::{AccountRepository, AccountRepositoryError, PremiumNameLookupError, PremiumNames};
pub use username::{Username, UsernameError};

use uuid::Uuid;

/// Internal identifier of a stored account (random v4, never the profile UUID,
/// so an offline and a premium account with the same name do not collide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountId(Uuid);

impl AccountId {
    pub fn new_random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountKind {
    /// Signed in with a Microsoft account that owns Minecraft.
    Microsoft,
    /// Username-only profile with a vanilla offline UUID.
    Offline,
}

/// A player profile the launcher can start the game with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub kind: AccountKind,
    pub username: Username,
    /// The Minecraft profile UUID passed to the game.
    pub profile_uuid: Uuid,
}

impl Account {
    /// A new offline account; its profile UUID is derived like vanilla's.
    pub fn offline(id: AccountId, username: Username) -> Self {
        let profile_uuid = offline_uuid(&username);
        Self {
            id,
            kind: AccountKind::Offline,
            username,
            profile_uuid,
        }
    }

    pub fn is_premium(&self) -> bool {
        self.kind == AccountKind::Microsoft
    }
}
