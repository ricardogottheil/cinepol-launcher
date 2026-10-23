//! Wear one of the capes a Microsoft account owns, or none.

use super::account_capes::{AccountCapesError, microsoft_account};
use super::refresh_session::{PremiumSessions, SessionError};
use crate::domain::account::{AccountId, AccountRepository, CapeId, MicrosoftAuth};

/// Wears `cape`, or no cape for `None`. Sent once, like every profile change.
pub async fn set_cape<A, Q, M>(
    id: AccountId,
    cape: Option<CapeId>,
    accounts: &A,
    sessions: &Q,
    auth: &M,
) -> Result<(), AccountCapesError>
where
    A: AccountRepository,
    Q: PremiumSessions,
    M: MicrosoftAuth,
{
    let account = microsoft_account(id, accounts).await?;
    let token = sessions.session_token(&account).await?;
    auth.set_cape(&token, cape.as_ref())
        .await
        .map_err(SessionError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, AuthError, Username};
    use crate::testing::{
        FakeMicrosoftAuth, FakeSessions, InMemoryAccountRepository, sample_profile,
    };

    const MIGRATOR: &str = "1981aad3-73bd-4d8b-a8d5-4dd0f5bc8e1c";

    fn premium() -> Account {
        Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428")
    }

    #[tokio::test]
    async fn wears_a_cape_or_none_with_the_session_token() {
        let account = premium();
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let auth = FakeMicrosoftAuth::approving();

        let migrator = CapeId::parse(MIGRATOR).unwrap();
        set_cape(account.id, Some(migrator), &accounts, &sessions, &auth)
            .await
            .unwrap();
        set_cape(account.id, None, &accounts, &sessions, &auth)
            .await
            .unwrap();
        assert_eq!(
            auth.calls(),
            [
                format!("cape:mc-token:{MIGRATOR}"),
                "cape:mc-token:none".into()
            ]
        );
    }

    #[tokio::test]
    async fn offline_profiles_and_refused_capes_fail_clearly() {
        let account = premium();
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let accounts = InMemoryAccountRepository::with(vec![account.clone(), offline.clone()]);
        let sessions = FakeSessions::with_token("mc-token");
        let mut auth = FakeMicrosoftAuth::approving();
        auth.change = Err(AuthError::CapeNotOwned);

        assert_eq!(
            set_cape(offline.id, None, &accounts, &sessions, &auth).await,
            Err(AccountCapesError::NotMicrosoft)
        );
        assert_eq!(
            set_cape(
                account.id,
                Some(CapeId::parse(MIGRATOR).unwrap()),
                &accounts,
                &sessions,
                &auth
            )
            .await,
            Err(AccountCapesError::Session(SessionError::Auth(
                AuthError::CapeNotOwned
            )))
        );
    }
}
