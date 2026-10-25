//! The CINEPOL `server-config.json`: parsing, validation and the HTTP source.

use serde::Deserialize;

use crate::domain::instance::GameVersion;
use crate::domain::ports::HttpClient;
use crate::domain::server::{
    CommunityLinks, CommunityServer, HttpsUrl, MAX_COMMUNITY_SERVERS, ServerConfig,
    ServerConfigError, ServerConfigInvalid, ServerConfigSource, SupportedVersions,
};
use crate::domain::server_list::ServerAddress;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    schema_version: u32,
    /// Schema 2: the community's name.
    #[serde(default)]
    name: Option<String>,
    /// Schema 1: the single server.
    #[serde(default)]
    server: Option<ServerSection>,
    /// Schema 2: every community server, possibly none.
    #[serde(default)]
    servers: Option<Vec<ServerSection>>,
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

/// A display name: 1-64 printable characters.
fn display_name(raw: &str, what: &str) -> Result<String, ServerConfigError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(invalid(format!(
            "the {what} must be 1-64 printable characters"
        )));
    }
    Ok(name.to_owned())
}

fn community_server(section: &ServerSection) -> Result<CommunityServer, ServerConfigError> {
    let raw_address = match section.port {
        Some(port) if port != 25565 => format!("{}:{port}", section.address),
        _ => section.address.clone(),
    };
    Ok(CommunityServer {
        name: display_name(&section.name, "server name")?,
        address: ServerAddress::parse(&raw_address).map_err(invalid)?,
    })
}

/// Parses and validates a `server-config.json` document (schema 1 or 2).
pub fn parse_server_config(bytes: &[u8]) -> Result<ServerConfig, ServerConfigError> {
    let file: ConfigFile = serde_json::from_slice(bytes).map_err(invalid)?;
    let (name, sections) = match file.schema_version {
        1 => {
            let server = file
                .server
                .ok_or_else(|| invalid("a schema 1 config needs its server"))?;
            (server.name.clone(), vec![server])
        }
        2 => (
            file.name
                .ok_or_else(|| invalid("a schema 2 config needs the community name"))?,
            file.servers
                .ok_or_else(|| invalid("a schema 2 config needs its servers, even if none"))?,
        ),
        other => return Err(invalid(ServerConfigInvalid::UnsupportedSchema(other))),
    };
    let name = display_name(&name, "community name")?;
    if sections.len() > MAX_COMMUNITY_SERVERS {
        return Err(invalid(format!(
            "a config lists at most {MAX_COMMUNITY_SERVERS} servers"
        )));
    }
    let mut servers: Vec<CommunityServer> = Vec::with_capacity(sections.len());
    for section in &sections {
        let server = community_server(section)?;
        if servers
            .iter()
            .any(|known| known.address.same_server(&server.address))
        {
            return Err(invalid(format!("{} is listed twice", server.address)));
        }
        servers.push(server);
    }
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
        name,
        servers,
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

    /// A schema 1 document, as the website served before schema 2.
    fn schema_one() -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "server": { "name": "CINEPOL", "address": "mc.policarpiers.com", "port": 25565 },
            "versions": { "min": "1.21.11", "max": "26.2", "recommended": "26.2" },
            "links": {
                "website": "https://cinepol.policarpiers.com/",
                "twitch": "https://twitch.tv/polispol1",
                "discord": "https://discord.gg/cinepol"
            }
        })
    }

    fn servers(config: &ServerConfig) -> Vec<(String, String)> {
        config
            .servers
            .iter()
            .map(|server| (server.name.clone(), server.address.to_string()))
            .collect()
    }

    #[test]
    fn parses_the_bundled_config() {
        let config = parse_server_config(BUNDLED).unwrap();
        assert_eq!(config.name, "CINEPOL");
        assert_eq!(
            servers(&config),
            [("CINEPOL".to_owned(), "mc.policarpiers.com".to_owned())]
        );
        assert_eq!(config.versions.min().as_str(), "1.21.11");
        assert_eq!(config.versions.max().as_str(), "26.2");
        assert_eq!(config.versions.recommended().as_str(), "26.2");
        assert_eq!(config.links.twitch.as_str(), "https://twitch.tv/polispol1");
        assert_eq!(config.links.discord.as_str(), "https://discord.gg/cinepol");
    }

    #[test]
    fn reads_a_schema_one_config_as_a_single_server() {
        let bytes = serde_json::to_vec(&schema_one()).unwrap();
        let config = parse_server_config(&bytes).unwrap();
        assert_eq!(config.name, "CINEPOL");
        assert_eq!(
            servers(&config),
            [("CINEPOL".to_owned(), "mc.policarpiers.com".to_owned())]
        );
    }

    #[test]
    fn lists_every_community_server_in_order_with_its_port() {
        let config = parse_server_config(&with(|v| {
            v["servers"] = serde_json::json!([
                { "name": "CINEPOL", "address": "mc.policarpiers.com" },
                { "name": "Creative", "address": "creative.policarpiers.com", "port": 25570 }
            ]);
        }))
        .unwrap();
        assert_eq!(
            servers(&config),
            [
                ("CINEPOL".to_owned(), "mc.policarpiers.com".to_owned()),
                (
                    "Creative".to_owned(),
                    "creative.policarpiers.com:25570".to_owned()
                ),
            ]
        );
        assert_eq!(config.main_server().unwrap().name, "CINEPOL");
    }

    #[test]
    fn accepts_a_config_without_servers() {
        let config = parse_server_config(&with(|v| v["servers"] = serde_json::json!([]))).unwrap();
        assert!(config.servers.is_empty());
        assert!(config.main_server().is_none());
        assert_eq!(config.name, "CINEPOL");
    }

    #[test]
    fn rejects_unsafe_or_inconsistent_configs() {
        type Edit = Box<dyn FnOnce(&mut serde_json::Value)>;
        let too_many: Vec<serde_json::Value> = (0..=MAX_COMMUNITY_SERVERS)
            .map(|index| serde_json::json!({ "name": format!("S{index}"), "address": format!("s{index}.example.org") }))
            .collect();
        let cases: Vec<Edit> = vec![
            Box::new(|v| v["schemaVersion"] = 3.into()),
            Box::new(|v| v["servers"][0]["address"] = "bad host".into()),
            Box::new(|v| v["servers"][0]["name"] = "".into()),
            Box::new(|v| v["name"] = "".into()),
            Box::new(|v| {
                v.as_object_mut().unwrap().remove("servers");
            }),
            Box::new(|v| {
                v["servers"] = serde_json::json!([
                    { "name": "A", "address": "mc.policarpiers.com" },
                    { "name": "B", "address": "MC.policarpiers.com:25565" }
                ]);
            }),
            Box::new(move |v| v["servers"] = serde_json::Value::Array(too_many)),
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
        let mut no_server = schema_one();
        no_server.as_object_mut().unwrap().remove("server");
        assert!(matches!(
            parse_server_config(&serde_json::to_vec(&no_server).unwrap()),
            Err(ServerConfigError::Invalid(_))
        ));
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
