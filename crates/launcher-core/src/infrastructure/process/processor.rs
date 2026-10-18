//! Installer processor runner over `tokio::process`.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::domain::loader::{ProcessorCommand, ProcessorRunError, ProcessorRunner};

/// Lines of output kept for the error of a failed processor.
const TAIL_LINES: usize = 40;
/// How long to wait for the output readers after the tool ends.
const READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Hides the console window of `java.exe` on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Runs `java -cp <classpath> <main> <args>` in the working directory, keeps
/// the last lines of its output for the error, and kills it on cancel.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioProcessorRunner;

type Tail = Arc<Mutex<VecDeque<String>>>;

impl ProcessorRunner for TokioProcessorRunner {
    async fn run(
        &self,
        command: &ProcessorCommand,
        cancel: &CancellationToken,
    ) -> Result<(), ProcessorRunError> {
        // `;` on Windows, `:` elsewhere; fails if a path holds the separator.
        let classpath = std::env::join_paths(&command.classpath)
            .map_err(|error| ProcessorRunError::Spawn(format!("invalid classpath: {error}")))?;
        let mut process = Command::new(&command.java);
        process
            .arg("-cp")
            .arg(classpath)
            .arg(&command.main_class)
            .args(&command.args)
            .current_dir(&command.working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        process.creation_flags(CREATE_NO_WINDOW);

        // Arguments are paths and flags; only the tool is logged.
        tracing::debug!(main_class = %command.main_class, "starting a processor");
        let mut child = process.spawn().map_err(|error| {
            ProcessorRunError::Spawn(format!("{}: {error}", command.java.display()))
        })?;

        let tail: Tail = Arc::default();
        let readers: Vec<JoinHandle<()>> = [
            child
                .stdout
                .take()
                .map(|out| tokio::spawn(keep_tail(out, Arc::clone(&tail)))),
            child
                .stderr
                .take()
                .map(|err| tokio::spawn(keep_tail(err, Arc::clone(&tail)))),
        ]
        .into_iter()
        .flatten()
        .collect();

        let status = tokio::select! {
            status = child.wait() => status,
            () = cancel.cancelled() => {
                if let Err(error) = child.start_kill() {
                    tracing::warn!(%error, "could not kill a processor");
                }
                let _ = child.wait().await;
                drain(readers).await;
                return Err(ProcessorRunError::Cancelled);
            }
        };
        drain(readers).await;

        let status = status.map_err(|error| ProcessorRunError::Spawn(error.to_string()))?;
        if status.success() {
            return Ok(());
        }
        let output = tail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        Err(ProcessorRunError::Failed {
            code: status.code(),
            output,
        })
    }
}

/// Keeps the last [`TAIL_LINES`] lines (lossy UTF-8) of both streams.
async fn keep_tail<R: AsyncRead + Unpin>(reader: R, tail: Tail) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buffer).trim_end().to_owned();
                let mut lines = tail.lock().unwrap_or_else(PoisonError::into_inner);
                if lines.len() == TAIL_LINES {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
            Err(error) => {
                tracing::debug!(%error, "stopped reading a processor's output");
                break;
            }
        }
    }
}

/// Joins the readers, aborting any still running after the drain timeout.
async fn drain(readers: Vec<JoinHandle<()>>) {
    for mut reader in readers {
        if tokio::time::timeout(READER_DRAIN_TIMEOUT, &mut reader)
            .await
            .is_err()
        {
            reader.abort();
            let _ = reader.await;
        }
    }
}

// Plain `cfg(test)` so clippy's `allow-unwrap-in-tests` recognizes the module.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;

    /// A stand-in for `java`: a shell script that receives the same arguments.
    fn fake_java(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("java");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn command(java: PathBuf, working_dir: &Path) -> ProcessorCommand {
        ProcessorCommand {
            java,
            classpath: vec![PathBuf::from("/libs/a.jar"), PathBuf::from("/libs/b.jar")],
            main_class: "net.example.Main".into(),
            args: vec!["--input".into(), "with space".into()],
            working_dir: working_dir.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn runs_java_with_the_classpath_main_class_and_args_in_the_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let java = fake_java(
            dir.path(),
            r#"for arg in "$@"; do echo "$arg"; done > args.txt"#,
        );
        TokioProcessorRunner
            .run(&command(java, dir.path()), &CancellationToken::new())
            .await
            .unwrap();
        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "-cp",
                "/libs/a.jar:/libs/b.jar",
                "net.example.Main",
                "--input",
                "with space"
            ]
        );
    }

    #[tokio::test]
    async fn a_failing_tool_reports_its_exit_code_and_the_tail_of_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let java = fake_java(
            dir.path(),
            "i=1; while [ $i -le 60 ]; do echo \"line $i\"; i=$((i+1)); done; echo boom >&2; exit 3",
        );
        let result = TokioProcessorRunner
            .run(&command(java, dir.path()), &CancellationToken::new())
            .await;
        let Err(ProcessorRunError::Failed { code, output }) = result else {
            panic!("{result:?}");
        };
        assert_eq!(code, Some(3));
        assert!(output.contains("line 60"), "{output}");
        assert!(output.contains("boom"), "{output}");
        assert!(
            !output.contains("line 1\n"),
            "only the tail is kept: {output}"
        );
        assert!(output.lines().count() <= 40, "{output}");
    }

    #[tokio::test]
    async fn cancelling_kills_the_tool() {
        let dir = tempfile::tempdir().unwrap();
        let java = fake_java(dir.path(), "sleep 30");
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let result = TokioProcessorRunner
            .run(&command(java, dir.path()), &cancel)
            .await;
        assert_eq!(result, Err(ProcessorRunError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_missing_java_is_a_spawn_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = TokioProcessorRunner
            .run(
                &command(dir.path().join("no-such-java"), dir.path()),
                &CancellationToken::new(),
            )
            .await;
        assert!(
            matches!(result, Err(ProcessorRunError::Spawn(_))),
            "{result:?}"
        );
    }
}
