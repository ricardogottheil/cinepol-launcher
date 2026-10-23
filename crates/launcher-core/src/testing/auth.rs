//! A scripted Microsoft sign-in: every step answers what the test set.

use std::collections::VecDeque;
use std::sync::Mutex;

use uuid::Uuid;

use crate::domain::account::{
    AuthError, CapeId, DeviceCode, DevicePoll, MicrosoftAuth, MinecraftLogin, MinecraftProfile,
    MinecraftToken, MsaTokens, Skin, SkinPng, SkinUrl, SkinVariant, Username,
};
use crate::domain::ports::Secret;

/// Answers each step of the chain from its field and records the calls in order.
#[derive(Debug)]
pub struct FakeMicrosoftAuth {
    pub device_code: Result<DeviceCode, AuthError>,
    /// Answered in order; once empty, every poll is `Pending`.
    pub polls: Mutex<VecDeque<Result<DevicePoll, AuthError>>>,
    pub refresh: Result<MsaTokens, AuthError>,
    pub login: Result<MinecraftLogin, AuthError>,
    pub owns_game: Result<bool, AuthError>,
    pub profile: Result<MinecraftProfile, AuthError>,
    pub skin: Result<Vec<u8>, AuthError>,
    /// The answer of a skin upload and of a cape change.
    pub change: Result<MinecraftProfile, AuthError>,
    calls: Mutex<Vec<String>>,
}

pub fn sample_profile() -> MinecraftProfile {
    MinecraftProfile {
        id: Uuid::from_u128(0xd13a_9ff1_3e60_4163_a6e9_35d7_9f68_58ed),
        name: Username::parse("Polispol1").unwrap(),
        skin: Some(Skin {
            url: SkinUrl::parse("http://textures.minecraft.net/texture/abc123").unwrap(),
            variant: SkinVariant::Classic,
        }),
        capes: Vec::new(),
    }
}

/// Microsoft tokens of one sign-in or refresh "generation".
pub fn tokens(generation: u32) -> MsaTokens {
    MsaTokens {
        access_token: Secret::new(format!("msa-access-{generation}")),
        refresh_token: Secret::new(format!("msa-refresh-{generation}")),
    }
}

impl FakeMicrosoftAuth {
    /// A player who approves on the first poll and owns the game.
    pub fn approving() -> Self {
        Self {
            device_code: Ok(DeviceCode {
                user_code: "ABCD-EFGH".into(),
                verification_uri: "https://www.microsoft.com/link".into(),
                device_code: Secret::new("device-code"),
                interval_secs: 5,
                expires_in_secs: 900,
            }),
            polls: Mutex::new(VecDeque::from([Ok(DevicePoll::Granted(tokens(1)))])),
            refresh: Ok(tokens(2)),
            login: Ok(MinecraftLogin {
                token: MinecraftToken {
                    access_token: Secret::new("minecraft-token"),
                    expires_in_secs: 86_400,
                },
                xuid: "2535428".into(),
            }),
            owns_game: Ok(true),
            profile: Ok(sample_profile()),
            skin: Ok(b"\x89PNG".to_vec()),
            change: Ok(sample_profile()),
            calls: Mutex::default(),
        }
    }

    pub fn with_polls(self, polls: Vec<Result<DevicePoll, AuthError>>) -> Self {
        *self.polls.lock().unwrap() = polls.into();
        self
    }

    /// Steps called so far, with the token each one received.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl MicrosoftAuth for FakeMicrosoftAuth {
    async fn start_device_code(&self) -> Result<DeviceCode, AuthError> {
        self.record("start".into());
        self.device_code.clone()
    }

    async fn poll_device_code(&self, device_code: &Secret) -> Result<DevicePoll, AuthError> {
        self.record(format!("poll:{}", device_code.expose()));
        self.polls
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(DevicePoll::Pending))
    }

    async fn refresh(&self, refresh_token: &Secret) -> Result<MsaTokens, AuthError> {
        self.record(format!("refresh:{}", refresh_token.expose()));
        self.refresh.clone()
    }

    async fn minecraft_login(
        &self,
        msa_access_token: &Secret,
    ) -> Result<MinecraftLogin, AuthError> {
        self.record(format!("login:{}", msa_access_token.expose()));
        self.login.clone()
    }

    async fn owns_game(&self, minecraft_token: &Secret) -> Result<bool, AuthError> {
        self.record(format!("owns:{}", minecraft_token.expose()));
        self.owns_game.clone()
    }

    async fn profile(&self, minecraft_token: &Secret) -> Result<MinecraftProfile, AuthError> {
        self.record(format!("profile:{}", minecraft_token.expose()));
        self.profile.clone()
    }

    async fn skin_texture(&self, url: &SkinUrl) -> Result<Vec<u8>, AuthError> {
        self.record(format!("skin:{}", url.as_str()));
        self.skin.clone()
    }

    async fn upload_skin(
        &self,
        minecraft_token: &Secret,
        skin: &SkinPng,
        variant: SkinVariant,
    ) -> Result<MinecraftProfile, AuthError> {
        self.record(format!(
            "upload:{}:{variant:?}:{}",
            minecraft_token.expose(),
            skin.as_bytes().len()
        ));
        self.change.clone()
    }

    async fn set_cape(
        &self,
        minecraft_token: &Secret,
        cape: Option<&CapeId>,
    ) -> Result<MinecraftProfile, AuthError> {
        let cape = cape.map_or("none", CapeId::as_str);
        self.record(format!("cape:{}:{cape}", minecraft_token.expose()));
        self.change.clone()
    }
}

/// Answers a fixed game token, or an expired session when there is none;
/// records which accounts asked.
#[derive(Debug, Default)]
pub struct FakeSessions {
    pub token: Option<&'static str>,
    asked: Mutex<Vec<crate::domain::account::AccountId>>,
}

impl FakeSessions {
    pub fn with_token(token: &'static str) -> Self {
        Self {
            token: Some(token),
            ..Self::default()
        }
    }

    pub fn asked(&self) -> Vec<crate::domain::account::AccountId> {
        self.asked.lock().unwrap().clone()
    }
}

impl crate::application::refresh_session::PremiumSessions for FakeSessions {
    async fn session_token(
        &self,
        account: &crate::domain::account::Account,
    ) -> Result<Secret, crate::application::refresh_session::SessionError> {
        self.asked.lock().unwrap().push(account.id);
        self.token
            .map(Secret::new)
            .ok_or(crate::application::refresh_session::SessionError::Expired)
    }
}
