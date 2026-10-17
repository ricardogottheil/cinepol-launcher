use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::domain::java::{JavaProbe, JavaProbeError, JavaVersion, parse_java_version_output};

/// Runs `<java> -version` as a child process and parses its output.
#[derive(Debug, Clone, Copy)]
pub struct ProcessJavaProbe {
    timeout: Duration,
}

impl ProcessJavaProbe {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

    pub fn new() -> Self {
        Self::with_timeout(Self::DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for ProcessJavaProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaProbe for ProcessJavaProbe {
    async fn probe(&self, executable: &Path) -> Result<JavaVersion, JavaProbeError> {
        let child = tokio::process::Command::new(executable)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                JavaProbeError::NotExecutable(format!("{} ({error})", executable.display()))
            })?;
        // Dropping the future on timeout kills the child (`kill_on_drop`).
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                JavaProbeError::Unreadable(format!(
                    "{} -version timed out after {} s",
                    executable.display(),
                    self.timeout.as_secs_f32()
                ))
            })?
            .map_err(|error| JavaProbeError::Unreadable(error.to_string()))?;

        // `java -version` prints to stderr; some wrappers print to stdout.
        let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        parse_java_version_output(&text)
            .map_err(|error| JavaProbeError::Unreadable(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn parses_the_version_printed_on_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let java = script(
            dir.path(),
            "java",
            "echo 'openjdk version \"21.0.8\" 2025-07-15 LTS' >&2\necho 'OpenJDK 64-Bit Server VM Temurin-21.0.8+9' >&2",
        );

        let version = ProcessJavaProbe::new().probe(&java).await.unwrap();

        assert_eq!(version.major, 21);
        assert_eq!(version.full, "21.0.8");
        assert!(version.is_64_bit);
    }

    #[tokio::test]
    async fn a_missing_executable_is_not_executable() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-java-here");

        assert!(matches!(
            ProcessJavaProbe::new().probe(&missing).await,
            Err(JavaProbeError::NotExecutable(_))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_without_a_version_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let java = script(dir.path(), "java", "echo 'command not found' >&2");

        assert!(matches!(
            ProcessJavaProbe::new().probe(&java).await,
            Err(JavaProbeError::Unreadable(_))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_hanging_executable_times_out_as_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let java = script(dir.path(), "java", "sleep 5");

        let result = ProcessJavaProbe::with_timeout(Duration::from_millis(200))
            .probe(&java)
            .await;

        assert!(
            matches!(&result, Err(JavaProbeError::Unreadable(reason)) if reason.contains("timed out")),
            "{result:?}"
        );
    }
}
