//! The CINEPOL `server-config.json`: parsing, validation and the HTTP source.

use serde::Deserialize;

use crate::domain::instance::GameVersion;
use crate::domain::ports::HttpClient;
use crate::domain::server::{
    CommunityLinks, HttpsUrl, SERVER_CONFIG_SCHEMA, ServerConfig, ServerConfigError,
    ServerConfigInvalid, ServerConfigSource, SupportedVersions,
};
use crate::domain::server_list::ServerAddress;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    schema_version: u32,
    server: ServerSection,
    versions: VersionsSection,
    links: LinksSection,
}

#[derive(Debug, Deserialize)]
struct ServerSection {
    name: String,
    address: String,
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct VersionsSection {
    min: String,
    max: String,
    recommended: String,
}

#[derive(Debug, Deserialize)]
struct LinksSection {
    website: String,
    twitch: String,
    discord: String,
}

fn invalid(error: impl std::fmt::Display) -> ServerConfigError {
    ServerConfigError::Invalid(error.to_string())
}

/// Parses and validates a `server-config.json` document.
pub fn parse_server_config(bytes: &[u8]) -> Result<ServerConfig, ServerConfigError> {
    let file: ConfigFile = serde_json::from_slice(bytes).map_err(invalid)?;
    if file.schema_version != SERVER_CONFIG_SCHEMA {
        return Err(invalid(ServerConfigInvalid::UnsupportedSchema(
            file.schema_version,
        )));
    }
    let name = file.server.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(invalid("the server name must be 1-64 printable characters"));
    }
    let raw_address = match file.server.port {
        Some(port) if port != 25565 => format!("{}:{port}", file.server.address),
        _ => file.server.address.clone(),
    };
    let address = ServerAddress::parse(&raw_address).map_err(invalid)?;
    let version = |raw: &str| GameVersion::parse(raw).map_err(invalid);
    let versions = SupportedVersions::new(
        version(&file.versions.min)?,
        version(&file.versions.max)?,
        version(&file.versions.recommended)?,
    )
    .map_err(invalid)?;
    let links = CommunityLinks {
        website: HttpsUrl::parse(&file.links.website).map_err(invalid)?,
        twitch: HttpsUrl::parse(&file.links.twitch).map_err(invalid)?,
        discord: HttpsUrl::parse(&file.links.discord).map_err(invalid)?,
    };
    Ok(ServerConfig {
        name: name.to_owned(),
        address,
        versions,
        links,
    })
}

/// Reads the config over HTTP with the launcher's client.
#[derive(Debug, Clone)]
pub struct HttpServerConfigSource<H> {
    http: H,
}

impl<H> HttpServerConfigSource<H> {
    pub fn new(http: H) -> Self {
        Self { http }
    }
}

impl<H: HttpClient> ServerConfigSource for HttpServerConfigSource<H> {
    async fn fetch(&self, url: &HttpsUrl) -> Result<ServerConfig, ServerConfigError> {
        let bytes = self
            .http
            .get_bytes(url.as_str())
            .await
            .map_err(|error| ServerConfigError::Unavailable(error.to_string()))?;
        parse_server_config(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttpClient;

    /// The config bundled with the launcher at the repository root.
    const BUNDLED: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/server-config.json"
    ));

    fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(BUNDLED).unwrap();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn parses_the_bundled_config() {
        let config = parse_server_config(BUNDLED).unwrap();
        assert_eq!(config.name, "CINEPOL");
        assert_eq!(config.address.to_string(), "mc.policarpiers.com");
        assert_eq!(config.versions.min().as_str(), "1.21.11");
        assert_eq!(config.versions.max().as_str(), "26.2");
        assert_eq!(config.versions.recommended().as_str(), "26.2");
        assert_eq!(config.links.twitch.as_str(), "https://twitch.tv/polispol1");
        assert_eq!(config.links.discord.as_str(), "https://discord.gg/cinepol");
    }

    #[test]
    fn keeps_a_non_default_port() {
        let config = parse_server_config(&with(|v| v["server"]["port"] = 25570.into())).unwrap();
        assert_eq!(config.address.to_string(), "mc.policarpiers.com:25570");
    }

    #[test]
    fn rejects_unsafe_or_inconsistent_configs() {
        type Edit = Box<dyn FnOnce(&mut serde_json::Value)>;
        let cases: Vec<Edit> = vec![
            Box::new(|v| v["schemaVersion"] = 2.into()),
            Box::new(|v| v["server"]["address"] = "bad host".into()),
            Box::new(|v| v["server"]["name"] = "".into()),
            Box::new(|v| v["versions"]["recommended"] = "26.3".into()),
            Box::new(|v| v["versions"]["min"] = "../1.21".into()),
            Box::new(|v| v["links"]["twitch"] = "http://twitch.tv/polispol1".into()),
            Box::new(|v| v["links"]["discord"] = "javascript:alert(1)".into()),
        ];
        for edit in cases {
            assert!(matches!(
                parse_server_config(&with(edit)),
                Err(ServerConfigError::Invalid(_))
            ));
        }
        assert!(matches!(
            parse_server_config(b"{"),
            Err(ServerConfigError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn fetches_over_http_and_reports_offline_as_unavailable() {
        let url = HttpsUrl::parse("https://example.invalid/server-config.json").unwrap();
        let online = HttpServerConfigSource::new(FakeHttpClient::with([(
            url.to_string(),
            BUNDLED.to_vec(),
        )]));
        assert_eq!(online.fetch(&url).await.unwrap().name, "CINEPOL");
        let offline = HttpServerConfigSource::new(FakeHttpClient::default());
        assert!(matches!(
            offline.fetch(&url).await,
            Err(ServerConfigError::Unavailable(_))
        ));
    }
}
