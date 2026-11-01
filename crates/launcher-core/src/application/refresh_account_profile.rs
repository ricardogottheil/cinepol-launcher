//! Reads a Microsoft account's profile again from Mojang and stores its
//! current name and skin, so changes made outside the launcher show up.

use thiserror::Error;

use super::refresh_session::{PremiumSessions, SessionError};
use crate::domain::account::{
    Account, AccountId, AccountKind, AccountRepository, AccountRepositoryError, MicrosoftAuth,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RefreshAccountProfileError {
    #[error("no account with that id")]
    NotFound,
    #[error("offline profiles have nothing to refresh")]
    NotMicrosoft,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
}

/// Answers the account as stored after the refresh.
pub async fn refresh_account_profile<A, Q, M>(
    id: AccountId,
    accounts: &A,
    sessions: &Q,
    auth: &M,
) -> Result<Account, RefreshAccountProfileError>
where
    A: AccountRepository,
    Q: PremiumSessions,
    M: MicrosoftAuth,
{
    let account = find(id, accounts).await?;
    if !account.is_premium() {
        return Err(RefreshAccountProfileError::NotMicrosoft);
    }
    let token = sessions.session_token(&account).await?;
    let profile = auth.profile(&token).await.map_err(SessionError::from)?;
    // The account as it is now: a sign-in may have saved it while Mojang
    // answered. A removed account stays removed.
    let mut latest = find(id, accounts).await?;
    let AccountKind::Microsoft(microsoft) = &mut latest.kind else {
        return Err(RefreshAccountProfileError::NotMicrosoft);
    };
    microsoft.skin = profile.skin;
    latest.username = profile.name;
    accounts.save(&latest).await?;
    Ok(latest)
}

async fn find<A: AccountRepository>(
    id: AccountId,
    accounts: &A,
) -> Result<Account, RefreshAccountProfileError> {
    accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or(RefreshAccountProfileError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{AuthError, Skin, SkinUrl, SkinVariant, Username};
    use crate::testing::{
        FakeMicrosoftAuth, FakeSessions, InMemoryAccountRepository, sample_profile,
    };

    fn premium() -> Account {
        Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428")
    }

    #[tokio::test]
    async fn stores_the_name_and_skin_mojang_reports_now() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        let mut profile = sample_profile();
        profile.name = Username::parse("NewName").unwrap();
        let new_skin = Skin {
            url: SkinUrl::parse("https://textures.minecraft.net/texture/f00d").unwrap(),
            variant: SkinVariant::Slim,
        };
        profile.skin = Some(new_skin.clone());
        auth.profile = Ok(profile);

        let refreshed = refresh_account_profile(account.id, &accounts, &sessions, &auth)
            .await
            .unwrap();

        assert_eq!(refreshed.username.as_str(), "NewName");
        assert_eq!(refreshed.profile_uuid, account.profile_uuid);
        let AccountKind::Microsoft(microsoft) = &refreshed.kind else {
            panic!("still a Microsoft account");
        };
        assert_eq!(microsoft.skin, Some(new_skin));
        assert_eq!(microsoft.xuid, "2535428");
        assert_eq!(accounts.list().await.unwrap(), [refreshed]);
        assert_eq!(auth.calls(), ["profile:mc-token"]);
    }

    #[tokio::test]
    async fn a_profile_without_a_skin_clears_the_stored_one() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        let mut profile = sample_profile();
        profile.skin = None;
        auth.profile = Ok(profile);

        let refreshed = refresh_account_profile(account.id, &accounts, &sessions, &auth)
            .await
            .unwrap();
        let AccountKind::Microsoft(microsoft) = refreshed.kind else {
            panic!("still a Microsoft account");
        };
        assert_eq!(microsoft.skin, None);
    }

    #[tokio::test]
    async fn offline_and_unknown_accounts_are_refused_without_asking_mojang() {
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![offline.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let auth = FakeMicrosoftAuth::approving();
        assert_eq!(
            refresh_account_profile(offline.id, &accounts, &sessions, &auth).await,
            Err(RefreshAccountProfileError::NotMicrosoft)
        );
        assert_eq!(
            refresh_account_profile(AccountId::new_random(), &accounts, &sessions, &auth).await,
            Err(RefreshAccountProfileError::NotFound)
        );
        assert!(auth.calls().is_empty());
        assert_eq!(accounts.list().await.unwrap(), [offline]);
    }

    #[tokio::test]
    async fn a_failed_profile_read_keeps_the_stored_account() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        auth.profile = Err(AuthError::Network("down".into()));

        assert_eq!(
            refresh_account_profile(account.id, &accounts, &sessions, &auth).await,
            Err(RefreshAccountProfileError::Session(SessionError::from(
                AuthError::Network("down".into())
            )))
        );
        assert_eq!(accounts.list().await.unwrap(), [account]);
    }
}
