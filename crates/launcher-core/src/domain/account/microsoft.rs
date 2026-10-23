//! Microsoft (premium) accounts: profile data, sessions and the sign-in port.
//!
//! The sign-in chain is the OAuth 2.0 device code flow, then Xbox Live, XSTS
//! and Minecraft Services (`login_with_xbox`), then the entitlement and profile
//! checks. Tokens only ever travel as [`Secret`] and are stored through the
//! `SecretStore` port, never in these types' files.

use std::future::Future;

use thiserror::Error;
use uuid::Uuid;

use super::{AccountId, Cape, CapeId, SkinPng, Username};
use crate::domain::ports::{Secret, SecretKey};

/// Mojang serves skins from this host only.
const TEXTURE_HOST: &str = "textures.minecraft.net";

/// The arm model a skin is drawn for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinVariant {
    /// Four-pixel arms ("Steve").
    Classic,
    /// Three-pixel arms ("Alex").
    Slim,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("not a Minecraft skin texture URL: {0}")]
pub struct SkinUrlError(pub String);

/// A skin texture on `textures.minecraft.net`, always fetched over https
/// (Mojang's profile API answers with plain `http://` links).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinUrl(String);

impl SkinUrl {
    pub fn parse(raw: &str) -> Result<Self, SkinUrlError> {
        let rest = raw
            .strip_prefix("https://")
            .or_else(|| raw.strip_prefix("http://"))
            .ok_or_else(|| SkinUrlError(raw.to_owned()))?;
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| SkinUrlError(raw.to_owned()))?;
        let texture = path
            .strip_prefix("texture/")
            .ok_or_else(|| SkinUrlError(raw.to_owned()))?;
        let valid = host == TEXTURE_HOST
            && !texture.is_empty()
            && texture.len() <= 128
            && texture.chars().all(|c| c.is_ascii_hexdigit());
        if valid {
            Ok(Self(format!("https://{TEXTURE_HOST}/texture/{texture}")))
        } else {
            Err(SkinUrlError(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The skin a profile currently wears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub url: SkinUrl,
    pub variant: SkinVariant,
}

/// What a Microsoft account keeps besides its name and profile UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrosoftAccount {
    /// The Xbox user id, passed to the game as `${auth_xuid}`.
    pub xuid: String,
    pub skin: Option<Skin>,
}

/// What the player sees to approve the sign-in on another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    /// The short code the player types (shown, not secret).
    pub user_code: String,
    /// Where to type it, e.g. `https://www.microsoft.com/link`.
    pub verification_uri: String,
    /// Identifies this sign-in when polling. Secret: it can be exchanged for tokens.
    pub device_code: Secret,
    /// Seconds to wait between polls.
    pub interval_secs: u64,
    /// Seconds until the code stops working.
    pub expires_in_secs: u64,
}

/// Microsoft OAuth tokens. The refresh token rotates on every use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsaTokens {
    pub access_token: Secret,
    pub refresh_token: Secret,
}

/// One answer to a device-code poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    /// The player has not approved yet.
    Pending,
    /// Polling too often: wait longer before the next poll.
    SlowDown,
    Granted(MsaTokens),
}

/// The game's own access token and its lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftToken {
    pub access_token: Secret,
    pub expires_in_secs: u64,
}

/// The result of the Xbox Live, XSTS and `login_with_xbox` exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftLogin {
    pub token: MinecraftToken,
    pub xuid: String,
}

/// The Minecraft Java profile behind a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftProfile {
    pub id: Uuid,
    pub name: Username,
    pub skin: Option<Skin>,
    /// The capes the profile owns; never stored, read again when shown.
    pub capes: Vec<Cape>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("the sign-in code expired before it was approved")]
    CodeExpired,
    #[error("the sign-in was declined")]
    Declined,
    #[error("the Microsoft session is no longer valid; sign in again")]
    SessionExpired,
    #[error("this Microsoft account has no Xbox profile yet")]
    NoXboxAccount,
    #[error("Xbox Live is not available in this account's country")]
    XboxUnavailableInCountry,
    #[error("a child account must be added to a Microsoft family first")]
    ChildAccount,
    #[error("this launcher's app registration is not approved for Minecraft yet")]
    AppNotApproved,
    #[error("this account does not own Minecraft: Java Edition")]
    NotEntitled,
    #[error("this account has no Minecraft: Java Edition profile yet")]
    NoProfile,
    #[error("Minecraft refused this skin")]
    SkinRejected,
    #[error("this profile does not own that cape")]
    CapeNotOwned,
    #[error("too many changes in a short time; wait a few minutes")]
    TooManyRequests,
    #[error("the sign-in service is unreachable: {0}")]
    Network(String),
    #[error("the sign-in service answered unexpectedly: {0}")]
    Unexpected(String),
}

/// Microsoft sign-in, the Xbox and Minecraft Services exchange, and profiles.
pub trait MicrosoftAuth: Send + Sync {
    fn start_device_code(&self) -> impl Future<Output = Result<DeviceCode, AuthError>> + Send;
    fn poll_device_code(
        &self,
        device_code: &Secret,
    ) -> impl Future<Output = Result<DevicePoll, AuthError>> + Send;
    /// `SessionExpired` when the refresh token was revoked or is too old.
    fn refresh(
        &self,
        refresh_token: &Secret,
    ) -> impl Future<Output = Result<MsaTokens, AuthError>> + Send;
    fn minecraft_login(
        &self,
        msa_access_token: &Secret,
    ) -> impl Future<Output = Result<MinecraftLogin, AuthError>> + Send;
    fn owns_game(
        &self,
        minecraft_token: &Secret,
    ) -> impl Future<Output = Result<bool, AuthError>> + Send;
    fn profile(
        &self,
        minecraft_token: &Secret,
    ) -> impl Future<Output = Result<MinecraftProfile, AuthError>> + Send;
    /// The PNG bytes of a skin or cape texture.
    fn skin_texture(
        &self,
        url: &SkinUrl,
    ) -> impl Future<Output = Result<Vec<u8>, AuthError>> + Send;
    /// Uploads a skin; answers the skin Mojang now reports, `None` when its
    /// answer did not say (the upload was still applied). Sent once: Mojang
    /// may suspend accounts that hit its rate limit while uploading.
    fn upload_skin(
        &self,
        minecraft_token: &Secret,
        skin: &SkinPng,
        variant: SkinVariant,
    ) -> impl Future<Output = Result<Option<Skin>, AuthError>> + Send;
    /// Wears an owned cape, or none; answers the profile as it is now.
    fn set_cape(
        &self,
        minecraft_token: &Secret,
        cape: Option<&CapeId>,
    ) -> impl Future<Output = Result<MinecraftProfile, AuthError>> + Send;
}

/// Keychain entry of an account's Microsoft refresh token.
pub fn refresh_token_key(account: AccountId) -> SecretKey {
    SecretKey::new(format!("msa-refresh:{}", account.as_uuid()))
}

/// Keychain entry of an account's cached Minecraft token and its expiry.
pub fn minecraft_token_key(account: AccountId) -> SecretKey {
    SecretKey::new(format!("mc-token:{}", account.as_uuid()))
}

/// A Minecraft token as kept in the keychain: `<expires at, unix secs>:<token>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMinecraftToken {
    pub access_token: Secret,
    pub expires_at_unix: u64,
}

impl StoredMinecraftToken {
    pub fn encode(&self) -> Secret {
        Secret::new(format!(
            "{}:{}",
            self.expires_at_unix,
            self.access_token.expose()
        ))
    }

    /// `None` for an entry this version cannot read; the caller signs in again.
    pub fn decode(stored: &Secret) -> Option<Self> {
        let (expires, token) = stored.expose().split_once(':')?;
        let expires_at_unix = expires.parse().ok()?;
        (!token.is_empty()).then(|| Self {
            access_token: Secret::new(token),
            expires_at_unix,
        })
    }

    /// Still usable `margin_secs` from `now_unix` (refresh a little early).
    pub fn is_fresh(&self, now_unix: u64, margin_secs: u64) -> bool {
        now_unix.saturating_add(margin_secs) < self.expires_at_unix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_urls_are_mojang_textures_upgraded_to_https() {
        let url = SkinUrl::parse("http://textures.minecraft.net/texture/3b60a1f6d562f52aaebbf1434f1de147933a3affe0e764fa49ea057536623cd3").unwrap();
        assert_eq!(
            url.as_str(),
            "https://textures.minecraft.net/texture/3b60a1f6d562f52aaebbf1434f1de147933a3affe0e764fa49ea057536623cd3"
        );
        for bad in [
            "https://evil.example/texture/abc",
            "https://textures.minecraft.net/other/abc",
            "https://textures.minecraft.net/texture/../x",
            "https://textures.minecraft.net/texture/",
            "ftp://textures.minecraft.net/texture/abc",
            "javascript:alert(1)",
        ] {
            assert!(SkinUrl::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn stored_tokens_round_trip_and_know_when_to_refresh() {
        let stored = StoredMinecraftToken {
            access_token: Secret::new("eyJ.token.part"),
            expires_at_unix: 1_000,
        };
        let decoded = StoredMinecraftToken::decode(&stored.encode()).unwrap();
        assert_eq!(decoded, stored);
        assert!(decoded.is_fresh(600, 300));
        assert!(
            !decoded.is_fresh(800, 300),
            "refreshed before the last minutes"
        );
        for bad in ["", "abc", "12:", ":token"] {
            assert!(
                StoredMinecraftToken::decode(&Secret::new(bad)).is_none(),
                "{bad}"
            );
        }
    }

    #[test]
    fn keychain_keys_are_per_account() {
        let id = AccountId::from_uuid(Uuid::nil());
        assert_eq!(
            refresh_token_key(id).as_str(),
            "msa-refresh:00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(
            minecraft_token_key(id).as_str(),
            "mc-token:00000000-0000-0000-0000-000000000000"
        );
    }
}
