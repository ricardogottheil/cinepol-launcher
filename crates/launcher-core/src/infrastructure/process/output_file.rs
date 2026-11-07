//! Saves the game's output to a file, so it outlives the launcher window.

use std::io::{LineWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::launch::log4j::LogEntry;
use crate::domain::launch::{LogSink, LogStream};
use crate::infrastructure::fs::create_rotated_log;

/// The previous launch is kept next to the new one.
const KEEP_PREVIOUS: usize = 1;

/// A mod that floods its output must not fill the disk: past this size the
/// file starts over, so it always holds the latest lines.
const MAX_LOG_BYTES: u64 = 32 * 1024 * 1024;

/// The first line of a file that started over.
const EARLIER_OUTPUT_DROPPED: &str =
    "(earlier output was dropped to keep this file small; the game's own logs/latest.log has it)\n";

/// `LogSink` that writes every line to the instance's game output log.
/// The file is started on the first line, so a launch that fails before the
/// game prints anything keeps the previous output.
pub struct GameOutputFile {
    path: PathBuf,
    max_bytes: u64,
    file: Mutex<Output>,
}

enum Output {
    NotStarted,
    Writing {
        file: LineWriter<std::fs::File>,
        written: u64,
    },
    /// It could not be created or written; the game runs on without it.
    Off,
}

impl GameOutputFile {
    pub fn create(path: &Path) -> Self {
        Self::with_limit(path, MAX_LOG_BYTES)
    }

    /// Like `create`, starting over once the file would pass `max_bytes`.
    pub fn with_limit(path: &Path, max_bytes: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            max_bytes,
            file: Mutex::new(Output::NotStarted),
        }
    }

    fn write(&self, text: &str) {
        let Ok(mut output) = self.file.lock() else {
            return;
        };
        if matches!(*output, Output::NotStarted) {
            *output = match create_rotated_log(&self.path, KEEP_PREVIOUS) {
                Ok(file) => Output::Writing {
                    file: LineWriter::new(file),
                    written: 0,
                },
                Err(error) => {
                    tracing::warn!(path = %self.path.display(), %error, "could not save the game output");
                    Output::Off
                }
            };
        }
        if let Output::Writing { file, written } = &mut *output
            && let Err(error) = append(file, written, self.max_bytes, text)
        {
            tracing::warn!(%error, "stopped saving the game output");
            *output = Output::Off;
        }
    }
}

/// Writes one line, emptying the file first when it would pass `max_bytes`.
fn append(
    file: &mut LineWriter<std::fs::File>,
    written: &mut u64,
    max_bytes: u64,
    text: &str,
) -> std::io::Result<()> {
    let line_len = text.len() as u64 + 1;
    if *written > 0 && *written + line_len > max_bytes {
        file.flush()?;
        let inner = file.get_mut();
        inner.set_len(0)?;
        inner.seek(SeekFrom::Start(0))?;
        file.write_all(EARLIER_OUTPUT_DROPPED.as_bytes())?;
        *written = EARLIER_OUTPUT_DROPPED.len() as u64;
    }
    writeln!(file, "{text}")?;
    *written += line_len;
    Ok(())
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
    fn a_log_past_its_size_limit_starts_over_and_keeps_the_latest_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game-cinepol.log");
        // Room for the note and three 13-byte lines after starting over.
        let limit = EARLIER_OUTPUT_DROPPED.len() as u64 + 40;
        let sink = GameOutputFile::with_limit(&path, limit);
        for index in 0..19 {
            sink.line(LogStream::Stdout, format!("spam line {index:02}"));
        }
        drop(sink);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            format!("{EARLIER_OUTPUT_DROPPED}spam line 16\nspam line 17\nspam line 18\n")
        );
        assert!(saved.len() as u64 <= limit);
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
