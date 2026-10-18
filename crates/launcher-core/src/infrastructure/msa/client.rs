//! [`MicrosoftAuth`] against the real Microsoft, Xbox and Minecraft Services
//! endpoints.

use serde::de::DeserializeOwned;
use serde_json::json;
use uuid::Uuid;

use super::wire::{
    DeviceCodeResponse, EntitlementsResponse, MinecraftLoginResponse, OAuthErrorResponse,
    ProfileResponse, ProfileSkin, TokenResponse, XboxTokenResponse, XboxUserClaims,
    XstsErrorResponse,
};
use crate::domain::account::{
    AuthError, DeviceCode, DevicePoll, MicrosoftAuth, MinecraftLogin, MinecraftProfile,
    MinecraftToken, MsaTokens, Skin, SkinUrl, SkinVariant, Username,
};
use crate::domain::ports::{HttpBody, HttpError, HttpRequest, HttpRequester, HttpResponse, Secret};

pub const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const XBOX_LIVE_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const LOGIN_WITH_XBOX_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
pub const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

const SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const MINECRAFT_RELYING_PARTY: &str = "rp://api.minecraftservices.com/";
const XBOX_LIVE_RELYING_PARTY: &str = "http://xboxlive.com";
const OWNERSHIP_ITEMS: [&str; 2] = ["product_minecraft", "game_minecraft"];
const MAX_SKIN_BYTES: usize = 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// XSTS `XErr` codes.
const XERR_NO_XBOX_ACCOUNT: u64 = 2_148_916_233;
const XERR_COUNTRY_UNAVAILABLE: u64 = 2_148_916_235;
const XERR_CHILD_ACCOUNTS: [u64; 3] = [2_148_916_236, 2_148_916_237, 2_148_916_238];

/// The Microsoft sign-in chain over an [`HttpRequester`].
///
/// `client_id` is the "Application (client) ID" of an Azure app registration
/// that supports personal Microsoft accounts and has "Allow public client
/// flows" turned on (the device code flow needs it). Minecraft Services also
/// has to approve that client id before `login_with_xbox` accepts its tokens:
/// request it at <https://aka.ms/mce-reviewappid>. Until approval,
/// `login_with_xbox` answers HTTP 403 and this adapter reports
/// [`AuthError::AppNotApproved`].
///
/// The Xbox user id (`xuid`) is read from the Minecraft XSTS answer when it
/// carries `xid`. It normally does not, so one more XSTS call is made for the
/// `http://xboxlive.com` relying party, whose answer does, after
/// `login_with_xbox` succeeded.
///
/// Error messages name the step and the HTTP status only; they never carry
/// tokens, codes or response bodies.
pub struct MsaClient<H> {
    http: H,
    client_id: String,
}

/// Which exchange failed, for error messages.
#[derive(Debug, Clone, Copy)]
enum Step {
    DeviceCode,
    Poll,
    Refresh,
    XboxLive,
    Xsts,
    Xuid,
    MinecraftLogin,
    Entitlements,
    Profile,
    Skin,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::DeviceCode => "device code request",
            Self::Poll => "device code poll",
            Self::Refresh => "token refresh",
            Self::XboxLive => "Xbox Live sign-in",
            Self::Xsts => "XSTS authorization",
            Self::Xuid => "Xbox user id lookup",
            Self::MinecraftLogin => "Minecraft login",
            Self::Entitlements => "entitlements check",
            Self::Profile => "profile request",
            Self::Skin => "skin download",
        }
    }
}

fn unexpected(step: Step, status: u16) -> AuthError {
    AuthError::Unexpected(format!(
        "the {} gave an unexpected answer (HTTP {status})",
        step.name()
    ))
}

fn transport(step: Step, error: HttpError) -> AuthError {
    match error {
        // The reason comes from the transport and never holds the body.
        HttpError::Network { reason, .. } => {
            AuthError::Network(format!("{} failed: {reason}", step.name()))
        }
        HttpError::Status { status, .. } => unexpected(step, status),
        HttpError::NotFound(_) => unexpected(step, 404),
        HttpError::RateLimited { .. } => unexpected(step, 429),
    }
}

/// The serde error is dropped on purpose: it can quote the body's values.
fn parse<T: DeserializeOwned>(step: Step, response: &HttpResponse) -> Result<T, AuthError> {
    serde_json::from_slice(&response.body).map_err(|_| unexpected(step, response.status))
}

fn form(fields: &[(&str, &str)]) -> HttpBody {
    HttpBody::Form(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Secret::new(*value)))
            .collect(),
    )
}

fn json_body(document: &serde_json::Value) -> HttpBody {
    HttpBody::Json(Secret::new(document.to_string()))
}

/// The OAuth `error` code of an HTTP 400 answer.
/// An OAuth error answer, named by its `error` code and `AADSTS` numbers.
/// Both are fixed identifiers that never hold tokens; the free-text
/// description stays out because it can echo request details.
fn oauth_failure(step: Step, response: &HttpResponse) -> AuthError {
    let Ok(body) = serde_json::from_slice::<OAuthErrorResponse>(&response.body) else {
        return unexpected(step, response.status);
    };
    let is_identifier = |text: &str| {
        !text.is_empty()
            && text.len() <= 64
            && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let error = if is_identifier(&body.error) {
        body.error
    } else {
        "an unknown error".to_owned()
    };
    let codes: Vec<String> = body
        .error_codes
        .iter()
        .map(|code| format!("AADSTS{code}"))
        .collect();
    AuthError::Unexpected(format!(
        "the {} answered {error}{} (HTTP {})",
        step.name(),
        if codes.is_empty() {
            String::new()
        } else {
            format!(" [{}]", codes.join(", "))
        },
        response.status
    ))
}

fn oauth_error(step: Step, response: &HttpResponse) -> Result<String, AuthError> {
    if response.status != 400 {
        return Err(unexpected(step, response.status));
    }
    parse::<OAuthErrorResponse>(step, response).map(|body| body.error)
}

/// An Xbox user token and its first user's claims.
struct XboxToken {
    token: String,
    user: XboxUserClaims,
}

fn xbox_token(step: Step, response: &HttpResponse) -> Result<XboxToken, AuthError> {
    let body: XboxTokenResponse = parse(step, response)?;
    let user = body
        .display_claims
        .xui
        .into_iter()
        .next()
        .filter(|user| !user.uhs.is_empty())
        .ok_or_else(|| unexpected(step, response.status))?;
    if body.token.is_empty() {
        return Err(unexpected(step, response.status));
    }
    Ok(XboxToken {
        token: body.token,
        user,
    })
}

/// The xuid becomes a game argument, so only plain digits are accepted.
fn valid_xuid(xid: Option<String>) -> Option<String> {
    xid.filter(|xid| !xid.is_empty() && xid.len() <= 32 && xid.bytes().all(|b| b.is_ascii_digit()))
}

fn active_skin(skins: Vec<ProfileSkin>) -> Option<Skin> {
    let skin = skins
        .into_iter()
        .find(|skin| skin.state.as_deref() == Some("ACTIVE"))?;
    let variant = match skin.variant.as_deref()? {
        "CLASSIC" => SkinVariant::Classic,
        "SLIM" => SkinVariant::Slim,
        _ => return None,
    };
    let url = SkinUrl::parse(skin.url.as_deref()?).ok()?;
    Some(Skin { url, variant })
}

/// Mojang ids are 32 hex digits without dashes.
fn profile_id(raw: &str) -> Option<Uuid> {
    let simple = raw.len() == 32 && raw.bytes().all(|b| b.is_ascii_hexdigit());
    simple.then(|| Uuid::try_parse(raw).ok()).flatten()
}

impl<H: HttpRequester> MsaClient<H> {
    pub fn new(http: H, client_id: String) -> Self {
        Self { http, client_id }
    }

    #[cfg(test)]
    pub(super) fn http(&self) -> &H {
        &self.http
    }

    async fn send(&self, step: Step, request: HttpRequest) -> Result<HttpResponse, AuthError> {
        self.http
            .send(request)
            .await
            .map_err(|error| transport(step, error))
    }

    async fn post_token(
        &self,
        step: Step,
        fields: &[(&str, &str)],
    ) -> Result<HttpResponse, AuthError> {
        self.send(step, HttpRequest::post(TOKEN_URL, form(fields)))
            .await
    }

    async fn xbox_live(&self, msa_access_token: &Secret) -> Result<XboxToken, AuthError> {
        let body = json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", msa_access_token.expose()),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        });
        let step = Step::XboxLive;
        let response = self
            .send(step, HttpRequest::post(XBOX_LIVE_URL, json_body(&body)))
            .await?;
        if !response.is_success() {
            return Err(unexpected(step, response.status));
        }
        xbox_token(step, &response)
    }

    async fn xsts(
        &self,
        step: Step,
        xbl_token: &str,
        relying_party: &str,
    ) -> Result<XboxToken, AuthError> {
        let body = json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": relying_party,
            "TokenType": "JWT",
        });
        let response = self
            .send(step, HttpRequest::post(XSTS_URL, json_body(&body)))
            .await?;
        if response.status == 401 {
            let refusal: XstsErrorResponse = parse(step, &response)?;
            return Err(match refusal.xerr {
                XERR_NO_XBOX_ACCOUNT => AuthError::NoXboxAccount,
                XERR_COUNTRY_UNAVAILABLE => AuthError::XboxUnavailableInCountry,
                code if XERR_CHILD_ACCOUNTS.contains(&code) => AuthError::ChildAccount,
                _ => unexpected(step, response.status),
            });
        }
        if !response.is_success() {
            return Err(unexpected(step, response.status));
        }
        xbox_token(step, &response)
    }

    async fn login_with_xbox(&self, xsts: &XboxToken) -> Result<MinecraftToken, AuthError> {
        let body = json!({
            "identityToken": format!("XBL3.0 x={};{}", xsts.user.uhs, xsts.token),
        });
        let step = Step::MinecraftLogin;
        let response = self
            .send(
                step,
                HttpRequest::post(LOGIN_WITH_XBOX_URL, json_body(&body)),
            )
            .await?;
        if response.status == 403 {
            return Err(AuthError::AppNotApproved);
        }
        if !response.is_success() {
            return Err(unexpected(step, response.status));
        }
        let login: MinecraftLoginResponse = parse(step, &response)?;
        if login.access_token.is_empty() {
            return Err(unexpected(step, response.status));
        }
        Ok(MinecraftToken {
            access_token: Secret::new(login.access_token),
            expires_in_secs: login.expires_in,
        })
    }
}

impl<H: HttpRequester> MicrosoftAuth for MsaClient<H> {
    async fn start_device_code(&self) -> Result<DeviceCode, AuthError> {
        let step = Step::DeviceCode;
        let fields = [("client_id", self.client_id.as_str()), ("scope", SCOPE)];
        let response = self
            .send(step, HttpRequest::post(DEVICE_CODE_URL, form(&fields)))
            .await?;
        if !response.is_success() {
            return Err(oauth_failure(step, &response));
        }
        let code: DeviceCodeResponse = parse(step, &response)?;
        Ok(DeviceCode {
            user_code: code.user_code,
            verification_uri: code.verification_uri,
            device_code: Secret::new(code.device_code),
            interval_secs: code.interval,
            expires_in_secs: code.expires_in,
        })
    }

    async fn poll_device_code(&self, device_code: &Secret) -> Result<DevicePoll, AuthError> {
        let step = Step::Poll;
        let response = self
            .post_token(
                step,
                &[
                    ("grant_type", DEVICE_CODE_GRANT),
                    ("client_id", &self.client_id),
                    ("device_code", device_code.expose()),
                ],
            )
            .await?;
        if response.is_success() {
            let tokens: TokenResponse = parse(step, &response)?;
            let refresh_token = tokens
                .refresh_token
                .ok_or_else(|| unexpected(step, response.status))?;
            return Ok(DevicePoll::Granted(MsaTokens {
                access_token: Secret::new(tokens.access_token),
                refresh_token: Secret::new(refresh_token),
            }));
        }
        match oauth_error(step, &response)?.as_str() {
            "authorization_pending" => Ok(DevicePoll::Pending),
            "slow_down" => Ok(DevicePoll::SlowDown),
            "expired_token" => Err(AuthError::CodeExpired),
            "authorization_declined" => Err(AuthError::Declined),
            _ => Err(oauth_failure(step, &response)),
        }
    }

    /// When Microsoft does not rotate the refresh token, the old one stays valid.
    async fn refresh(&self, refresh_token: &Secret) -> Result<MsaTokens, AuthError> {
        let step = Step::Refresh;
        let response = self
            .post_token(
                step,
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &self.client_id),
                    ("refresh_token", refresh_token.expose()),
                    ("scope", SCOPE),
                ],
            )
            .await?;
        if response.is_success() {
            let tokens: TokenResponse = parse(step, &response)?;
            return Ok(MsaTokens {
                access_token: Secret::new(tokens.access_token),
                refresh_token: tokens
                    .refresh_token
                    .map_or_else(|| refresh_token.clone(), Secret::new),
            });
        }
        match oauth_error(step, &response)?.as_str() {
            "invalid_grant" => Err(AuthError::SessionExpired),
            _ => Err(oauth_failure(step, &response)),
        }
    }

    async fn minecraft_login(
        &self,
        msa_access_token: &Secret,
    ) -> Result<MinecraftLogin, AuthError> {
        let xbl = self.xbox_live(msa_access_token).await?;
        let xsts = self
            .xsts(Step::Xsts, &xbl.token, MINECRAFT_RELYING_PARTY)
            .await?;
        let token = self.login_with_xbox(&xsts).await?;
        let xuid = match xsts.user.xid {
            Some(xid) => valid_xuid(Some(xid)),
            None => match self
                .xsts(Step::Xuid, &xbl.token, XBOX_LIVE_RELYING_PARTY)
                .await
            {
                Ok(xbox) => valid_xuid(xbox.user.xid),
                Err(error) => {
                    tracing::warn!(%error, "could not look up the Xbox user id");
                    None
                }
            },
        };
        // The game only forwards the xuid to Mojang's telemetry and accepts
        // "0", so a missing one must not fail an otherwise complete sign-in.
        let xuid = xuid.unwrap_or_else(|| "0".to_owned());
        Ok(MinecraftLogin { token, xuid })
    }

    async fn owns_game(&self, minecraft_token: &Secret) -> Result<bool, AuthError> {
        let step = Step::Entitlements;
        let response = self
            .send(
                step,
                HttpRequest::get(ENTITLEMENTS_URL).with_bearer(minecraft_token),
            )
            .await?;
        if !response.is_success() {
            return Err(unexpected(step, response.status));
        }
        let entitlements: EntitlementsResponse = parse(step, &response)?;
        Ok(entitlements
            .items
            .iter()
            .any(|item| OWNERSHIP_ITEMS.contains(&item.name.as_str())))
    }

    async fn profile(&self, minecraft_token: &Secret) -> Result<MinecraftProfile, AuthError> {
        let step = Step::Profile;
        let response = self
            .send(
                step,
                HttpRequest::get(PROFILE_URL).with_bearer(minecraft_token),
            )
            .await?;
        if response.status == 404 {
            return Err(AuthError::NoProfile);
        }
        if !response.is_success() {
            return Err(unexpected(step, response.status));
        }
        let profile: ProfileResponse = parse(step, &response)?;
        let id = profile_id(&profile.id).ok_or_else(|| unexpected(step, response.status))?;
        let name = Username::parse(&profile.name).map_err(|_| unexpected(step, response.status))?;
        Ok(MinecraftProfile {
            id,
            name,
            skin: active_skin(profile.skins),
        })
    }

    async fn skin_texture(&self, url: &SkinUrl) -> Result<Vec<u8>, AuthError> {
        let step = Step::Skin;
        let response = self.send(step, HttpRequest::get(url.as_str())).await?;
        let png = response.body.starts_with(&PNG_SIGNATURE);
        if !response.is_success() || !png || response.body.len() > MAX_SKIN_BYTES {
            return Err(unexpected(step, response.status));
        }
        Ok(response.body)
    }
}
