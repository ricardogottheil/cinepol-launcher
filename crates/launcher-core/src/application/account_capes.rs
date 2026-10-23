//! The capes a Microsoft account owns, with their textures.

use thiserror::Error;

use super::refresh_session::{PremiumSessions, SessionError};
use crate::domain::account::{
    Account, AccountId, AccountKind, AccountRepository, AccountRepositoryError, CapeId,
    MicrosoftAuth,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountCapesError {
    #[error("no account with that id")]
    NotFound,
    #[error("offline profiles have no capes")]
    NotMicrosoft,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
}

/// A cape and its texture. `png` is `None` when the texture did not download:
/// the cape can still be worn, it just has no picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapeTexture {
    pub id: CapeId,
    pub alias: String,
    pub active: bool,
    pub png: Option<Vec<u8>>,
}

/// The account behind `id`, when it is a Microsoft one.
pub(crate) async fn microsoft_account<A: AccountRepository>(
    id: AccountId,
    accounts: &A,
) -> Result<Account, AccountCapesError> {
    let account = accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or(AccountCapesError::NotFound)?;
    match account.kind {
        AccountKind::Microsoft(_) => Ok(account),
        AccountKind::Offline => Err(AccountCapesError::NotMicrosoft),
    }
}

/// Read live from Mojang every time: capes are never stored.
pub async fn account_capes<A, Q, M>(
    id: AccountId,
    accounts: &A,
    sessions: &Q,
    auth: &M,
) -> Result<Vec<CapeTexture>, AccountCapesError>
where
    A: AccountRepository,
    Q: PremiumSessions,
    M: MicrosoftAuth,
{
    let account = microsoft_account(id, accounts).await?;
    let token = sessions.session_token(&account).await?;
    let profile = auth.profile(&token).await.map_err(SessionError::from)?;
    let mut capes = Vec::with_capacity(profile.capes.len());
    for cape in profile.capes {
        let png = match auth.skin_texture(&cape.url).await {
            Ok(png) => Some(png),
            Err(error) => {
                tracing::warn!(%error, cape = %cape.alias, "could not download a cape texture");
                None
            }
        };
        capes.push(CapeTexture {
            id: cape.id,
            alias: cape.alias,
            active: cape.active,
            png,
        });
    }
    Ok(capes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{AuthError, Cape, SkinUrl, Username};
    use crate::testing::{
        FakeMicrosoftAuth, FakeSessions, InMemoryAccountRepository, sample_profile,
    };

    const MIGRATOR: &str = "1981aad3-73bd-4d8b-a8d5-4dd0f5bc8e1c";

    fn premium() -> Account {
        Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428")
    }

    fn cape(id: &str, alias: &str, texture: &str, active: bool) -> Cape {
        Cape {
            id: CapeId::parse(id).unwrap(),
            alias: alias.into(),
            url: SkinUrl::parse(&format!("http://textures.minecraft.net/texture/{texture}"))
                .unwrap(),
            active,
        }
    }

    #[tokio::test]
    async fn lists_the_owned_capes_with_their_textures() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        let mut profile = sample_profile();
        profile.capes = vec![cape(MIGRATOR, "Migrator", "aa", true)];
        auth.profile = Ok(profile);

        let capes = account_capes(account.id, &accounts, &sessions, &auth)
            .await
            .unwrap();

        assert_eq!(
            capes,
            [CapeTexture {
                id: CapeId::parse(MIGRATOR).unwrap(),
                alias: "Migrator".into(),
                active: true,
                png: Some(b"\x89PNG".to_vec()),
            }]
        );
        assert_eq!(
            auth.calls(),
            [
                "profile:mc-token",
                "skin:https://textures.minecraft.net/texture/aa"
            ]
        );
    }

    #[tokio::test]
    async fn a_cape_without_its_texture_is_still_listed() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        let mut profile = sample_profile();
        profile.capes = vec![cape(MIGRATOR, "Migrator", "aa", false)];
        auth.profile = Ok(profile);
        auth.skin = Err(AuthError::Network("down".into()));

        let capes = account_capes(account.id, &accounts, &sessions, &auth)
            .await
            .unwrap();
        assert_eq!(capes.len(), 1);
        assert_eq!(capes[0].png, None);
    }

    #[tokio::test]
    async fn offline_profiles_have_no_capes() {
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![offline.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let auth = FakeMicrosoftAuth::approving();
        assert_eq!(
            account_capes(offline.id, &accounts, &sessions, &auth).await,
            Err(AccountCapesError::NotMicrosoft)
        );
        assert!(auth.calls().is_empty());
    }
}
