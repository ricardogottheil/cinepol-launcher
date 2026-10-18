//! The skin texture of an account, for the 3D preview.

use thiserror::Error;

use crate::domain::account::{
    AccountId, AccountKind, AccountRepository, AccountRepositoryError, AuthError, MicrosoftAuth,
    SkinVariant,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountSkinError {
    #[error("no account with that id")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] AccountRepositoryError),
    #[error(transparent)]
    Auth(#[from] AuthError),
}

/// A skin texture (PNG bytes) and the arm model it is drawn for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinTexture {
    pub png: Vec<u8>,
    pub variant: SkinVariant,
}

/// `None` for offline accounts and Microsoft accounts without a custom skin.
pub async fn account_skin<A: AccountRepository, M: MicrosoftAuth>(
    id: AccountId,
    accounts: &A,
    auth: &M,
) -> Result<Option<SkinTexture>, AccountSkinError> {
    let account = accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or(AccountSkinError::NotFound)?;
    let AccountKind::Microsoft(microsoft) = account.kind else {
        return Ok(None);
    };
    let Some(skin) = microsoft.skin else {
        return Ok(None);
    };
    let png = auth.skin_texture(&skin.url).await?;
    Ok(Some(SkinTexture {
        png,
        variant: skin.variant,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::testing::{FakeMicrosoftAuth, InMemoryAccountRepository, sample_profile};

    #[tokio::test]
    async fn fetches_the_skin_of_a_microsoft_account() {
        let premium = Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428");
        let accounts = InMemoryAccountRepository::with(vec![premium.clone()]);
        let auth = FakeMicrosoftAuth::approving();
        let skin = account_skin(premium.id, &accounts, &auth)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(skin.variant, SkinVariant::Classic);
        assert_eq!(skin.png, b"\x89PNG");
        assert_eq!(
            auth.calls(),
            ["skin:https://textures.minecraft.net/texture/abc123"]
        );
    }

    #[tokio::test]
    async fn offline_and_skinless_accounts_have_none_and_unknown_ids_fail() {
        let offline = Account::offline(AccountId::new_random(), Username::parse("Steve").unwrap());
        let mut profile = sample_profile();
        profile.skin = None;
        let bare = Account::microsoft(AccountId::new_random(), &profile, "1");
        let accounts = InMemoryAccountRepository::with(vec![offline.clone(), bare.clone()]);
        let auth = FakeMicrosoftAuth::approving();
        assert_eq!(account_skin(offline.id, &accounts, &auth).await, Ok(None));
        assert_eq!(account_skin(bare.id, &accounts, &auth).await, Ok(None));
        assert!(auth.calls().is_empty());
        assert_eq!(
            account_skin(AccountId::new_random(), &accounts, &auth).await,
            Err(AccountSkinError::NotFound)
        );
    }
}
