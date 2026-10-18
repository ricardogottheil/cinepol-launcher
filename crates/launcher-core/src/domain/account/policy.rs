use thiserror::Error;

use super::Account;

/// Build-time rule for offline profiles (`requirePremiumForOffline`).
/// CINEPOL accepts non-premium players, so the default is to allow them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OfflinePolicy {
    pub require_premium_for_offline: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OfflinePolicyError {
    #[error("an offline profile requires a Microsoft account that owns Minecraft")]
    PremiumRequired,
}

impl OfflinePolicy {
    /// Whether a new offline profile may be created next to `existing` accounts.
    pub fn check_can_create_offline(&self, existing: &[Account]) -> Result<(), OfflinePolicyError> {
        if self.require_premium_for_offline && !existing.iter().any(Account::is_premium) {
            return Err(OfflinePolicyError::PremiumRequired);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::domain::account::{AccountId, AccountKind, Username};

    fn offline(name: &str) -> Account {
        Account::offline(AccountId::new_random(), Username::parse(name).unwrap())
    }

    fn premium(name: &str) -> Account {
        Account {
            id: AccountId::new_random(),
            kind: AccountKind::Microsoft(crate::domain::account::MicrosoftAccount {
                xuid: "2535".into(),
                skin: None,
            }),
            username: Username::parse(name).unwrap(),
            profile_uuid: Uuid::new_v4(),
        }
    }

    #[test]
    fn allows_offline_profiles_by_default() {
        assert_eq!(
            OfflinePolicy::default().check_can_create_offline(&[]),
            Ok(())
        );
    }

    #[test]
    fn requires_a_premium_account_when_the_flag_is_on() {
        let policy = OfflinePolicy {
            require_premium_for_offline: true,
        };
        assert_eq!(
            policy.check_can_create_offline(&[]),
            Err(OfflinePolicyError::PremiumRequired)
        );
        assert_eq!(
            policy.check_can_create_offline(&[offline("Steve")]),
            Err(OfflinePolicyError::PremiumRequired)
        );
        assert_eq!(policy.check_can_create_offline(&[premium("Alex")]), Ok(()));
    }
}
