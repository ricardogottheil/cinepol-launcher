//! Keeps the session token out of the game output the launcher shows and saves.

use super::log4j::LogEntry;
use super::{LogSink, LogStream};

/// Shorter secrets (the offline placeholder `0`) would mask ordinary text.
const MIN_SECRET_LEN: usize = 8;
const MASK: &str = "<redacted>";

/// Replaces `secret` in every line and entry before passing it on.
pub struct MaskingLogSink<S> {
    inner: S,
    secret: Option<String>,
}

impl<S: LogSink> MaskingLogSink<S> {
    pub fn new(inner: S, secret: &str) -> Self {
        Self {
            inner,
            secret: (secret.len() >= MIN_SECRET_LEN).then(|| secret.to_owned()),
        }
    }

    fn mask(&self, text: String) -> String {
        match &self.secret {
            Some(secret) if text.contains(secret.as_str()) => text.replace(secret.as_str(), MASK),
            _ => text,
        }
    }
}

impl<S: LogSink> LogSink for MaskingLogSink<S> {
    fn line(&self, stream: LogStream, line: String) {
        self.inner.line(stream, self.mask(line));
    }

    fn entry(&self, mut entry: LogEntry) {
        entry.message = self.mask(entry.message);
        self.inner.entry(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingLogSink;

    #[test]
    fn masks_the_secret_in_lines_and_entries() {
        let logs = RecordingLogSink::default();
        let sink = MaskingLogSink::new(logs.clone(), "eyJsecret-token");
        sink.line(LogStream::Stderr, "token=eyJsecret-token end".into());
        sink.entry(LogEntry::plain(LogStream::Stdout, "eyJsecret-token".into()));
        assert_eq!(
            logs.lines(),
            [
                (LogStream::Stderr, "token=<redacted> end".to_owned()),
                (LogStream::Stdout, "<redacted>".to_owned()),
            ]
        );
    }

    #[test]
    fn short_secrets_leave_the_text_alone() {
        let logs = RecordingLogSink::default();
        MaskingLogSink::new(logs.clone(), "0").line(LogStream::Stdout, "Loaded 10 mods".into());
        assert_eq!(
            logs.lines(),
            [(LogStream::Stdout, "Loaded 10 mods".to_owned())]
        );
    }
}
