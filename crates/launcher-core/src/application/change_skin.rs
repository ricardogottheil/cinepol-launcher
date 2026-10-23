//! Upload a new skin for a Microsoft account.

use thiserror::Error;

use super::refresh_session::{PremiumSessions, SessionError};
use crate::domain::account::{
    Account, AccountId, AccountKind, AccountRepository, AccountRepositoryError, MicrosoftAuth,
    SkinPng, SkinPngError, SkinVariant,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChangeSkinError {
    #[error("no account with that id")]
    NotFound,
    #[error("offline profiles cannot change their skin")]
    NotMicrosoft,
    #[error(transparent)]
    InvalidSkin(#[from] SkinPngError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
}

/// Checks the image, uploads it once (Mojang may suspend accounts that keep
/// hitting its rate limit, so nothing here retries) and stores the skin
/// Mojang now reports, so the preview shows it without signing in again.
/// Once Mojang applied the upload this succeeds, even when storing fails.
pub async fn change_skin<A, Q, M>(
    id: AccountId,
    png: Vec<u8>,
    variant: SkinVariant,
    accounts: &A,
    sessions: &Q,
    auth: &M,
) -> Result<(), ChangeSkinError>
where
    A: AccountRepository,
    Q: PremiumSessions,
    M: MicrosoftAuth,
{
    let account = find(id, accounts).await?;
    if !matches!(account.kind, AccountKind::Microsoft(_)) {
        return Err(ChangeSkinError::NotMicrosoft);
    }
    let skin = SkinPng::parse(png)?;
    let token = sessions.session_token(&account).await?;
    let reported = auth
        .upload_skin(&token, &skin, variant)
        .await
        .map_err(SessionError::from)?;
    // The upload is applied from here on, so nothing below reports a failure:
    // the player would upload again against Mojang's rate limit.
    let Some(reported) = reported else {
        return Ok(());
    };
    // The account as it is now: the upload took a while, and a sign-in may
    // have saved it meanwhile. A removed account is left removed.
    match find(id, accounts).await {
        Ok(mut latest) => {
            if let AccountKind::Microsoft(microsoft) = &mut latest.kind {
                microsoft.skin = Some(reported);
                if let Err(error) = accounts.save(&latest).await {
                    tracing::warn!(%error, "the new skin is applied but could not be stored");
                }
            }
        }
        Err(error) => tracing::warn!(%error, "the account changed while its skin was uploaded"),
    }
    Ok(())
}

async fn find<A: AccountRepository>(
    id: AccountId,
    accounts: &A,
) -> Result<Account, ChangeSkinError> {
    accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or(ChangeSkinError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{AuthError, Skin, SkinUrl, Username};
    use crate::testing::{
        FakeMicrosoftAuth, FakeSessions, InMemoryAccountRepository, sample_profile,
    };

    fn png() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&64u32.to_be_bytes());
        bytes.extend_from_slice(&64u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn premium() -> Account {
        Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428")
    }

    fn slim_profile() -> crate::domain::account::MinecraftProfile {
        let mut profile = sample_profile();
        profile.skin = Some(Skin {
            url: SkinUrl::parse("http://textures.minecraft.net/texture/f00d").unwrap(),
            variant: SkinVariant::Slim,
        });
        profile
    }

    #[tokio::test]
    async fn uploads_the_skin_and_stores_the_one_mojang_reports() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        auth.uploaded = Ok(slim_profile().skin);

        change_skin(
            account.id,
            png(),
            SkinVariant::Slim,
            &accounts,
            &sessions,
            &auth,
        )
        .await
        .unwrap();

        assert_eq!(
            auth.calls(),
            [format!("upload:mc-token:Slim:{}", png().len())]
        );
        let stored = accounts.list().await.unwrap();
        let AccountKind::Microsoft(microsoft) = &stored[0].kind else {
            panic!("still a Microsoft account");
        };
        let skin = microsoft.skin.as_ref().unwrap();
        assert_eq!(skin.variant, SkinVariant::Slim);
        assert_eq!(
            skin.url.as_str(),
            "https://textures.minecraft.net/texture/f00d"
        );
    }

    #[tokio::test]
    async fn refuses_bad_images_and_offline_profiles_without_asking_mojang() {
        let account = premium();
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![account.clone(), offline.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let auth = FakeMicrosoftAuth::approving();

        let gif = b"GIF89a".to_vec();
        assert_eq!(
            change_skin(
                account.id,
                gif,
                SkinVariant::Classic,
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(ChangeSkinError::InvalidSkin(SkinPngError::NotPng))
        );
        assert_eq!(
            change_skin(
                offline.id,
                png(),
                SkinVariant::Classic,
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(ChangeSkinError::NotMicrosoft)
        );
        assert_eq!(
            change_skin(
                AccountId::new_random(),
                png(),
                SkinVariant::Classic,
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(ChangeSkinError::NotFound)
        );
        assert!(auth.calls().is_empty());
        assert!(sessions.asked().is_empty());
    }

    #[tokio::test]
    async fn a_refused_upload_keeps_the_stored_skin() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        auth.uploaded = Err(AuthError::SkinRejected);

        assert_eq!(
            change_skin(
                account.id,
                png(),
                SkinVariant::Classic,
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(ChangeSkinError::Session(SessionError::Auth(
                AuthError::SkinRejected
            )))
        );
        assert_eq!(accounts.list().await.unwrap(), [account]);
    }

    #[tokio::test]
    async fn an_upload_that_reports_no_skin_keeps_the_stored_one() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        auth.uploaded = Ok(None);

        change_skin(
            account.id,
            png(),
            SkinVariant::Slim,
            &accounts,
            &sessions,
            &auth,
        )
        .await
        .unwrap();
        assert_eq!(accounts.list().await.unwrap(), [account]);
    }

    #[tokio::test]
    async fn an_expired_session_asks_to_sign_in_again() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::default();
        let auth = FakeMicrosoftAuth::approving();
        assert_eq!(
            change_skin(
                account.id,
                png(),
                SkinVariant::Classic,
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(ChangeSkinError::Session(SessionError::Expired))
        );
        assert!(auth.calls().is_empty());
    }
}
