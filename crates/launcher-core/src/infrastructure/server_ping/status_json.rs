//! The status response JSON and its chat-component MOTD, as plain data.

use serde::Deserialize;
use serde_json::Value;

use crate::domain::server::ServerStatus;

#[derive(Debug, Deserialize)]
struct RawStatus {
    #[serde(default)]
    version: Option<RawVersion>,
    #[serde(default)]
    players: Option<RawPlayers>,
    #[serde(default)]
    description: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    protocol: i32,
}

#[derive(Debug, Deserialize)]
struct RawPlayers {
    #[serde(default)]
    online: i64,
    #[serde(default)]
    max: i64,
}

/// Parses the status JSON. `latency_ms` is left empty for the caller.
pub(crate) fn parse_status(json: &str) -> Result<ServerStatus, serde_json::Error> {
    let raw: RawStatus = serde_json::from_str(json)?;
    let count = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
    let (version_name, protocol) = raw
        .version
        .map(|version| (version.name, version.protocol))
        .unwrap_or_default();
    let (online, max) = raw
        .players
        .map(|players| (players.online, players.max))
        .unwrap_or_default();
    let mut text = String::new();
    if let Some(description) = &raw.description {
        flatten_component(description, &mut text);
    }
    Ok(ServerStatus {
        online_players: count(online),
        max_players: count(max),
        version_name,
        protocol,
        motd: plain_text(&text),
        latency_ms: None,
    })
}

/// Appends the text of a chat component: a string, an array of components, or
/// an object with `text` (or the `translate` key) followed by its `extra` children.
fn flatten_component(component: &Value, out: &mut String) {
    match component {
        Value::String(text) => out.push_str(text),
        Value::Array(parts) => parts.iter().for_each(|part| flatten_component(part, out)),
        Value::Object(fields) => {
            match (fields.get("text"), fields.get("translate")) {
                (Some(Value::String(text)), _) => out.push_str(text),
                (_, Some(Value::String(key))) => out.push_str(key),
                _ => {}
            }
            if let Some(extra) = fields.get("extra") {
                flatten_component(extra, out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Removes legacy `§x` formatting codes and trailing whitespace on each line.
fn plain_text(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            stripped.push(c);
        }
    }
    let lines: Vec<&str> = stripped.lines().map(str::trim_end).collect();
    lines.join("\n").trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/server_ping/cinepol-status.json");

    #[test]
    fn parses_the_real_cinepol_status() {
        let status = parse_status(FIXTURE).unwrap();
        assert_eq!(status.online_players, 11);
        assert_eq!(status.max_players, 100);
        assert_eq!(status.version_name, "Velocity-CTD 1.21.11-26.3");
        assert_eq!(status.protocol, 777);
        assert!(status.motd.contains("ᴄɪɴᴇᴘᴏʟ"), "{}", status.motd);
        assert!(status.motd.contains("ᴇɴ ʟɪɴᴇᴀ"), "{}", status.motd);
        assert!(!status.motd.contains('§'));
        assert_eq!(status.motd.lines().count(), 2, "{}", status.motd);
        assert_eq!(status.latency_ms, None);
    }

    #[test]
    fn a_plain_string_description_is_the_motd() {
        let json = r#"{"version":{"name":"1.21.11","protocol":774},
            "players":{"online":1,"max":20},"description":"§aHello   \n§lworld"}"#;
        assert_eq!(parse_status(json).unwrap().motd, "Hello\nworld");
    }

    #[test]
    fn missing_players_count_as_zero() {
        let status = parse_status(r#"{"version":{"name":"x","protocol":1}}"#).unwrap();
        assert_eq!((status.online_players, status.max_players), (0, 0));
        assert_eq!(status.motd, "");
    }

    #[test]
    fn negative_counts_are_zero() {
        let json = r#"{"version":{"name":"x","protocol":1},"players":{"online":-5,"max":-1}}"#;
        let status = parse_status(json).unwrap();
        assert_eq!((status.online_players, status.max_players), (0, 0));
    }

    #[test]
    fn translate_keys_and_arrays_are_flattened() {
        let json = r#"{"description":[{"translate":"multiplayer.status.ping"},
            " ", {"text":"a","extra":["b",{"text":"c","extra":[{"text":"d"}]}]}]}"#;
        assert_eq!(
            parse_status(json).unwrap().motd,
            "multiplayer.status.ping abcd"
        );
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_status("not json").is_err());
        assert!(parse_status(r#"{"version":{"protocol":"seven"}}"#).is_err());
    }
}
