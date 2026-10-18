//! Tells whether an offline profile name belongs to a premium account, so the
//! UI can warn that the server will ask for Microsoft sign-in.

use crate::domain::account::{PremiumNames, Username, UsernameError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PremiumNameCheck {
    Premium,
    Free,
    /// The lookup failed (offline, rate limited): no warning either way.
    Unknown,
}

pub async fn check_premium_name<P: PremiumNames>(
    raw_username: &str,
    names: &P,
) -> Result<PremiumNameCheck, UsernameError> {
    let name = Username::parse(raw_username.trim())?;
    Ok(match names.is_premium(&name).await {
        Ok(true) => PremiumNameCheck::Premium,
        Ok(false) => PremiumNameCheck::Free,
        Err(error) => {
            tracing::debug!(%error, "could not check whether the name is premium");
            PremiumNameCheck::Unknown
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakePremiumNames;

    #[tokio::test]
    async fn a_premium_name_is_reported_as_premium() {
        let names = FakePremiumNames::with(["polispol1"]);
        assert_eq!(
            check_premium_name(" polispol1 ", &names).await,
            Ok(PremiumNameCheck::Premium)
        );
        assert_eq!(names.asked(), ["polispol1"]);
    }

    #[tokio::test]
    async fn an_unknown_name_is_free() {
        let names = FakePremiumNames::with(["polispol1"]);
        assert_eq!(
            check_premium_name("CinepolTest7319", &names).await,
            Ok(PremiumNameCheck::Free)
        );
    }

    #[tokio::test]
    async fn a_failed_lookup_is_unknown() {
        let names = FakePremiumNames::unavailable();
        assert_eq!(
            check_premium_name("polispol1", &names).await,
            Ok(PremiumNameCheck::Unknown)
        );
    }

    #[tokio::test]
    async fn an_invalid_name_is_rejected_without_a_lookup() {
        let names = FakePremiumNames::with(["polispol1"]);
        assert!(check_premium_name("no spaces", &names).await.is_err());
        assert!(names.asked().is_empty());
    }
}
