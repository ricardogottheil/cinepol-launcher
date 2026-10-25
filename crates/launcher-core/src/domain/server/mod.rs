//! The CINEPOL server: its configuration (address, supported versions, links)
//! and its live status.

mod ports;

pub use ports::{ServerConfigError, ServerConfigSource, ServerStatusError, ServerStatusProbe};

/// Where the launcher reads the CINEPOL server configuration.
pub const SERVER_CONFIG_URL: &str = "https://cinepol.policarpiers.com/server-config.json";

use std::cmp::Ordering;
use std::fmt;

use thiserror::Error;

use crate::domain::instance::GameVersion;
use crate::domain::server_list::ServerAddress;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServerConfigInvalid {
    #[error("{0:?} is not an https URL")]
    NotHttps(String),
    #[error("{0} is not a release version (expected numbers like 1.21.11 or 26.2)")]
    NotARelease(String),
    #[error("the recommended version {recommended} is outside {min} - {max}")]
    RecommendedOutOfRange {
        min: String,
        max: String,
        recommended: String,
    },
    #[error("the minimum version {min} is above the maximum {max}")]
    InvertedRange { min: String, max: String },
    #[error("unsupported config schema version {0}")]
    UnsupportedSchema(u32),
}

/// An absolute `https://` URL without whitespace. Links open in the system
/// browser, so nothing else is accepted from a remote config.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpsUrl(String);

impl HttpsUrl {
    pub fn parse(raw: &str) -> Result<Self, ServerConfigInvalid> {
        let rest = raw.strip_prefix("https://");
        let valid = rest.is_some_and(|rest| {
            !rest.is_empty()
                && raw.len() <= 2048
                && !raw.chars().any(|c| c.is_whitespace() || c.is_control())
                && !rest.starts_with('/')
        });
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ServerConfigInvalid::NotHttps(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// For compile-time constants that are known to be valid.
    pub(crate) fn from_trusted(url: &'static str) -> Self {
        Self(url.to_owned())
    }
}

impl fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Orders release ids numerically segment by segment: 1.21.11 < 26.1 < 26.2.
/// `None` when either id is not purely numeric (snapshots, pre-releases).
pub fn compare_releases(a: &GameVersion, b: &GameVersion) -> Option<Ordering> {
    let parse = |version: &GameVersion| -> Option<Vec<u32>> {
        version
            .as_str()
            .split('.')
            .map(|part| part.parse().ok())
            .collect()
    };
    let (a, b) = (parse(a)?, parse(b)?);
    let len = a.len().max(b.len());
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    Some(
        (0..len)
            .map(|i| at(&a, i).cmp(&at(&b, i)))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal),
    )
}

/// Which game versions can join, and which one the launcher installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedVersions {
    min: GameVersion,
    max: GameVersion,
    recommended: GameVersion,
}

impl SupportedVersions {
    pub fn new(
        min: GameVersion,
        max: GameVersion,
        recommended: GameVersion,
    ) -> Result<Self, ServerConfigInvalid> {
        let not_release = |v: &GameVersion| ServerConfigInvalid::NotARelease(v.as_str().to_owned());
        let min_max = compare_releases(&min, &max).ok_or_else(|| not_release(&min))?;
        if min_max == Ordering::Greater {
            return Err(ServerConfigInvalid::InvertedRange {
                min: min.as_str().into(),
                max: max.as_str().into(),
            });
        }
        let above_min =
            compare_releases(&recommended, &min).ok_or_else(|| not_release(&recommended))?;
        let below_max =
            compare_releases(&recommended, &max).ok_or_else(|| not_release(&recommended))?;
        if above_min == Ordering::Less || below_max == Ordering::Greater {
            return Err(ServerConfigInvalid::RecommendedOutOfRange {
                min: min.as_str().into(),
                max: max.as_str().into(),
                recommended: recommended.as_str().into(),
            });
        }
        Ok(Self {
            min,
            max,
            recommended,
        })
    }

    pub fn min(&self) -> &GameVersion {
        &self.min
    }

    pub fn max(&self) -> &GameVersion {
        &self.max
    }

    pub fn recommended(&self) -> &GameVersion {
        &self.recommended
    }

    /// Whether `version` (a release) can join. Snapshots never can.
    pub fn supports(&self, version: &GameVersion) -> bool {
        matches!(
            (
                compare_releases(version, &self.min),
                compare_releases(version, &self.max)
            ),
            (
                Some(Ordering::Greater | Ordering::Equal),
                Some(Ordering::Less | Ordering::Equal)
            )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityLinks {
    pub website: HttpsUrl,
    pub twitch: HttpsUrl,
    pub discord: HttpsUrl,
}

/// The CINEPOL server configuration, from the remote `server-config.json`
/// or the copy bundled with the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub name: String,
    pub address: ServerAddress,
    pub versions: SupportedVersions,
    pub links: CommunityLinks,
}

/// The only `server-config.json` schema this launcher reads.
pub const SERVER_CONFIG_SCHEMA: u32 = 1;

/// What the server answers to a status ping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    pub online_players: u32,
    pub max_players: u32,
    /// The server's version label, e.g. "Velocity-CTD 1.21.11-26.3".
    pub version_name: String,
    pub protocol: i32,
    /// The message of the day as plain text (formatting codes removed).
    pub motd: String,
    pub latency_ms: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(raw: &str) -> GameVersion {
        GameVersion::parse(raw).unwrap()
    }

    #[test]
    fn https_urls_only() {
        assert!(HttpsUrl::parse("https://twitch.tv/polispol1").is_ok());
        for raw in [
            "http://twitch.tv",
            "https://",
            "https:///x",
            "javascript:alert(1)",
            "https://a b",
            "file:///etc",
        ] {
            assert!(HttpsUrl::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn releases_compare_numerically() {
        assert_eq!(
            compare_releases(&version("1.21.11"), &version("26.2")),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_releases(&version("26.2"), &version("26.10")),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_releases(&version("1.21"), &version("1.21.0")),
            Some(Ordering::Equal)
        );
        assert_eq!(
            compare_releases(&version("26.3-snapshot-1"), &version("26.2")),
            None
        );
    }

    #[test]
    fn the_cinepol_range_accepts_its_versions_only() {
        let range =
            SupportedVersions::new(version("1.21.11"), version("26.2"), version("26.2")).unwrap();
        for ok in ["1.21.11", "26.1", "26.2"] {
            assert!(range.supports(&version(ok)), "{ok}");
        }
        for no in ["1.21.10", "26.3", "1.8.9", "26.3-snapshot-1"] {
            assert!(!range.supports(&version(no)), "{no}");
        }
    }

    #[test]
    fn rejects_inconsistent_ranges() {
        assert!(matches!(
            SupportedVersions::new(version("1.21.11"), version("26.2"), version("26.3")),
            Err(ServerConfigInvalid::RecommendedOutOfRange { .. })
        ));
        assert!(matches!(
            SupportedVersions::new(version("26.2"), version("1.21.11"), version("26.2")),
            Err(ServerConfigInvalid::InvertedRange { .. })
        ));
        assert!(matches!(
            SupportedVersions::new(version("1.21.11"), version("26.2"), version("26.2-pre1")),
            Err(ServerConfigInvalid::NotARelease(_))
        ));
    }
}
