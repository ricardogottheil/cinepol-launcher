//! Rotating log files: each run starts a fresh file and keeps a few older ones.

use std::io;
use std::path::{Path, PathBuf};

/// `name.log` becomes `name.1.log`, `name.1.log` becomes `name.2.log`, and so
/// on; the oldest beyond `keep` is removed. A missing file is not an error.
pub fn rotate_log(path: &Path, keep: usize) -> io::Result<()> {
    if keep == 0 {
        return remove_if_present(path);
    }
    remove_if_present(&numbered(path, keep))?;
    for index in (1..keep).rev() {
        rename_if_present(&numbered(path, index), &numbered(path, index + 1))?;
    }
    rename_if_present(path, &numbered(path, 1))
}

/// Rotates `path`, then creates it empty (and its folder when missing).
pub fn create_rotated_log(path: &Path, keep: usize) -> io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rotate_log(path, keep)?;
    std::fs::File::create(path)
}

/// `launcher.log` with index 2 is `launcher.2.log`.
fn numbered(path: &Path, index: usize) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(extension) => format!("{stem}.{index}.{}", extension.to_string_lossy()),
        None => format!("{stem}.{index}"),
    };
    path.with_file_name(name)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn rename_if_present(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn keeps_the_newest_runs_and_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs").join("launcher.log");
        for run in ["one", "two", "three", "four"] {
            let mut file = create_rotated_log(&log, 2).unwrap();
            std::io::Write::write_all(&mut file, run.as_bytes()).unwrap();
        }
        assert_eq!(read(&log), "four");
        assert_eq!(
            read(&dir.path().join("logs").join("launcher.1.log")),
            "three"
        );
        assert_eq!(read(&dir.path().join("logs").join("launcher.2.log")), "two");
        assert!(!dir.path().join("logs").join("launcher.3.log").exists());
    }

    #[test]
    fn rotating_nothing_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        rotate_log(&dir.path().join("game-cinepol.log"), 1).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
