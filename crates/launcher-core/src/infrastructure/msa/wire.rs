//! The JSON answers of the Microsoft, Xbox and Minecraft Services endpoints.
//! Unknown fields are ignored; only what the launcher reads is declared.
//! These types hold raw tokens, so none of them implements `Debug`.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct DeviceCodeResponse {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    /// Microsoft omits it sometimes; RFC 8628 says to default to 5 s.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// The body of an OAuth error answer (HTTP 400).
#[derive(Deserialize)]
pub struct OAuthErrorResponse {
    pub error: String,
    /// The `AADSTS` numbers; safe to log, unlike the free-text description.
    #[serde(default)]
    pub error_codes: Vec<u64>,
}

/// The answer of both the Xbox Live and the XSTS endpoints.
#[derive(Deserialize)]
pub struct XboxTokenResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: DisplayClaims,
}

#[derive(Deserialize)]
pub struct DisplayClaims {
    pub xui: Vec<XboxUserClaims>,
}

#[derive(Deserialize)]
pub struct XboxUserClaims {
    /// The user hash, part of the Minecraft identity token.
    pub uhs: String,
    /// The Xbox user id. Only some relying parties include it.
    pub xid: Option<String>,
}

/// The body of an XSTS refusal (HTTP 401).
#[derive(Deserialize)]
pub struct XstsErrorResponse {
    #[serde(rename = "XErr")]
    pub xerr: u64,
}

#[derive(Deserialize)]
pub struct MinecraftLoginResponse {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
pub struct EntitlementsResponse {
    #[serde(default)]
    pub items: Vec<EntitlementItem>,
}

#[derive(Deserialize)]
pub struct EntitlementItem {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ProfileResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<ProfileSkin>,
    #[serde(default)]
    pub capes: Vec<ProfileCape>,
}

/// Lenient like `ProfileSkin`: an unreadable cape is left out.
#[derive(Deserialize)]
pub struct ProfileCape {
    pub id: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub alias: Option<String>,
}

/// Lenient on purpose: a skin the launcher cannot read is dropped instead of
/// failing the whole sign-in.
#[derive(Deserialize)]
pub struct ProfileSkin {
    pub state: Option<String>,
    pub url: Option<String>,
    pub variant: Option<String>,
}
