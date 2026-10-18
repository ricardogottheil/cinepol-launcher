#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use uuid::Uuid;

use super::client::{
    DEVICE_CODE_URL, ENTITLEMENTS_URL, LOGIN_WITH_XBOX_URL, PROFILE_URL, TOKEN_URL, XBOX_LIVE_URL,
    XSTS_URL,
};
use super::*;
use crate::domain::account::{
    AuthError, DevicePoll, MicrosoftAuth, MsaTokens, SkinUrl, SkinVariant, Username,
};
use crate::domain::ports::{HttpBody, HttpMethod, HttpRequest, Secret};
use crate::testing::FakeHttpRequester;

const CLIENT_ID: &str = "00000000-0000-0000-0000-00000000c1d0";
const SCOPE: &str = "XboxLive.signin offline_access";
const SKIN: &str = "https://textures.minecraft.net/texture/3b60a1f6d562f52aaebbf1434f1de147933a3affe0e764fa49ea057536623cd3";

/// Every token-like value the fixtures and tests use. None may ever appear in
/// an error message.
const SECRETS: &[&str] = &[
    "FAKE-device-code-AAAABBBBCCCC",
    "FAKE-msa-access-token",
    "FAKE-msa-refresh-token",
    "FAKE-xbl-user-token",
    "FAKE-xsts-minecraft-token",
    "FAKE-xsts-xboxlive-token",
    "FAKE-minecraft-access-token",
    "FAKE.signature",
    "1234567890123456789",
];

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../tests/fixtures/msa/", $name))
    };
}

fn client(http: FakeHttpRequester) -> MsaClient<FakeHttpRequester> {
    MsaClient::new(http, CLIENT_ID.to_owned())
}

fn secret(value: &str) -> Secret {
    Secret::new(value)
}

fn form(request: &HttpRequest) -> Vec<(String, String)> {
    match &request.body {
        Some(HttpBody::Form(fields)) => fields
            .iter()
            .map(|(name, value)| (name.clone(), value.expose().to_owned()))
            .collect(),
        other => panic!("expected a form body, got {other:?}"),
    }
}

fn json_body(request: &HttpRequest) -> Value {
    match &request.body {
        Some(HttpBody::Json(document)) => serde_json::from_str(document.expose()).unwrap(),
        other => panic!("expected a JSON body, got {other:?}"),
    }
}

fn pairs(fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn assert_no_secret(error: &AuthError) {
    let text = error.to_string();
    for secret in SECRETS {
        assert!(!text.contains(secret), "{text:?} leaks {secret}");
    }
}

fn assert_unexpected(error: AuthError, step: &str) {
    assert_no_secret(&error);
    match &error {
        AuthError::Unexpected(text) => assert!(text.contains(step), "{text:?} names {step}"),
        other => panic!("expected Unexpected, got {other:?}"),
    }
}

// --- Device code ----------------------------------------------------------

#[tokio::test]
async fn starts_the_device_code_flow() {
    let http =
        FakeHttpRequester::default().reply(DEVICE_CODE_URL, 200, fixture!("device-code.json"));
    let msa = client(http);
    let code = msa.start_device_code().await.unwrap();
    assert_eq!(code.user_code, "FAKE1234");
    assert_eq!(code.verification_uri, "https://www.microsoft.com/link");
    assert_eq!(code.device_code, secret("FAKE-device-code-AAAABBBBCCCC"));
    assert_eq!(code.interval_secs, 5);
    assert_eq!(code.expires_in_secs, 900);

    let requests = msa.http().requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert_eq!(requests[0].url, DEVICE_CODE_URL);
    assert_eq!(requests[0].bearer, None);
    assert_eq!(
        form(&requests[0]),
        pairs(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
    );
}

#[tokio::test]
async fn a_refused_device_code_request_is_unexpected() {
    let http = FakeHttpRequester::default().reply(
        DEVICE_CODE_URL,
        400,
        r#"{"error":"invalid_client","error_description":"AADSTS700016"}"#,
    );
    let error = client(http).start_device_code().await.unwrap_err();
    assert_unexpected(error, "device code");
}

// --- Device code poll -----------------------------------------------------

#[tokio::test]
async fn a_granted_poll_returns_the_tokens() {
    let http = FakeHttpRequester::default().reply(TOKEN_URL, 200, fixture!("token.json"));
    let msa = client(http);
    let poll = msa
        .poll_device_code(&secret("FAKE-device-code-AAAABBBBCCCC"))
        .await
        .unwrap();
    assert_eq!(
        poll,
        DevicePoll::Granted(MsaTokens {
            access_token: secret("FAKE-msa-access-token"),
            refresh_token: secret("FAKE-msa-refresh-token"),
        })
    );
    let requests = msa.http().requests();
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert_eq!(requests[0].url, TOKEN_URL);
    assert_eq!(
        form(&requests[0]),
        pairs(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", CLIENT_ID),
            ("device_code", "FAKE-device-code-AAAABBBBCCCC"),
        ])
    );
}

#[tokio::test]
async fn poll_answers_map_to_pending_slow_down_expired_and_declined() {
    let cases = [
        (
            fixture!("token-authorization-pending.json"),
            Ok(DevicePoll::Pending),
        ),
        (fixture!("token-slow-down.json"), Ok(DevicePoll::SlowDown)),
        (
            fixture!("token-expired-token.json"),
            Err(AuthError::CodeExpired),
        ),
        (
            fixture!("token-authorization-declined.json"),
            Err(AuthError::Declined),
        ),
    ];
    for (body, expected) in cases {
        let http = FakeHttpRequester::default().reply(TOKEN_URL, 400, body);
        let result = client(http)
            .poll_device_code(&secret("FAKE-device-code-AAAABBBBCCCC"))
            .await;
        assert_eq!(result, expected, "{body}");
    }
}

#[tokio::test]
async fn other_poll_errors_are_unexpected() {
    for (status, body) in [
        (400, r#"{"error":"bad_verification_code"}"#),
        (400, "not json"),
        (500, "oops"),
        (
            200,
            r#"{"token_type":"Bearer","access_token":"FAKE-msa-access-token"}"#,
        ),
    ] {
        let http = FakeHttpRequester::default().reply(TOKEN_URL, status, body);
        let error = client(http)
            .poll_device_code(&secret("FAKE-device-code-AAAABBBBCCCC"))
            .await
            .unwrap_err();
        assert_unexpected(error, "device code poll");
    }
}

// --- Refresh --------------------------------------------------------------

#[tokio::test]
async fn refreshes_the_microsoft_tokens() {
    let http = FakeHttpRequester::default().reply(TOKEN_URL, 200, fixture!("token.json"));
    let msa = client(http);
    let tokens = msa
        .refresh(&secret("FAKE-old-refresh-token"))
        .await
        .unwrap();
    assert_eq!(tokens.access_token, secret("FAKE-msa-access-token"));
    assert_eq!(tokens.refresh_token, secret("FAKE-msa-refresh-token"));
    let requests = msa.http().requests();
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert_eq!(requests[0].url, TOKEN_URL);
    assert_eq!(
        form(&requests[0]),
        pairs(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", "FAKE-old-refresh-token"),
            ("scope", SCOPE),
        ])
    );
}

#[tokio::test]
async fn a_refresh_without_a_new_refresh_token_keeps_the_old_one() {
    let http = FakeHttpRequester::default().reply(
        TOKEN_URL,
        200,
        r#"{"token_type":"Bearer","access_token":"FAKE-msa-access-token"}"#,
    );
    let tokens = client(http)
        .refresh(&secret("FAKE-old-refresh-token"))
        .await
        .unwrap();
    assert_eq!(tokens.refresh_token, secret("FAKE-old-refresh-token"));
}

#[tokio::test]
async fn an_invalid_grant_means_the_session_expired() {
    let http =
        FakeHttpRequester::default().reply(TOKEN_URL, 400, fixture!("token-invalid-grant.json"));
    assert_eq!(
        client(http)
            .refresh(&secret("FAKE-old-refresh-token"))
            .await,
        Err(AuthError::SessionExpired)
    );
}

#[tokio::test]
async fn other_refresh_errors_are_unexpected() {
    let http = FakeHttpRequester::default().reply(TOKEN_URL, 400, r#"{"error":"invalid_client"}"#);
    let error = client(http)
        .refresh(&secret("FAKE-msa-refresh-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "token refresh");
}

// --- Xbox Live, XSTS and login_with_xbox ----------------------------------

fn signed_in_http() -> FakeHttpRequester {
    FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 200, fixture!("xsts-minecraft.json"))
        .reply(LOGIN_WITH_XBOX_URL, 200, fixture!("login-with-xbox.json"))
        .reply(XSTS_URL, 200, fixture!("xsts-xboxlive.json"))
}

#[tokio::test]
async fn exchanges_the_microsoft_token_for_a_minecraft_token() {
    let msa = client(signed_in_http());
    let login = msa
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap();
    assert_eq!(
        login.token.access_token,
        secret("FAKE-minecraft-access-token")
    );
    assert_eq!(login.token.expires_in_secs, 86_400);
    assert_eq!(login.xuid, "2535400000000000");

    let requests = msa.http().requests();
    let urls: Vec<_> = requests.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(
        urls,
        [XBOX_LIVE_URL, XSTS_URL, LOGIN_WITH_XBOX_URL, XSTS_URL]
    );
    assert!(requests.iter().all(|r| r.method == HttpMethod::Post));
    assert!(requests.iter().all(|r| r.bearer.is_none()));
    assert_eq!(
        json_body(&requests[0]),
        json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": "d=FAKE-msa-access-token"
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        })
    );
    assert_eq!(
        json_body(&requests[1]),
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": ["FAKE-xbl-user-token"] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        })
    );
    assert_eq!(
        json_body(&requests[2]),
        json!({ "identityToken": "XBL3.0 x=1234567890123456789;FAKE-xsts-minecraft-token" })
    );
    assert_eq!(
        json_body(&requests[3]),
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": ["FAKE-xbl-user-token"] },
            "RelyingParty": "http://xboxlive.com",
            "TokenType": "JWT"
        })
    );
}

#[tokio::test]
async fn an_xid_in_the_minecraft_xsts_answer_skips_the_extra_call() {
    let xsts = r#"{"Token":"FAKE-xsts-minecraft-token","DisplayClaims":{"xui":[{"uhs":"1234567890123456789","xid":"2535411111111111"}]}}"#;
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 200, xsts)
        .reply(LOGIN_WITH_XBOX_URL, 200, fixture!("login-with-xbox.json"));
    let msa = client(http);
    let login = msa
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap();
    assert_eq!(login.xuid, "2535411111111111");
    assert_eq!(msa.http().requests().len(), 3);
}

#[tokio::test]
async fn xsts_refusals_map_to_account_errors() {
    let cases = [
        (2_148_916_233_u64, AuthError::NoXboxAccount),
        (2_148_916_235, AuthError::XboxUnavailableInCountry),
        (2_148_916_236, AuthError::ChildAccount),
        (2_148_916_237, AuthError::ChildAccount),
        (2_148_916_238, AuthError::ChildAccount),
    ];
    for (xerr, expected) in cases {
        let body = format!(r#"{{"Identity":"0","XErr":{xerr},"Message":"","Redirect":""}}"#);
        let http = FakeHttpRequester::default()
            .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
            .reply(XSTS_URL, 401, body);
        let result = client(http)
            .minecraft_login(&secret("FAKE-msa-access-token"))
            .await;
        assert_eq!(result, Err(expected), "XErr {xerr}");
    }
}

#[tokio::test]
async fn xsts_refusal_fixtures_parse() {
    for (body, expected) in [
        (
            fixture!("xsts-no-xbox-account.json"),
            AuthError::NoXboxAccount,
        ),
        (fixture!("xsts-child-account.json"), AuthError::ChildAccount),
    ] {
        let http = FakeHttpRequester::default()
            .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
            .reply(XSTS_URL, 401, body);
        let result = client(http)
            .minecraft_login(&secret("FAKE-msa-access-token"))
            .await;
        assert_eq!(result, Err(expected));
    }
}

#[tokio::test]
async fn an_unknown_xsts_refusal_is_unexpected() {
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 401, r#"{"XErr":2148916239}"#);
    let error = client(http)
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "XSTS");
}

#[tokio::test]
async fn an_xbox_live_refusal_is_unexpected() {
    let http = FakeHttpRequester::default().reply(XBOX_LIVE_URL, 400, "");
    let error = client(http)
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "Xbox Live");
}

#[tokio::test]
async fn an_unapproved_app_registration_is_reported() {
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 200, fixture!("xsts-minecraft.json"))
        .reply(
            LOGIN_WITH_XBOX_URL,
            403,
            fixture!("login-with-xbox-invalid-app.json"),
        );
    assert_eq!(
        client(http)
            .minecraft_login(&secret("FAKE-msa-access-token"))
            .await,
        Err(AuthError::AppNotApproved)
    );
}

#[tokio::test]
async fn malformed_minecraft_logins_are_unexpected_without_leaking_tokens() {
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 200, fixture!("xsts-minecraft.json"))
        .reply(
            LOGIN_WITH_XBOX_URL,
            200,
            r#"{"access_token":"FAKE-minecraft-access-token","expires_in":"FAKE-minecraft-access-token"}"#,
        );
    let error = client(http)
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "Minecraft login");
}

#[tokio::test]
async fn a_missing_invalid_or_unreachable_xuid_falls_back_to_zero() {
    for xboxlive in [
        Some(
            r#"{"Token":"FAKE-xsts-xboxlive-token","DisplayClaims":{"xui":[{"uhs":"1234567890123456789"}]}}"#,
        ),
        Some(
            r#"{"Token":"FAKE-xsts-xboxlive-token","DisplayClaims":{"xui":[{"uhs":"1","xid":"12 -Dx=y"}]}}"#,
        ),
        // No answer at all: the extra lookup fails at the transport.
        None,
    ] {
        let mut http = FakeHttpRequester::default()
            .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
            .reply(XSTS_URL, 200, fixture!("xsts-minecraft.json"))
            .reply(LOGIN_WITH_XBOX_URL, 200, fixture!("login-with-xbox.json"));
        if let Some(body) = xboxlive {
            http = http.reply(XSTS_URL, 200, body);
        }
        let login = client(http)
            .minecraft_login(&secret("FAKE-msa-access-token"))
            .await
            .unwrap();
        assert_eq!(login.xuid, "0", "the game accepts a zero xuid");
    }
}

#[tokio::test]
async fn an_xsts_answer_without_users_is_unexpected() {
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(
            XSTS_URL,
            200,
            r#"{"Token":"FAKE-xsts-minecraft-token","DisplayClaims":{"xui":[]}}"#,
        );
    let error = client(http)
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "XSTS");
}

// --- Entitlements ---------------------------------------------------------

#[tokio::test]
async fn ownership_comes_from_the_entitlements() {
    let http = FakeHttpRequester::default()
        .reply(ENTITLEMENTS_URL, 200, fixture!("entitlements.json"))
        .reply(ENTITLEMENTS_URL, 200, fixture!("entitlements-empty.json"))
        .reply(
            ENTITLEMENTS_URL,
            200,
            r#"{"items":[{"name":"game_minecraft"}]}"#,
        )
        .reply(
            ENTITLEMENTS_URL,
            200,
            r#"{"items":[{"name":"game_minecraft_bedrock"}]}"#,
        );
    let msa = client(http);
    let token = secret("FAKE-minecraft-access-token");
    assert!(msa.owns_game(&token).await.unwrap());
    assert!(!msa.owns_game(&token).await.unwrap());
    assert!(msa.owns_game(&token).await.unwrap());
    assert!(!msa.owns_game(&token).await.unwrap());
    let request = &msa.http().requests()[0];
    assert_eq!(request.method, HttpMethod::Get);
    assert_eq!(request.url, ENTITLEMENTS_URL);
    assert_eq!(request.body, None);
    assert_eq!(request.bearer, Some(token));
}

#[tokio::test]
async fn an_entitlements_failure_is_unexpected() {
    let http = FakeHttpRequester::default().reply(ENTITLEMENTS_URL, 401, "");
    let error = client(http)
        .owns_game(&secret("FAKE-minecraft-access-token"))
        .await
        .unwrap_err();
    assert_unexpected(error, "entitlements");
}

// --- Profile --------------------------------------------------------------

#[tokio::test]
async fn reads_the_profile_and_its_active_skin() {
    let http = FakeHttpRequester::default().reply(PROFILE_URL, 200, fixture!("profile.json"));
    let msa = client(http);
    let token = secret("FAKE-minecraft-access-token");
    let profile = msa.profile(&token).await.unwrap();
    assert_eq!(
        profile.id,
        Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap()
    );
    assert_eq!(profile.name, Username::parse("FakePlayer_01").unwrap());
    let skin = profile.skin.unwrap();
    assert_eq!(skin.url.as_str(), SKIN);
    assert_eq!(skin.variant, SkinVariant::Slim);
    let request = &msa.http().requests()[0];
    assert_eq!(request.method, HttpMethod::Get);
    assert_eq!(request.url, PROFILE_URL);
    assert_eq!(request.bearer, Some(token));
}

#[tokio::test]
async fn a_profile_without_a_readable_active_skin_has_none() {
    for skins in [
        r#"[]"#,
        r#"[{"state":"INACTIVE","url":"http://textures.minecraft.net/texture/ab","variant":"CLASSIC"}]"#,
        r#"[{"state":"ACTIVE","url":"https://evil.example/texture/ab","variant":"CLASSIC"}]"#,
        r#"[{"state":"ACTIVE","url":"http://textures.minecraft.net/texture/ab","variant":"WIDE"}]"#,
    ] {
        let body = format!(
            r#"{{"id":"0123456789abcdef0123456789abcdef","name":"Steve","skins":{skins}}}"#
        );
        let http = FakeHttpRequester::default().reply(PROFILE_URL, 200, body);
        let profile = client(http)
            .profile(&secret("FAKE-minecraft-access-token"))
            .await
            .unwrap();
        assert_eq!(profile.skin, None, "{skins}");
    }
}

#[tokio::test]
async fn a_classic_skin_is_read_as_classic() {
    let body = r#"{"id":"0123456789abcdef0123456789abcdef","name":"Steve","skins":[{"state":"ACTIVE","url":"http://textures.minecraft.net/texture/ab","variant":"CLASSIC"}]}"#;
    let http = FakeHttpRequester::default().reply(PROFILE_URL, 200, body);
    let profile = client(http)
        .profile(&secret("FAKE-minecraft-access-token"))
        .await
        .unwrap();
    assert_eq!(profile.skin.unwrap().variant, SkinVariant::Classic);
}

#[tokio::test]
async fn a_missing_profile_is_reported() {
    let http =
        FakeHttpRequester::default().reply(PROFILE_URL, 404, fixture!("profile-not-found.json"));
    assert_eq!(
        client(http)
            .profile(&secret("FAKE-minecraft-access-token"))
            .await,
        Err(AuthError::NoProfile)
    );
}

#[tokio::test]
async fn invalid_profiles_are_unexpected() {
    for body in [
        r#"{"id":"01234567-89ab-cdef-0123-456789abcdef","name":"Steve"}"#,
        r#"{"id":"0123456789abcdef0123456789abcdeg","name":"Steve"}"#,
        r#"{"id":"0123456789abcdef0123456789abcdef","name":"no spaces allowed"}"#,
        r#"{"name":"Steve"}"#,
    ] {
        let http = FakeHttpRequester::default().reply(PROFILE_URL, 200, body);
        let error = client(http)
            .profile(&secret("FAKE-minecraft-access-token"))
            .await
            .unwrap_err();
        assert_unexpected(error, "profile");
    }
}

// --- Skin texture ---------------------------------------------------------

const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn png(len: usize) -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.resize(len, 0);
    bytes
}

#[tokio::test]
async fn downloads_a_png_skin_without_the_bearer() {
    let http = FakeHttpRequester::default().reply(SKIN, 200, png(64));
    let msa = client(http);
    let bytes = msa
        .skin_texture(&SkinUrl::parse(SKIN).unwrap())
        .await
        .unwrap();
    assert_eq!(bytes, png(64));
    let request = &msa.http().requests()[0];
    assert_eq!(request.method, HttpMethod::Get);
    assert_eq!(request.url, SKIN);
    assert_eq!(request.bearer, None);
    assert_eq!(request.body, None);
}

#[tokio::test]
async fn rejects_skins_that_are_not_small_pngs() {
    for (status, body) in [
        (200, b"<html>not a png</html>".to_vec()),
        (200, png(1024 * 1024 + 1)),
        (404, b"".to_vec()),
    ] {
        let http = FakeHttpRequester::default().reply(SKIN, status, body);
        let error = client(http)
            .skin_texture(&SkinUrl::parse(SKIN).unwrap())
            .await
            .unwrap_err();
        assert_unexpected(error, "skin");
    }
}

#[tokio::test]
async fn a_one_mebibyte_png_is_accepted() {
    let http = FakeHttpRequester::default().reply(SKIN, 200, png(1024 * 1024));
    assert!(
        client(http)
            .skin_texture(&SkinUrl::parse(SKIN).unwrap())
            .await
            .is_ok()
    );
}

// --- Transport failures ---------------------------------------------------

#[tokio::test]
async fn transport_failures_are_network_errors_without_tokens() {
    // No scripted reply: the fake answers with a transport failure.
    let msa = client(FakeHttpRequester::default());
    let token = secret("FAKE-minecraft-access-token");
    let errors = [
        msa.start_device_code().await.unwrap_err(),
        msa.poll_device_code(&secret("FAKE-device-code-AAAABBBBCCCC"))
            .await
            .unwrap_err(),
        msa.refresh(&secret("FAKE-msa-refresh-token"))
            .await
            .unwrap_err(),
        msa.minecraft_login(&secret("FAKE-msa-access-token"))
            .await
            .unwrap_err(),
        msa.owns_game(&token).await.unwrap_err(),
        msa.profile(&token).await.unwrap_err(),
        msa.skin_texture(&SkinUrl::parse(SKIN).unwrap())
            .await
            .unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, AuthError::Network(_)), "{error:?}");
        assert_no_secret(&error);
    }
}

#[tokio::test]
async fn later_steps_report_network_errors_too() {
    let http = FakeHttpRequester::default()
        .reply(XBOX_LIVE_URL, 200, fixture!("xbl-authenticate.json"))
        .reply(XSTS_URL, 200, fixture!("xsts-minecraft.json"));
    let error = client(http)
        .minecraft_login(&secret("FAKE-msa-access-token"))
        .await
        .unwrap_err();
    assert!(matches!(error, AuthError::Network(_)), "{error:?}");
    assert_no_secret(&error);
}
