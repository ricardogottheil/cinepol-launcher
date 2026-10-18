//! The JSON answers of the Microsoft, Xbox and Minecraft Services endpoints.
//! Unknown fields are ignored; only what the launcher reads is declared.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// The body of an OAuth error answer (HTTP 400).
#[derive(Debug, Deserialize)]
pub struct OAuthErrorResponse {
    pub error: String,
}

/// The answer of both the Xbox Live and the XSTS endpoints.
#[derive(Debug, Deserialize)]
pub struct XboxTokenResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
pub struct DisplayClaims {
    pub xui: Vec<XboxUserClaims>,
}

#[derive(Debug, Deserialize)]
pub struct XboxUserClaims {
    /// The user hash, part of the Minecraft identity token.
    pub uhs: String,
    /// The Xbox user id. Only some relying parties include it.
    pub xid: Option<String>,
}

/// The body of an XSTS refusal (HTTP 401).
#[derive(Debug, Deserialize)]
pub struct XstsErrorResponse {
    #[serde(rename = "XErr")]
    pub xerr: u64,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftLoginResponse {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct EntitlementsResponse {
    #[serde(default)]
    pub items: Vec<EntitlementItem>,
}

#[derive(Debug, Deserialize)]
pub struct EntitlementItem {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ProfileResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<ProfileSkin>,
}

/// Lenient on purpose: a skin the launcher cannot read is dropped instead of
/// failing the whole sign-in.
#[derive(Debug, Deserialize)]
pub struct ProfileSkin {
    pub state: Option<String>,
    pub url: Option<String>,
    pub variant: Option<String>,
}
