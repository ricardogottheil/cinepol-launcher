//! Game process runner over `tokio::process`.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::domain::launch::{
    GameExit, GameProcessError, GameProcessRunner, LogSink, LogStream, ProcessSpec,
};

/// How long to wait for the output readers after the game ends. A child the
/// game started may keep the pipes open; past this the readers are dropped.
const READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Hides the console window of `java.exe` on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Spawns the game, streams stdout and stderr line by line, and kills it
/// when the token is cancelled.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioGameProcessRunner;

impl GameProcessRunner for TokioGameProcessRunner {
    async fn run<S: LogSink>(
        &self,
        spec: ProcessSpec,
        logs: S,
        kill: CancellationToken,
    ) -> Result<GameExit, GameProcessError> {
        let mut command = Command::new(&spec.java);
        command
            .args(spec.command.args())
            .current_dir(&spec.working_dir)
            .envs(spec.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        tracing::info!(
            java = %spec.java.display(),
            working_dir = %spec.working_dir.display(),
            args = ?spec.command.redacted_args(),
            "starting the game"
        );
        let mut child = command.spawn().map_err(|error| {
            GameProcessError::Spawn(format!("{}: {error}", spec.java.display()))
        })?;

        let logs = Arc::new(logs);
        let readers: Vec<JoinHandle<()>> =
            [
                child.stdout.take().map(|out| {
                    tokio::spawn(forward_lines(out, LogStream::Stdout, Arc::clone(&logs)))
                }),
                child.stderr.take().map(|err| {
                    tokio::spawn(forward_lines(err, LogStream::Stderr, Arc::clone(&logs)))
                }),
            ]
            .into_iter()
            .flatten()
            .collect();

        let (status, killed_by_launcher) = tokio::select! {
            status = child.wait() => (status, false),
            () = kill.cancelled() => {
                if let Err(error) = child.start_kill() {
                    tracing::warn!(%error, "could not kill the game");
                }
                (child.wait().await, true)
            }
        };
        drain(readers).await;

        let status = status.map_err(|error| GameProcessError::Wait(error.to_string()))?;
        tracing::info!(code = ?status.code(), killed_by_launcher, "the game exited");
        Ok(GameExit {
            code: status.code(),
            killed_by_launcher,
        })
    }
}

/// Forwards each line (lossy UTF-8, without the trailing `\n` or `\r\n`).
async fn forward_lines<R, S>(reader: R, stream: LogStream, logs: Arc<S>)
where
    R: AsyncRead + Unpin,
    S: LogSink,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) => break,
            Ok(_) => {
                if buffer.last() == Some(&b'\n') {
                    buffer.pop();
                    if buffer.last() == Some(&b'\r') {
                        buffer.pop();
                    }
                }
                logs.line(stream, String::from_utf8_lossy(&buffer).into_owned());
            }
            Err(error) => {
                tracing::warn!(?stream, %error, "stopped reading the game output");
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
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::domain::launch::{LaunchCommand, LogStream};

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<(LogStream, String)>>>);

    impl RecordingSink {
        fn lines(&self, stream: LogStream) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(s, _)| *s == stream)
                .map(|(_, line)| line.clone())
                .collect()
        }
    }

    impl LogSink for RecordingSink {
        fn line(&self, stream: LogStream, line: String) {
            self.0.lock().unwrap().push((stream, line));
        }
    }

    fn shell(script: &str, working_dir: PathBuf) -> ProcessSpec {
        ProcessSpec {
            java: PathBuf::from("/bin/sh"),
            command: LaunchCommand::new(vec!["-c".into(), script.into()], "sh", vec![]),
            working_dir,
            env: vec![("CINEPOL_TEST_VALUE".into(), "from-env".into())],
        }
    }

    #[tokio::test]
    async fn captures_the_exit_code_and_both_output_streams() {
        let dir = tempfile::tempdir().unwrap();
        let sink = RecordingSink::default();
        let exit = TokioGameProcessRunner
            .run(
                shell(
                    "echo out1; echo err1 >&2; printf 'crlf\\r\\n'; echo \"$CINEPOL_TEST_VALUE\"; pwd; exit 3",
                    dir.path().to_path_buf(),
                ),
                sink.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            exit,
            GameExit {
                code: Some(3),
                killed_by_launcher: false
            }
        );
        let cwd = dir.path().canonicalize().unwrap().display().to_string();
        assert_eq!(
            sink.lines(LogStream::Stdout),
            ["out1", "crlf", "from-env", cwd.as_str()]
        );
        assert_eq!(sink.lines(LogStream::Stderr), ["err1"]);
    }

    #[tokio::test]
    async fn invalid_utf8_output_is_decoded_lossily() {
        let dir = tempfile::tempdir().unwrap();
        let sink = RecordingSink::default();
        let exit = TokioGameProcessRunner
            .run(
                shell("printf 'a\\377b\\n'", dir.path().to_path_buf()),
                sink.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(exit.success());
        assert_eq!(sink.lines(LogStream::Stdout), ["a\u{fffd}b"]);
    }

    #[tokio::test]
    async fn cancelling_the_token_kills_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let kill = CancellationToken::new();
        let trigger = kill.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let exit = TokioGameProcessRunner
            .run(
                shell("sleep 30", dir.path().to_path_buf()),
                RecordingSink::default(),
                kill,
            )
            .await
            .unwrap();
        assert!(exit.killed_by_launcher);
        assert!(!exit.success());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_missing_executable_is_a_spawn_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = shell("exit 0", dir.path().to_path_buf());
        spec.java = dir.path().join("no-such-java");
        let result = TokioGameProcessRunner
            .run(spec, RecordingSink::default(), CancellationToken::new())
            .await;
        assert!(
            matches!(&result, Err(GameProcessError::Spawn(_))),
            "{result:?}"
        );
    }
}
