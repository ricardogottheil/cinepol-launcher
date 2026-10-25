//! Reads the CINEPOL server config from the website, falling back to the copy
//! bundled with the launcher when it is unreachable or invalid.

use crate::domain::server::{HttpsUrl, SERVER_CONFIG_URL, ServerConfig, ServerConfigSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOrigin {
    Remote,
    /// The bundled copy, with why the remote one was not used.
    Bundled {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedServerConfig {
    pub config: ServerConfig,
    pub origin: ConfigOrigin,
}

/// Never fails: the launcher must always know how to reach the server.
/// Always the website's address: a URL an earlier version let players store
/// is ignored, so nobody stays on an old one.
pub async fn load_server_config<C: ServerConfigSource>(
    source: &C,
    bundled: &ServerConfig,
) -> LoadedServerConfig {
    match source
        .fetch(&HttpsUrl::from_trusted(SERVER_CONFIG_URL))
        .await
    {
        Ok(config) => LoadedServerConfig {
            config,
            origin: ConfigOrigin::Remote,
        },
        Err(error) => {
            tracing::info!(%error, "using the bundled server config");
            LoadedServerConfig {
                config: bundled.clone(),
                origin: ConfigOrigin::Bundled {
                    reason: error.to_string(),
                },
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use crate::domain::instance::GameVersion;
    use crate::domain::server::{
        CommunityLinks, CommunityServer, HttpsUrl, ServerConfig, SupportedVersions,
    };
    use crate::domain::server_list::ServerAddress;

    pub fn cinepol(recommended: &str) -> ServerConfig {
        let version = |raw: &str| GameVersion::parse(raw).unwrap();
        ServerConfig {
            name: "CINEPOL".into(),
            servers: vec![CommunityServer {
                name: "CINEPOL".into(),
                address: ServerAddress::parse("mc.policarpiers.com").unwrap(),
            }],
            versions: SupportedVersions::new(
                version("1.21.11"),
                version("26.3"),
                version(recommended),
            )
            .unwrap(),
            links: CommunityLinks {
                website: HttpsUrl::parse("https://cinepol.policarpiers.com/").unwrap(),
                twitch: HttpsUrl::parse("https://twitch.tv/polispol1").unwrap(),
                discord: HttpsUrl::parse("https://discord.gg/cinepol").unwrap(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::cinepol;
    use super::*;
    use crate::testing::FakeServerConfigSource;

    #[tokio::test]
    async fn prefers_the_remote_config() {
        let remote = cinepol("26.3");
        let source = FakeServerConfigSource::serving(SERVER_CONFIG_URL, Ok(remote.clone()));
        let loaded = load_server_config(&source, &cinepol("26.2")).await;
        assert_eq!(
            loaded,
            LoadedServerConfig {
                config: remote,
                origin: ConfigOrigin::Remote
            }
        );
    }

    #[tokio::test]
    async fn falls_back_to_the_bundled_config_when_offline_or_invalid() {
        let bundled = cinepol("26.2");
        let offline = load_server_config(&FakeServerConfigSource::default(), &bundled).await;
        assert_eq!(offline.config, bundled);
        assert!(matches!(offline.origin, ConfigOrigin::Bundled { .. }));

        let invalid = FakeServerConfigSource::serving(
            SERVER_CONFIG_URL,
            Err(crate::domain::server::ServerConfigError::Invalid(
                "bad".into(),
            )),
        );
        let loaded = load_server_config(&invalid, &bundled).await;
        assert_eq!(loaded.config, bundled);
    }

    #[test]
    fn reads_the_cinepol_website() {
        assert_eq!(
            SERVER_CONFIG_URL,
            "https://cinepol.policarpiers.com/server-config.json"
        );
    }
}
