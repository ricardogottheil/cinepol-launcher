//! Saves the game's output to a file, so it outlives the launcher window.

use std::io::{LineWriter, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::domain::launch::log4j::LogEntry;
use crate::domain::launch::{LogSink, LogStream};
use crate::infrastructure::fs::create_rotated_log;

/// The previous launch is kept next to the new one.
const KEEP_PREVIOUS: usize = 1;

/// `LogSink` that writes every line to the instance's game output log.
pub struct GameOutputFile {
    file: Mutex<Option<LineWriter<std::fs::File>>>,
}

impl GameOutputFile {
    /// Starts a new log at `path`. When the file cannot be created the game
    /// still runs; its output just is not saved.
    pub fn create(path: &Path) -> Self {
        let file = match create_rotated_log(path, KEEP_PREVIOUS) {
            Ok(file) => Some(LineWriter::new(file)),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not save the game output");
                None
            }
        };
        Self {
            file: Mutex::new(file),
        }
    }

    fn write(&self, text: &str) {
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        if let Some(file) = guard.as_mut()
            && let Err(error) = writeln!(file, "{text}")
        {
            tracing::warn!(%error, "stopped saving the game output");
            *guard = None;
        }
    }
}

impl LogSink for GameOutputFile {
    fn line(&self, stream: LogStream, line: String) {
        match stream {
            LogStream::Stdout => self.write(&line),
            LogStream::Stderr => self.write(&format!("[stderr] {line}")),
        }
    }

    fn entry(&self, entry: LogEntry) {
        self.write(&format_entry(&entry));
    }
}

/// `[thread/LEVEL] [logger]: message`, close to the game's own `latest.log`.
fn format_entry(entry: &LogEntry) -> String {
    let thread = entry.thread.as_deref().unwrap_or("main");
    let level = entry.level.map_or_else(
        || "INFO".to_owned(),
        |level| format!("{level:?}").to_uppercase(),
    );
    match &entry.logger {
        Some(logger) => format!("[{thread}/{level}] [{logger}]: {}", entry.message),
        None => format!("[{thread}/{level}]: {}", entry.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::launch::log4j::LogLevel;

    #[test]
    fn saves_lines_and_entries_and_keeps_the_previous_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("game-cinepol.log");
        GameOutputFile::create(&path).line(LogStream::Stdout, "first launch".into());

        let sink = GameOutputFile::create(&path);
        sink.line(LogStream::Stderr, "Exception in thread main".into());
        sink.entry(LogEntry {
            stream: LogStream::Stdout,
            level: Some(LogLevel::Error),
            thread: Some("Render thread".into()),
            logger: Some("net.minecraft.client.Minecraft".into()),
            timestamp_ms: Some(1),
            message: "Unreported exception thrown!".into(),
        });
        drop(sink);

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[stderr] Exception in thread main\n\
             [Render thread/ERROR] [net.minecraft.client.Minecraft]: Unreported exception thrown!\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("logs").join("game-cinepol.1.log")).unwrap(),
            "first launch\n"
        );
    }

    #[test]
    fn a_log_that_cannot_be_created_does_not_stop_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("logs");
        std::fs::write(&blocker, b"a file where the folder should be").unwrap();
        let sink = GameOutputFile::create(&blocker.join("game-cinepol.log"));
        sink.line(LogStream::Stdout, "still running".into());
    }
}
