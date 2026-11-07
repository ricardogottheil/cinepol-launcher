//! Saves the game's output to a file, so it outlives the launcher window.

use std::io::{LineWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::launch::log4j::LogEntry;
use crate::domain::launch::{LogSink, LogStream};
use crate::infrastructure::fs::create_rotated_log;

/// The previous launch is kept next to the new one.
const KEEP_PREVIOUS: usize = 1;

/// `LogSink` that writes every line to the instance's game output log.
/// The file is started on the first line, so a launch that fails before the
/// game prints anything keeps the previous output.
pub struct GameOutputFile {
    path: PathBuf,
    file: Mutex<Output>,
}

enum Output {
    NotStarted,
    Writing(LineWriter<std::fs::File>),
    /// It could not be created or written; the game runs on without it.
    Off,
}

impl GameOutputFile {
    pub fn create(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            file: Mutex::new(Output::NotStarted),
        }
    }

    fn write(&self, text: &str) {
        let Ok(mut output) = self.file.lock() else {
            return;
        };
        if matches!(*output, Output::NotStarted) {
            *output = match create_rotated_log(&self.path, KEEP_PREVIOUS) {
                Ok(file) => Output::Writing(LineWriter::new(file)),
                Err(error) => {
                    tracing::warn!(path = %self.path.display(), %error, "could not save the game output");
                    Output::Off
                }
            };
        }
        if let Output::Writing(file) = &mut *output
            && let Err(error) = writeln!(file, "{text}")
        {
            tracing::warn!(%error, "stopped saving the game output");
            *output = Output::Off;
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
    fn a_launch_that_prints_nothing_keeps_the_previous_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game-cinepol.log");
        GameOutputFile::create(&path).line(LogStream::Stdout, "the crash".into());
        // A launch that fails before the game starts (sign-in, Java) prints nothing.
        drop(GameOutputFile::create(&path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the crash\n");
        assert!(!dir.path().join("game-cinepol.1.log").exists());
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
