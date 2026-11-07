//! The launcher's own log file, `<data>/logs/launcher.log`, so what happened
//! before a crash can still be read after the launcher is closed.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use launcher_core::infrastructure::fs::create_rotated_log;
use tracing_subscriber::fmt::MakeWriter;

/// Older runs kept next to the current one.
const KEEP_PREVIOUS_RUNS: usize = 3;

/// A `MakeWriter` for the log file; writes nothing when it could not be created.
pub struct LauncherLogFile(Option<File>);

impl LauncherLogFile {
    pub fn create(path: &Path) -> Self {
        match create_rotated_log(path, KEEP_PREVIOUS_RUNS) {
            Ok(file) => Self(Some(file)),
            Err(error) => {
                // Logging is not set up yet: stderr is the only place left.
                eprintln!("could not create the log file {}: {error}", path.display());
                Self(None)
            }
        }
    }
}

pub struct LauncherLogWriter<'a>(Option<&'a File>);

impl Write for LauncherLogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0.as_mut() {
            Some(file) => file.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.0.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

impl<'a> MakeWriter<'a> for LauncherLogFile {
    type Writer = LauncherLogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LauncherLogWriter(self.0.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_to_the_file_and_rotates_older_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("launcher.log");
        LauncherLogFile::create(&path)
            .make_writer()
            .write_all(b"first run\n")
            .unwrap();
        LauncherLogFile::create(&path)
            .make_writer()
            .write_all(b"second run\n")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second run\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("logs").join("launcher.1.log")).unwrap(),
            "first run\n"
        );
    }

    #[test]
    fn a_missing_file_swallows_the_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("logs"), b"not a folder").unwrap();
        let log = LauncherLogFile::create(&dir.path().join("logs").join("launcher.log"));
        log.make_writer().write_all(b"lost").unwrap();
    }
}
