//! Premium-name lookups against Mojang's public profile API.

use serde::Deserialize;

use crate::domain::account::{PremiumNameLookupError, PremiumNames, Username};
use crate::domain::ports::{HttpClient, HttpError};

/// `GET` this plus a name answers the profile, or 404 when no account has it.
pub const PROFILES_URL: &str = "https://api.mojang.com/users/profiles/minecraft/";

#[derive(Debug, Deserialize)]
struct Profile {
    id: String,
}

/// Asks Mojang whether a premium account owns a name (case-insensitive).
#[derive(Debug, Clone)]
pub struct MojangPremiumNames<H> {
    http: H,
}

impl<H> MojangPremiumNames<H> {
    pub fn new(http: H) -> Self {
        Self { http }
    }
}

impl<H: HttpClient> PremiumNames for MojangPremiumNames<H> {
    async fn is_premium(&self, name: &Username) -> Result<bool, PremiumNameLookupError> {
        // `Username` only holds [A-Za-z0-9_], so it is safe in a URL path.
        let url = format!("{PROFILES_URL}{}", name.as_str());
        match self.http.get_bytes(&url).await {
            Ok(body) if body.iter().all(u8::is_ascii_whitespace) => Ok(false),
            Ok(body) => serde_json::from_slice::<Profile>(&body)
                .map(|profile| !profile.id.is_empty())
                .map_err(|error| PremiumNameLookupError::Unavailable(error.to_string())),
            Err(HttpError::NotFound(_)) => Ok(false),
            Err(error) => Err(PremiumNameLookupError::Unavailable(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttpClient;

    const PROFILE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/profile-polispol1.json"
    ));

    fn name(raw: &str) -> Username {
        Username::parse(raw).unwrap()
    }

    fn url(raw: &str) -> String {
        format!("{PROFILES_URL}{raw}")
    }

    /// Fails every request the way an offline machine does.
    struct Offline;

    impl HttpClient for Offline {
        async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            Err(HttpError::Network {
                url: url.to_owned(),
                reason: "offline".into(),
            })
        }
    }

    #[tokio::test]
    async fn a_name_with_a_profile_is_premium() {
        let names =
            MojangPremiumNames::new(FakeHttpClient::with([(url("polispol1"), PROFILE.to_vec())]));
        assert_eq!(names.is_premium(&name("polispol1")).await, Ok(true));
        assert_eq!(names.http.requests(), [url("polispol1")]);
    }

    #[tokio::test]
    async fn a_404_or_an_empty_answer_means_no_premium_account() {
        let names = MojangPremiumNames::new(FakeHttpClient::default());
        assert_eq!(names.is_premium(&name("CinepolTest7319")).await, Ok(false));
        let empty = MojangPremiumNames::new(FakeHttpClient::with([(url("Nobody"), vec![])]));
        assert_eq!(empty.is_premium(&name("Nobody")).await, Ok(false));
    }

    #[tokio::test]
    async fn network_failures_and_odd_answers_are_unavailable() {
        let offline = MojangPremiumNames::new(Offline);
        assert!(offline.is_premium(&name("polispol1")).await.is_err());
        let odd = MojangPremiumNames::new(FakeHttpClient::with([(
            url("polispol1"),
            b"<html>".to_vec(),
        )]));
        assert!(odd.is_premium(&name("polispol1")).await.is_err());
    }
}
