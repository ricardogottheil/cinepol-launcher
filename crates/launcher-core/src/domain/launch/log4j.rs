//! Decoding the game's log4j XML output into structured log entries.
//!
//! Versions that ship a logging configuration print every event as XML:
//!
//! ```text
//! <log4j:Event logger="net.minecraft.client.Minecraft" timestamp="1700000000000" level="INFO" thread="Render thread">
//!   <log4j:Message><![CDATA[Setting user: polispol1]]></log4j:Message>
//! </log4j:Event>
//! ```
//!
//! A message or throwable may span several lines. Lines outside an event
//! (older versions, the JVM, crash dumps) pass through as plain entries.

use std::sync::{Mutex, PoisonError};

use super::ports::{LogSink, LogStream};

/// A log4j level; anything else is kept as `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_uppercase().as_str() {
            "TRACE" => Some(Self::Trace),
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            "FATAL" => Some(Self::Fatal),
            _ => None,
        }
    }
}

/// One line of output, decoded when it came from a log4j event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub stream: LogStream,
    pub level: Option<LogLevel>,
    pub thread: Option<String>,
    pub logger: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub timestamp_ms: Option<u64>,
    /// The message, followed by the throwable (stack trace) when there is one.
    pub message: String,
}

impl LogEntry {
    pub fn plain(stream: LogStream, line: String) -> Self {
        Self {
            stream,
            level: None,
            thread: None,
            logger: None,
            timestamp_ms: None,
            message: line,
        }
    }
}

/// Turns raw lines into entries; keeps the partial event between lines.
#[derive(Debug, Default)]
pub struct Log4jDecoder {
    event: Option<Partial>,
}

#[derive(Debug, Default)]
struct Partial {
    level: Option<LogLevel>,
    thread: Option<String>,
    logger: Option<String>,
    timestamp_ms: Option<u64>,
    text: String,
    /// The original lines, shown as plain text if the event never closes.
    raw: Vec<String>,
    /// Whether the text so far ends inside an unclosed CDATA section.
    in_cdata: bool,
}

/// Whether `line` ends inside a CDATA section, given whether it started in one.
fn ends_in_cdata(mut in_cdata: bool, mut line: &str) -> bool {
    loop {
        let (marker, next) = if in_cdata {
            ("]]>", false)
        } else {
            ("<![CDATA[", true)
        };
        let Some(at) = line.find(marker) else {
            return in_cdata;
        };
        line = &line[at + marker.len()..];
        in_cdata = next;
    }
}

/// log4j writes only its own tags (and indentation) outside CDATA; any other
/// line there was printed by someone else while the event was being written.
fn is_markup(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with("<log4j:") || line.starts_with("</log4j:")
}

/// An event longer than this is not log4j output after all (or never closes):
/// its lines are let through as plain text instead of being held back.
pub const MAX_EVENT_LINES: usize = 1000;

impl Log4jDecoder {
    /// Feeds one line; returns the entries it completes (zero or more).
    pub fn feed(&mut self, stream: LogStream, line: &str) -> Vec<LogEntry> {
        let Some(event) = self.event.as_mut() else {
            let Some(start) = line.find(EVENT_START) else {
                return vec![LogEntry::plain(stream, line.to_owned())];
            };
            let rest = &line[start..];
            let header_end = rest.find('>').unwrap_or(rest.len());
            let header = &rest[..header_end];
            let mut partial = Partial {
                level: attribute(header, "level").and_then(|level| LogLevel::parse(&level)),
                thread: attribute(header, "thread"),
                logger: attribute(header, "logger"),
                timestamp_ms: attribute(header, "timestamp").and_then(|ts| ts.parse().ok()),
                text: String::new(),
                raw: vec![line.to_owned()],
                in_cdata: false,
            };
            partial
                .text
                .push_str(rest.get(header_end + 1..).unwrap_or_default());
            partial.in_cdata = ends_in_cdata(false, &partial.text);
            if partial.text.contains(EVENT_END) {
                return vec![partial.finish(stream)];
            }
            self.event = Some(partial);
            return Vec::new();
        };
        if !event.in_cdata && !is_markup(line) {
            return vec![LogEntry::plain(stream, line.to_owned())];
        }
        event.in_cdata = ends_in_cdata(event.in_cdata, line);
        event.text.push('\n');
        event.text.push_str(line);
        event.raw.push(line.to_owned());
        if !line.contains(EVENT_END) {
            if event.raw.len() <= MAX_EVENT_LINES {
                return Vec::new();
            }
            let raw = std::mem::take(&mut event.raw);
            self.event = None;
            return raw
                .into_iter()
                .map(|line| LogEntry::plain(stream, line))
                .collect();
        }
        self.event
            .take()
            .map(|event| vec![event.finish(stream)])
            .unwrap_or_default()
    }
}

impl Log4jDecoder {
    /// The lines of an event that never closed (the game died while writing
    /// it), as plain entries.
    pub fn finish(&mut self, stream: LogStream) -> Vec<LogEntry> {
        self.event
            .take()
            .map(|event| {
                event
                    .raw
                    .into_iter()
                    .map(|line| LogEntry::plain(stream, line))
                    .collect()
            })
            .unwrap_or_default()
    }
}

const EVENT_START: &str = "<log4j:Event";
const EVENT_END: &str = "</log4j:Event>";

impl Partial {
    fn finish(self, stream: LogStream) -> LogEntry {
        let message = element(&self.text, "log4j:Message").unwrap_or_default();
        let message = match element(&self.text, "log4j:Throwable") {
            Some(throwable) if !throwable.is_empty() => format!("{message}\n{throwable}"),
            _ => message,
        };
        LogEntry {
            stream,
            level: self.level,
            thread: self.thread,
            logger: self.logger,
            timestamp_ms: self.timestamp_ms,
            message,
        }
    }
}

/// The text inside `<name>…</name>`: its CDATA sections joined (log4j splits
/// one around every `]]>`), or the unescaped text when there is no CDATA.
fn element(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let end = start + text[start..].find(&close)?;
    let inner = &text[start..end];
    if !inner.contains("<![CDATA[") {
        return Some(unescape(inner.trim()));
    }
    let mut out = String::new();
    let mut rest = inner;
    while let Some(open_at) = rest.find("<![CDATA[") {
        let body = &rest[open_at + "<![CDATA[".len()..];
        let close_at = body.find("]]>").unwrap_or(body.len());
        out.push_str(&body[..close_at]);
        rest = body.get(close_at + "]]>".len()..).unwrap_or_default();
    }
    Some(out)
}

/// An attribute's unescaped value from the event's opening tag.
fn attribute(header: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let start = header.find(&key)? + key.len();
    let end = start + header[start..].find('"')?;
    Some(unescape(&header[start..end]))
}

fn unescape(raw: &str) -> String {
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A [`LogSink`] that decodes log4j events before forwarding them as entries.
pub struct Log4jSink<S: LogSink> {
    inner: S,
    stdout: Mutex<Log4jDecoder>,
    stderr: Mutex<Log4jDecoder>,
}

impl<S: LogSink> Log4jSink<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            stdout: Mutex::default(),
            stderr: Mutex::default(),
        }
    }
}

/// The game is gone once the sink is dropped: show whatever was held back.
impl<S: LogSink> Drop for Log4jSink<S> {
    fn drop(&mut self) {
        for (stream, decoder) in [
            (LogStream::Stdout, &self.stdout),
            (LogStream::Stderr, &self.stderr),
        ] {
            let entries = decoder
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .finish(stream);
            for entry in entries {
                self.inner.entry(entry);
            }
        }
    }
}

impl<S: LogSink> LogSink for Log4jSink<S> {
    fn line(&self, stream: LogStream, line: String) {
        let decoder = match stream {
            LogStream::Stdout => &self.stdout,
            LogStream::Stderr => &self.stderr,
        };
        let entries = decoder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .feed(stream, &line);
        for entry in entries {
            self.inner.entry(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(lines: &[&str]) -> Vec<LogEntry> {
        let mut decoder = Log4jDecoder::default();
        lines
            .iter()
            .flat_map(|line| decoder.feed(LogStream::Stdout, line))
            .collect()
    }

    #[test]
    fn decodes_an_event_across_lines() {
        let entries = decode(&[
            r#"<log4j:Event logger="net.minecraft.client.Minecraft" timestamp="1700000000123" level="INFO" thread="Render thread">"#,
            "  <log4j:Message><![CDATA[Setting user: polispol1]]></log4j:Message>",
            "</log4j:Event>",
        ]);
        assert_eq!(
            entries,
            [LogEntry {
                stream: LogStream::Stdout,
                level: Some(LogLevel::Info),
                thread: Some("Render thread".into()),
                logger: Some("net.minecraft.client.Minecraft".into()),
                timestamp_ms: Some(1_700_000_000_123),
                message: "Setting user: polispol1".into(),
            }]
        );
    }

    #[test]
    fn keeps_multi_line_messages_and_throwables() {
        let entries = decode(&[
            r#"<log4j:Event logger="FabricLoader" timestamp="1" level="ERROR" thread="main">"#,
            "<log4j:Message><![CDATA[Loading 149 mods:",
            "\t- fabric-api 0.130.0]]></log4j:Message>",
            "<log4j:Throwable><![CDATA[java.lang.IllegalStateException: boom",
            "\tat Foo.bar(Foo.java:1)]]></log4j:Throwable>",
            "</log4j:Event>",
        ]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, Some(LogLevel::Error));
        assert_eq!(
            entries[0].message,
            "Loading 149 mods:\n\t- fabric-api 0.130.0\njava.lang.IllegalStateException: boom\n\tat Foo.bar(Foo.java:1)"
        );
    }

    #[test]
    fn unescapes_attributes_and_split_cdata() {
        let entries = decode(&[
            r#"<log4j:Event logger="a&amp;b" timestamp="2" level="WARN" thread="&quot;Worker&quot; &lt;1&gt;">"#,
            "<log4j:Message><![CDATA[contains ]]]]><![CDATA[> inside]]></log4j:Message>",
            "</log4j:Event>",
        ]);
        assert_eq!(entries[0].logger.as_deref(), Some("a&b"));
        assert_eq!(entries[0].thread.as_deref(), Some("\"Worker\" <1>"));
        assert_eq!(entries[0].message, "contains ]]> inside");
    }

    #[test]
    fn passes_other_lines_through_as_plain_entries() {
        let entries = decode(&[
            "[12:00:00] [main/INFO]: plain line",
            r#"<log4j:Event logger="x" timestamp="3" level="TRACE" thread="t">"#,
            "<log4j:Message><![CDATA[one]]></log4j:Message>",
            "</log4j:Event>",
            "Exception in thread \"main\"",
        ]);
        let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "[12:00:00] [main/INFO]: plain line",
                "one",
                "Exception in thread \"main\""
            ]
        );
        assert_eq!(entries[0].level, None);
        assert_eq!(entries[1].level, Some(LogLevel::Trace));
    }

    #[test]
    fn a_plain_line_printed_inside_an_open_event_is_not_lost() {
        let entries = decode(&[
            r#"<log4j:Event logger="x" timestamp="4" level="INFO" thread="t">"#,
            "<log4j:Message><![CDATA[first",
            "still the message]]></log4j:Message>",
            "System.out from a mod",
            "",
            "</log4j:Event>",
        ]);
        let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            ["System.out from a mod", "first\nstill the message"]
        );
        assert_eq!(entries[0].level, None);
        assert_eq!(entries[1].level, Some(LogLevel::Info));
    }

    #[test]
    fn an_unknown_level_is_kept_as_none() {
        let entries = decode(&[
            r#"<log4j:Event logger="x" timestamp="nope" level="NOTICE" thread="t">"#,
            "<log4j:Message><![CDATA[hi]]></log4j:Message>",
            "</log4j:Event>",
        ]);
        assert_eq!(entries[0].level, None);
        assert_eq!(entries[0].timestamp_ms, None);
        assert_eq!(entries[0].message, "hi");
    }

    #[test]
    fn the_sink_forwards_decoded_entries() {
        #[derive(Default, Clone)]
        struct Collect(std::sync::Arc<Mutex<Vec<LogEntry>>>);
        impl LogSink for Collect {
            fn line(&self, stream: LogStream, line: String) {
                self.entry(LogEntry::plain(stream, line));
            }
            fn entry(&self, entry: LogEntry) {
                self.0.lock().unwrap().push(entry);
            }
        }
        let collected = Collect::default();
        let sink = Log4jSink::new(collected.clone());
        sink.line(
            LogStream::Stdout,
            r#"<log4j:Event logger="x" timestamp="4" level="INFO" thread="t">"#.into(),
        );
        sink.line(LogStream::Stderr, "stderr line".into());
        sink.line(
            LogStream::Stdout,
            "<log4j:Message><![CDATA[ok]]></log4j:Message>".into(),
        );
        sink.line(LogStream::Stdout, "</log4j:Event>".into());
        let entries = collected.0.lock().unwrap().clone();
        let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["stderr line", "ok"]);
        assert_eq!(entries[0].stream, LogStream::Stderr);
    }

    #[test]
    fn an_event_that_never_closes_falls_back_to_plain_lines() {
        let mut decoder = Log4jDecoder::default();
        let mut entries = decoder.feed(
            LogStream::Stdout,
            r#"<log4j:Event logger="x" timestamp="5" level="INFO" thread="t">"#,
        );
        // A message whose CDATA never closes holds every following line.
        entries.extend(decoder.feed(LogStream::Stdout, "<log4j:Message><![CDATA[start"));
        for n in 0..MAX_EVENT_LINES - 1 {
            entries.extend(decoder.feed(LogStream::Stdout, &format!("line {n}")));
        }
        // Nothing is held back forever: the raw lines come out as plain text.
        assert_eq!(entries.len(), MAX_EVENT_LINES + 1);
        assert!(entries.iter().all(|entry| entry.level.is_none()));
        assert_eq!(
            entries.last().unwrap().message,
            format!("line {}", MAX_EVENT_LINES - 2)
        );
        let next = decoder.feed(LogStream::Stdout, "after");
        assert_eq!(next[0].message, "after");
    }

    #[test]
    fn a_game_that_dies_mid_event_still_shows_its_lines() {
        #[derive(Default, Clone)]
        struct Collect(std::sync::Arc<Mutex<Vec<LogEntry>>>);
        impl LogSink for Collect {
            fn line(&self, stream: LogStream, line: String) {
                self.entry(LogEntry::plain(stream, line));
            }
            fn entry(&self, entry: LogEntry) {
                self.0.lock().unwrap().push(entry);
            }
        }
        let collected = Collect::default();
        {
            let sink = Log4jSink::new(collected.clone());
            sink.line(
                LogStream::Stdout,
                r#"<log4j:Event logger="x" timestamp="6" level="FATAL" thread="t">"#.into(),
            );
            sink.line(
                LogStream::Stdout,
                "<log4j:Message><![CDATA[Out of memory".into(),
            );
            // The process ends here and the sink is dropped.
        }
        let messages: Vec<String> = collected
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.message.clone())
            .collect();
        assert_eq!(messages.len(), 2);
        assert!(messages[1].contains("Out of memory"));
    }
}
