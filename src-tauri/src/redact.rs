//! Log redaction: masks tokens, auth codes and API keys before a formatted
//! log line reaches its destination.
//!
//! [`RedactingMakeWriter`] wraps any `tracing_subscriber` writer. The fmt
//! layer formats each event into one buffer and writes it with a single
//! `write_all`, so every `write` call here sees a whole log line and a
//! secret cannot be split across two calls.

use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::LazyLock;

use regex_lite::Regex;
use tracing_subscriber::fmt::MakeWriter;

/// Replacement for every masked value.
const MASK: &str = "<redacted>";

/// Written instead of the line if the patterns ever fail to compile, so a
/// broken redactor fails closed rather than leaking.
const WITHHELD: &str = "[log line withheld: the redaction patterns are unavailable]\n";

/// Keys whose values are always secret. Matched case-insensitively.
const SECRET_KEYS: &str =
    "access_token|refresh_token|device_code|client_secret|rpsticket|identitytoken|token";

/// Optional ANSI color codes the fmt layer puts around field names.
const ANSI: &str = r"(?:\x1b\[[0-9;]*m)*";

/// Characters of an opaque token (base64, base64url, URL-encoded, and the
/// `!*$` Microsoft uses in refresh tokens and auth codes).
const TOKEN: &str = r"[A-Za-z0-9._~!*$+/=%-]";

/// What may precede a form key: start of line, a non-word character, or an
/// ANSI color code (which ends in the word character `m`).
const KEY_START: &str = r"(^|[^A-Za-z0-9_]|\x1b\[[0-9;]*m)";

static RULES: LazyLock<Option<Vec<(Regex, &'static str)>>> = LazyLock::new(|| {
    let patterns = [
        // JSON, also escaped inside a Debug string: "key": "value".
        (
            format!(r#"(?i)(\\?"(?:{SECRET_KEYS}|code)\\?"\s*:\s*\\?")[^"\\]+"#),
            "${1}<redacted>",
        ),
        // Form, query string or tracing field: key=value or key="value".
        (
            format!(r#"(?i){KEY_START}((?:{SECRET_KEYS}){ANSI}={ANSI}\\?"?)[^\s&"\\,;]+"#),
            "${1}${2}<redacted>",
        ),
        // `code` is a common word, so only long, token-shaped values count:
        // `code=Some(1)` (an exit code) stays readable.
        (
            format!(r#"(?i){KEY_START}(code{ANSI}={ANSI}\\?"?){TOKEN}{{8,}}"#),
            "${1}${2}<redacted>",
        ),
        (format!(r"(?i)(\bbearer\s+){TOKEN}+"), "${1}<redacted>"),
        (r#"(XBL3\.0 x=)[^\s"\\,]+"#.to_owned(), "${1}<redacted>"),
        // Bare Microsoft refresh tokens and auth codes.
        (
            format!(r"(^|[^A-Za-z0-9_])M\.(?:C|R3_){TOKEN}{{8,}}"),
            "${1}<redacted>",
        ),
        // JWT (three segments) and JWE (five segments).
        (
            r"eyJ[A-Za-z0-9_-]{4,}(?:\.[A-Za-z0-9_-]*){2,4}".to_owned(),
            MASK,
        ),
    ];
    patterns
        .into_iter()
        .map(|(pattern, replacement)| Regex::new(&pattern).ok().map(|re| (re, replacement)))
        .collect()
});

/// Masks token-like content in one formatted log line.
pub fn redact(line: &str) -> Cow<'_, str> {
    let Some(rules) = RULES.as_ref() else {
        return Cow::Borrowed(WITHHELD);
    };
    let mut text = Cow::Borrowed(line);
    for (rule, replacement) in rules {
        if let Cow::Owned(changed) = rule.replace_all(&text, *replacement) {
            text = Cow::Owned(changed);
        }
    }
    text
}

/// A [`MakeWriter`] whose writers pass every log line through [`redact`].
#[derive(Debug, Clone)]
pub struct RedactingMakeWriter<M> {
    inner: M,
}

impl<M> RedactingMakeWriter<M> {
    pub fn new(inner: M) -> Self {
        Self { inner }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer_for(meta),
        }
    }
}

/// Writer produced by [`RedactingMakeWriter`].
#[derive(Debug)]
pub struct RedactingWriter<W> {
    inner: W,
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.inner.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI0MjQyIiwieHVpZCI6IjI1MzUifQ.dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const MS_REFRESH: &str = "M.C507_BAY.0.U.-CgvIsj3!Fq8xY*kL2pZ$aB9cD7eF1gH";
    const MS_REFRESH_R3: &str = "M.R3_BAY.-CRaqeLb5!xw7Vp2*Kd0";
    const AUTH_CODE: &str = "M.C539_BL2.2.U.1a2b3c4d-5e6f-7a8b-9c0d-ef1234567890";

    fn assert_masked(line: &str, secret: &str) {
        let out = redact(line);
        assert!(!out.contains(secret), "secret leaked: {out}");
        assert!(out.contains("<redacted>"), "no mask in: {out}");
    }

    #[test]
    fn masks_jwt_shaped_strings() {
        assert_masked(
            &format!("minecraft login ok token {JWT} expires 86400"),
            JWT,
        );
        let jwe = "eyJlbmMiOiJBMTI4Q0JDK0hTMjU2In0.Zm9v.YmFy.YmF6.cXV4";
        assert_masked(&format!("xbl={jwe}"), "Zm9v.YmFy");
    }

    #[test]
    fn masks_values_in_json_bodies() {
        let samples = [
            (
                r#"{"access_token": "EwBYA+l3BAAUcDnR9grBJokeAHaUV8R3", "expires_in":3600}"#,
                "EwBYA+l3",
            ),
            (
                r#"{"refresh_token":"opaque-refresh-value-123"}"#,
                "opaque-refresh",
            ),
            (
                r#"{"device_code": "CAQABAAEAAAD--DLA3VO7QrddgJg7WevrA"}"#,
                "CAQABAAEAAAD",
            ),
            (r#"{"code":"short1"}"#, "short1"),
            (r#"{"client_secret": "s3cr3t-api-key"}"#, "s3cr3t"),
            (
                r#"{"Properties":{"RpsTicket":"d=EwAoA+pvBAAUKods63Ys1fGlwiccIFJ"}}"#,
                "EwAoA+pv",
            ),
            (
                r#"{"identityToken": "XBL3.0 x=1234567890;eyJhbGc.abc.def"}"#,
                "1234567890;",
            ),
        ];
        for (line, secret) in samples {
            assert_masked(line, secret);
        }
    }

    #[test]
    fn masks_json_escaped_inside_a_debug_string() {
        let line = r#"body="{\"access_token\":\"EwBYA+l3BAAUcDnR\",\"token_type\":\"Bearer\"}""#;
        assert_masked(line, "EwBYA+l3");
        assert!(redact(line).contains(r#"\"token_type\":\"Bearer\""#));
    }

    #[test]
    fn masks_values_in_form_bodies_and_query_strings() {
        let form = format!(
            "client_id=00000000402b5328&refresh_token={MS_REFRESH}&grant_type=refresh_token"
        );
        assert_masked(&form, MS_REFRESH);
        assert!(redact(&form).contains("client_id=00000000402b5328"));
        assert!(redact(&form).contains("grant_type=refresh_token"));

        let redirect =
            format!("redirect https://login.live.com/oauth20_desktop.srf?code={AUTH_CODE}&lc=1033");
        assert_masked(&redirect, AUTH_CODE);
        assert!(redact(&redirect).contains("&lc=1033"));

        assert_masked("access_token=EwBYA+l3BAAU expires_in=3600", "EwBYA+l3");
        assert_masked("device_code=CAQABAAEAAAD&client_id=abc", "CAQABAAEAAAD");
        assert_masked("client_secret=abc123def", "abc123def");
        assert_masked("RpsTicket=d=EwAoA+pvBAAU&x=1", "EwAoA+pv");
        assert_masked("identityToken=XBL3_token_value", "XBL3_token_value");
    }

    #[test]
    fn masks_structured_tracing_fields_with_ansi_colors() {
        let line = "\x1b[3maccess_token\x1b[0m\x1b[2m=\x1b[0m\"EwBYA+l3BAAU\" done";
        assert_masked(line, "EwBYA+l3");
    }

    #[test]
    fn masks_authorization_headers() {
        assert_masked(&format!("Authorization: Bearer {JWT}"), "dBjftJeZ4CVP");
        assert_masked(
            r#"headers {"authorization": "Bearer abc.def-ghi_jkl"}"#,
            "abc.def-ghi_jkl",
        );
        assert_masked(
            "Authorization: XBL3.0 x=2535405290118337;EwCAA+pvBAAUKods63Ys1",
            "EwCAA+pvBAAU",
        );
        assert_masked(
            "x=2535405290118337 XBL3.0 x=2535405290118337;tok",
            "2535405290118337;tok",
        );
    }

    #[test]
    fn masks_bare_microsoft_refresh_tokens() {
        assert_masked(&format!("cached {MS_REFRESH} for account"), MS_REFRESH);
        assert_masked(&format!("value={MS_REFRESH_R3}"), MS_REFRESH_R3);
    }

    #[test]
    fn leaves_ordinary_lines_unchanged() {
        let lines = [
            "installing Minecraft 1.21.11 with Fabric 0.17.3",
            "INFO cinepol_launcher: data dir /Users/me/Library/Application Support/CinepolLauncher",
            r"C:\Users\me\AppData\Roaming\CinepolLauncher\instances\survival",
            "account 3f2504e0-4f89-41d3-9a0c-0305e82c3301 is active",
            "GET https://piston-meta.mojang.com/mc/game/version_manifest_v2.json 200",
            "https://api.modrinth.com/v2/project/fabric-api/version?loaders=[\"fabric\"]",
            "the game exited code=Some(1) killed_by_launcher=false",
            "the game exited code=Some(-1073741819) killed_by_launcher=false",
            "error_code=429 retry in 2s",
            "user_code=ABCD-EFGH verification_uri=https://www.microsoft.com/link",
            r#"{"token_type":"Bearer","expires_in":86400,"code":0}"#,
            "loaded M.Craft modpack",
        ];
        for line in lines {
            assert_eq!(redact(line), line);
        }
    }

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("buffer lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_subscriber_writer_masks_events() {
        let buffer = Buffer::default();
        let sink = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(RedactingMakeWriter::new(move || sink.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(refresh_token = MS_REFRESH, "refreshed for version 1.21.11");
            tracing::info!("response {{\"access_token\":\"{JWT}\"}}");
        });
        let output =
            String::from_utf8(buffer.0.lock().expect("buffer lock").clone()).expect("utf-8 output");
        assert!(!output.contains(MS_REFRESH), "{output}");
        assert!(!output.contains("dBjftJeZ4CVP"), "{output}");
        assert!(output.contains("refreshed for version 1.21.11"), "{output}");
    }
}
