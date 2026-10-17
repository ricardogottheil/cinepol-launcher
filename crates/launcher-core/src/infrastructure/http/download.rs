//! Parallel download manager: bounded concurrency, retries with backoff,
//! streaming verification, atomic writes and reuse of valid files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::ReqwestHttpClient;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::ports::{
    DownloadError, DownloadRequest, DownloadSummary, Downloader, ProgressReporter, ProgressUpdate,
};

const MAX_CONCURRENCY: usize = 32;
const RENAME_ATTEMPTS: u32 = 5;
const RENAME_DELAY: Duration = Duration::from_millis(100);
/// Report progress at most once per this many bytes (plus once per file).
const PROGRESS_STEP: u64 = 256 * 1024;
const READ_BUFFER: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadConfig {
    /// Parallel downloads, clamped to `1..=32`.
    pub max_concurrency: usize,
    /// Attempts per file, at least 1.
    pub max_attempts: u32,
    /// First retry delay; it doubles after each attempt.
    pub initial_backoff: Duration,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 8,
            max_attempts: 4,
            initial_backoff: Duration::from_millis(500),
        }
    }
}

/// `Downloader` over reqwest.
#[derive(Debug, Clone)]
pub struct DownloadManager {
    client: reqwest::Client,
    config: DownloadConfig,
}

impl DownloadManager {
    pub fn new(client: reqwest::Client, config: DownloadConfig) -> Self {
        let config = DownloadConfig {
            max_concurrency: config.max_concurrency.clamp(1, MAX_CONCURRENCY),
            max_attempts: config.max_attempts.max(1),
            initial_backoff: config.initial_backoff,
        };
        Self { client, config }
    }

    /// Reuses the client (and its User-Agent and timeouts) with the default config.
    pub fn from_http_client(http: &ReqwestHttpClient) -> Self {
        Self::new(http.inner().clone(), DownloadConfig::default())
    }

    pub fn config(&self) -> DownloadConfig {
        self.config
    }

    async fn fetch<R: ProgressReporter>(
        &self,
        request: &DownloadRequest,
        progress: &BatchProgress<'_, R>,
        cancel: &CancellationToken,
    ) -> Result<Fetched, DownloadError> {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if request
            .checksum
            .as_ref()
            .is_some_and(|c| c.algorithm() == HashAlgorithm::Murmur2)
        {
            return Err(io_error(
                &request.destination,
                "murmur2 checksums are not supported by the downloader",
            ));
        }
        if is_valid(&request.destination, request).await {
            if let Some(size) = request.size {
                progress.add(size);
            }
            progress.flush();
            return Ok(Fetched::Reused);
        }
        if let Some(parent) = request.destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| io_error(parent, &error.to_string()))?;
        }

        let mut backoff = self.config.initial_backoff;
        let mut attempt = 1;
        loop {
            let temp = temp_path(&request.destination);
            let mut counted = 0;
            let result = self
                .attempt(request, &temp, progress, &mut counted, cancel)
                .await;
            let failure = match result {
                Ok(bytes) => match rename_into_place(&temp, &request.destination).await {
                    Ok(()) => {
                        progress.flush();
                        return Ok(Fetched::Downloaded(bytes));
                    }
                    Err(error) => Failure::fatal(error),
                },
                Err(failure) => failure,
            };
            let _ = tokio::fs::remove_file(&temp).await;
            progress.sub(counted);
            if !failure.retryable || attempt >= self.config.max_attempts {
                return Err(failure.error);
            }
            tracing::debug!(url = %request.url, attempt, error = %failure.error, "retrying download");
            tokio::select! {
                () = cancel.cancelled() => return Err(DownloadError::Cancelled),
                () = tokio::time::sleep(backoff) => {}
            }
            backoff = backoff.saturating_mul(2);
            attempt += 1;
        }
    }

    /// One request streamed into `temp`. Returns the bytes written.
    async fn attempt<R: ProgressReporter>(
        &self,
        request: &DownloadRequest,
        temp: &Path,
        progress: &BatchProgress<'_, R>,
        counted: &mut u64,
        cancel: &CancellationToken,
    ) -> Result<u64, Failure> {
        let http = |reason: String, retryable: bool| Failure {
            error: DownloadError::Http {
                url: request.url.clone(),
                reason,
            },
            retryable,
        };
        let sent = tokio::select! {
            () = cancel.cancelled() => return Err(Failure::fatal(DownloadError::Cancelled)),
            sent = self.client.get(&request.url).send() => sent,
        };
        let mut response = sent.map_err(|e| http(e.without_url().to_string(), true))?;
        let status = response.status();
        if !status.is_success() {
            let retryable = status.is_server_error() || status.as_u16() == 429;
            return Err(http(format!("HTTP {status}"), retryable));
        }

        let mut file = tokio::fs::File::create(temp)
            .await
            .map_err(|e| Failure::fatal(io_error(temp, &e.to_string())))?;
        let mut hasher = request.checksum.as_ref().and_then(StreamHasher::new);
        let mut written: u64 = 0;
        loop {
            let chunk = tokio::select! {
                () = cancel.cancelled() => return Err(Failure::fatal(DownloadError::Cancelled)),
                chunk = response.chunk() => chunk,
            };
            let Some(chunk) = chunk.map_err(|e| http(e.without_url().to_string(), true))? else {
                break;
            };
            file.write_all(&chunk)
                .await
                .map_err(|e| Failure::fatal(io_error(temp, &e.to_string())))?;
            if let Some(hasher) = hasher.as_mut() {
                hasher.update(&chunk);
            }
            written += chunk.len() as u64;
            if request.size.is_some() {
                progress.add(chunk.len() as u64);
                *counted += chunk.len() as u64;
            }
        }
        file.flush()
            .await
            .map_err(|e| Failure::fatal(io_error(temp, &e.to_string())))?;
        drop(file);

        if let Some(size) = request.size
            && written != size
        {
            return Err(http(format!("expected {size} bytes, got {written}"), true));
        }
        if let (Some(checksum), Some(hasher)) = (&request.checksum, hasher) {
            let actual = hasher.finish();
            if !checksum.matches(&actual) {
                return Err(Failure {
                    error: DownloadError::ChecksumMismatch {
                        url: request.url.clone(),
                        expected: checksum.value().to_owned(),
                        actual,
                    },
                    retryable: true,
                });
            }
        }
        Ok(written)
    }
}

impl Downloader for DownloadManager {
    async fn download_all<R: ProgressReporter>(
        &self,
        requests: Vec<DownloadRequest>,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<DownloadSummary, DownloadError> {
        let mut seen = HashSet::new();
        let requests: Vec<DownloadRequest> = requests
            .into_iter()
            .filter(|request| seen.insert(request.destination.clone()))
            .collect();
        let total = requests.iter().filter_map(|r| r.size).sum();
        let progress = BatchProgress::new(reporter, total);
        progress.flush();

        // Fail fast: the first error cancels the remaining work, which then
        // finishes cleaning up its temporary files before we return.
        let batch = cancel.child_token();
        // Futures are lazy, so building them up front starts nothing; collecting
        // first also avoids a higher-ranked `Send` inference issue with `map`.
        let pending: Vec<_> = requests
            .iter()
            .map(|request| self.fetch(request, &progress, &batch))
            .collect();
        let mut results =
            futures::stream::iter(pending).buffer_unordered(self.config.max_concurrency);
        let mut summary = DownloadSummary::default();
        let mut first_error = None;
        while let Some(result) = results.next().await {
            match result {
                Ok(Fetched::Reused) => summary.reused += 1,
                Ok(Fetched::Downloaded(bytes)) => {
                    summary.downloaded += 1;
                    summary.bytes += bytes;
                }
                Err(error) => {
                    batch.cancel();
                    first_error.get_or_insert(error);
                }
            }
        }
        drop(results);
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        progress.flush();
        Ok(summary)
    }
}

enum Fetched {
    Reused,
    Downloaded(u64),
}

struct Failure {
    error: DownloadError,
    retryable: bool,
}

impl Failure {
    fn fatal(error: DownloadError) -> Self {
        Self {
            error,
            retryable: false,
        }
    }
}

/// Byte progress shared by every download of a batch.
struct BatchProgress<'a, R: ProgressReporter> {
    reporter: &'a R,
    total: u64,
    done: AtomicU64,
    last_reported: AtomicU64,
}

impl<'a, R: ProgressReporter> BatchProgress<'a, R> {
    fn new(reporter: &'a R, total: u64) -> Self {
        Self {
            reporter,
            total,
            done: AtomicU64::new(0),
            last_reported: AtomicU64::new(0),
        }
    }

    fn add(&self, bytes: u64) {
        let done = self.done.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let last = self.last_reported.load(Ordering::Relaxed);
        if done.saturating_sub(last) >= PROGRESS_STEP
            && self
                .last_reported
                .compare_exchange(last, done, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.report(done);
        }
    }

    /// Takes back the bytes of a failed attempt.
    fn sub(&self, bytes: u64) {
        if bytes > 0 {
            self.done.fetch_sub(bytes, Ordering::Relaxed);
        }
    }

    fn flush(&self) {
        let done = self.done.load(Ordering::Relaxed);
        self.last_reported.store(done, Ordering::Relaxed);
        self.report(done);
    }

    fn report(&self, done: u64) {
        self.reporter
            .report(ProgressUpdate::new(done.min(self.total), Some(self.total)));
    }
}

enum StreamHasher {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}

impl StreamHasher {
    fn new(checksum: &Checksum) -> Option<Self> {
        match checksum.algorithm() {
            HashAlgorithm::Sha1 => Some(Self::Sha1(sha1::Sha1::new())),
            HashAlgorithm::Sha256 => Some(Self::Sha256(sha2::Sha256::new())),
            HashAlgorithm::Sha512 => Some(Self::Sha512(sha2::Sha512::new())),
            HashAlgorithm::Murmur2 => None,
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(data),
            Self::Sha256(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => hex::encode(h.finalize()),
            Self::Sha256(h) => hex::encode(h.finalize()),
            Self::Sha512(h) => hex::encode(h.finalize()),
        }
    }
}

/// Whether the destination already holds the expected file: checksum when
/// declared, else size when known, else mere existence.
async fn is_valid(path: &Path, request: &DownloadRequest) -> bool {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    if let Some(size) = request.size
        && metadata.len() != size
    {
        return false;
    }
    match &request.checksum {
        Some(checksum) => hash_file(path, checksum)
            .await
            .is_some_and(|actual| checksum.matches(&actual)),
        None => true,
    }
}

async fn hash_file(path: &Path, checksum: &Checksum) -> Option<String> {
    let mut hasher = StreamHasher::new(checksum)?;
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut buffer = vec![0u8; READ_BUFFER];
    loop {
        let read = file.read(&mut buffer).await.ok()?;
        if read == 0 {
            return Some(hasher.finish());
        }
        hasher.update(&buffer[..read]);
    }
}

/// A unique `.part` file next to the destination, so the rename stays on one volume.
fn temp_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    destination.with_file_name(format!("{name}.{}.part", uuid::Uuid::new_v4().simple()))
}

/// Renames over the destination, retrying briefly because Windows may hold
/// the file open (antivirus, indexer).
async fn rename_into_place(temp: &Path, destination: &Path) -> Result<(), DownloadError> {
    let mut attempt = 1;
    loop {
        match tokio::fs::rename(temp, destination).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= RENAME_ATTEMPTS => {
                return Err(io_error(destination, &error.to_string()));
            }
            Err(_) => {
                tokio::time::sleep(RENAME_DELAY).await;
                attempt += 1;
            }
        }
    }
}

fn io_error(path: &Path, reason: &str) -> DownloadError {
    DownloadError::Io {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use sha1::{Digest, Sha1};

    use super::*;
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::infrastructure::http::test_server::{Reply, TestServer};
    use crate::testing::RecordingReporter;

    fn manager() -> DownloadManager {
        DownloadManager::new(
            reqwest::Client::new(),
            DownloadConfig {
                max_concurrency: 4,
                max_attempts: 3,
                initial_backoff: Duration::from_millis(1),
            },
        )
    }

    fn sha1_of(body: &[u8]) -> Checksum {
        Checksum::parse(HashAlgorithm::Sha1, &hex::encode(Sha1::digest(body))).unwrap()
    }

    fn request(
        server: &TestServer,
        path: &str,
        destination: PathBuf,
        body: &[u8],
    ) -> DownloadRequest {
        DownloadRequest {
            url: server.url(path),
            destination,
            checksum: Some(sha1_of(body)),
            size: Some(body.len() as u64),
        }
    }

    /// Every file name under `dir`, recursively, relative to it.
    fn files_in(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in std::fs::read_dir(&current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    out.push(
                        path.strip_prefix(dir)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
        }
        out.sort();
        out
    }

    async fn run(requests: Vec<DownloadRequest>) -> Result<DownloadSummary, DownloadError> {
        manager()
            .download_all(
                requests,
                &RecordingReporter::default(),
                &CancellationToken::new(),
            )
            .await
    }

    #[tokio::test]
    async fn downloads_and_verifies_files_into_new_directories() {
        let server = TestServer::start(vec![
            ("/a.jar", vec![Reply::ok("alpha")]),
            ("/b.jar", vec![Reply::ok("bravo!")]),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let summary = run(vec![
            request(&server, "/a.jar", dir.path().join("libs/a.jar"), b"alpha"),
            request(
                &server,
                "/b.jar",
                dir.path().join("libs/x/b.jar"),
                b"bravo!",
            ),
        ])
        .await
        .unwrap();
        assert_eq!(
            summary,
            DownloadSummary {
                downloaded: 2,
                reused: 0,
                bytes: 11
            }
        );
        assert_eq!(
            std::fs::read(dir.path().join("libs/a.jar")).unwrap(),
            b"alpha"
        );
        assert_eq!(
            std::fs::read(dir.path().join("libs/x/b.jar")).unwrap(),
            b"bravo!"
        );
        assert_eq!(files_in(dir.path()), ["libs/a.jar", "libs/x/b.jar"]);
    }

    #[tokio::test]
    async fn reuses_an_existing_valid_file_without_a_request() {
        let server = TestServer::start(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("a.jar");
        std::fs::write(&destination, b"alpha").unwrap();
        let summary = run(vec![request(&server, "/a.jar", destination, b"alpha")])
            .await
            .unwrap();
        assert_eq!(
            summary,
            DownloadSummary {
                downloaded: 0,
                reused: 1,
                bytes: 0
            }
        );
        assert_eq!(server.hits("/a.jar"), 0);
    }

    #[tokio::test]
    async fn reuses_by_size_or_existence_without_a_checksum() {
        let server = TestServer::start(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sized"), b"12345").unwrap();
        std::fs::write(dir.path().join("bare"), b"anything").unwrap();
        let summary = run(vec![
            DownloadRequest {
                url: server.url("/sized"),
                destination: dir.path().join("sized"),
                checksum: None,
                size: Some(5),
            },
            DownloadRequest {
                url: server.url("/bare"),
                destination: dir.path().join("bare"),
                checksum: None,
                size: None,
            },
        ])
        .await
        .unwrap();
        assert_eq!(summary.reused, 2);
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn replaces_an_existing_corrupt_file() {
        let server = TestServer::start(vec![("/a.jar", vec![Reply::ok("alpha")])]).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("a.jar");
        std::fs::write(&destination, b"alphz").unwrap();
        let summary = run(vec![request(
            &server,
            "/a.jar",
            destination.clone(),
            b"alpha",
        )])
        .await
        .unwrap();
        assert_eq!(summary.downloaded, 1);
        assert_eq!(std::fs::read(&destination).unwrap(), b"alpha");
    }

    #[tokio::test]
    async fn checksum_mismatch_fails_after_retries_and_leaves_nothing_behind() {
        let server = TestServer::start(vec![("/a.jar", vec![Reply::ok("tampered")])]).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("libs/a.jar");
        let mut req = request(&server, "/a.jar", destination.clone(), b"original");
        req.size = None;
        let error = run(vec![req]).await.unwrap_err();
        assert_eq!(
            error,
            DownloadError::ChecksumMismatch {
                url: server.url("/a.jar"),
                expected: sha1_of(b"original").value().to_owned(),
                actual: sha1_of(b"tampered").value().to_owned(),
            }
        );
        assert_eq!(server.hits("/a.jar"), 3);
        assert!(!destination.exists());
        assert!(files_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn retries_a_transient_server_error() {
        let server = TestServer::start(vec![(
            "/a.jar",
            vec![Reply::status(500), Reply::ok("alpha")],
        )])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let summary = run(vec![request(
            &server,
            "/a.jar",
            dir.path().join("a.jar"),
            b"alpha",
        )])
        .await
        .unwrap();
        assert_eq!(summary.downloaded, 1);
        assert_eq!(server.hits("/a.jar"), 2);
    }

    #[tokio::test]
    async fn not_found_fails_without_retrying() {
        let server = TestServer::start(vec![]).await;
        let dir = tempfile::tempdir().unwrap();
        let error = run(vec![request(
            &server,
            "/gone.jar",
            dir.path().join("gone.jar"),
            b"x",
        )])
        .await
        .unwrap_err();
        assert!(
            matches!(&error, DownloadError::Http { url, reason } if url == &server.url("/gone.jar") && reason.contains("404")),
            "{error:?}"
        );
        assert_eq!(server.hits("/gone.jar"), 1);
        assert!(files_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn the_first_error_cancels_the_rest_of_the_batch() {
        let body = vec![7u8; 4096];
        let server =
            TestServer::start(vec![("/slow", vec![Reply::stalling(body.clone(), 100)])]).await;
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let slow = request(&server, "/slow", dir.path().join("slow.bin"), &body);
        let missing = request(&server, "/missing", dir.path().join("missing.bin"), b"x");
        let error = run(vec![slow, missing]).await.unwrap_err();
        assert!(matches!(error, DownloadError::Http { .. }), "{error:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(files_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn cancellation_returns_cancelled_and_cleans_temp_files() {
        let body = vec![1u8; 4096];
        let server =
            TestServer::start(vec![("/slow", vec![Reply::stalling(body.clone(), 100)])]).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let result = manager()
            .download_all(
                vec![request(
                    &server,
                    "/slow",
                    dir.path().join("slow.bin"),
                    &body,
                )],
                &RecordingReporter::default(),
                &cancel,
            )
            .await;
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(server.hits("/slow"), 1);
        assert!(files_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn an_already_cancelled_batch_requests_nothing() {
        let server = TestServer::start(vec![("/a.jar", vec![Reply::ok("alpha")])]).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = manager()
            .download_all(
                vec![request(
                    &server,
                    "/a.jar",
                    dir.path().join("a.jar"),
                    b"alpha",
                )],
                &RecordingReporter::default(),
                &cancel,
            )
            .await;
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn duplicate_destinations_are_fetched_once() {
        let server = TestServer::start(vec![("/obj", vec![Reply::ok("asset")])]).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("objects/ab/abcd");
        let req = request(&server, "/obj", destination, b"asset");
        let summary = run(vec![req.clone(), req.clone(), req]).await.unwrap();
        assert_eq!(summary.downloaded, 1);
        assert_eq!(server.hits("/obj"), 1);
    }

    #[tokio::test]
    async fn reports_byte_progress_over_the_whole_batch() {
        let server = TestServer::start(vec![("/new", vec![Reply::ok("0123456789")])]).await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old"), b"hello").unwrap();
        let reporter = RecordingReporter::default();
        manager()
            .download_all(
                vec![
                    request(&server, "/old", dir.path().join("old"), b"hello"),
                    request(&server, "/new", dir.path().join("new"), b"0123456789"),
                ],
                &reporter,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let updates = reporter.updates();
        assert!(!updates.is_empty());
        assert!(updates.iter().all(|u| u.total == Some(15)));
        assert_eq!(updates.last().map(|u| u.done), Some(15));
    }

    #[tokio::test]
    async fn murmur2_checksums_are_rejected_with_a_clear_reason() {
        let server = TestServer::start(vec![("/mod.jar", vec![Reply::ok("mod")])]).await;
        let dir = tempfile::tempdir().unwrap();
        let error = run(vec![DownloadRequest {
            url: server.url("/mod.jar"),
            destination: dir.path().join("mod.jar"),
            checksum: Some(Checksum::parse(HashAlgorithm::Murmur2, "123").unwrap()),
            size: None,
        }])
        .await
        .unwrap_err();
        assert!(
            matches!(&error, DownloadError::Io { reason, .. } if reason.contains("murmur2")),
            "{error:?}"
        );
        assert!(server.requests().is_empty());
    }

    #[test]
    fn concurrency_and_attempts_are_clamped() {
        let manager = DownloadManager::new(
            reqwest::Client::new(),
            DownloadConfig {
                max_concurrency: 100,
                max_attempts: 0,
                initial_backoff: Duration::ZERO,
            },
        );
        assert_eq!(manager.config().max_concurrency, 32);
        assert_eq!(manager.config().max_attempts, 1);
        let config = DownloadManager::new(
            reqwest::Client::new(),
            DownloadConfig {
                max_concurrency: 0,
                ..DownloadConfig::default()
            },
        )
        .config();
        assert_eq!(config.max_concurrency, 1);
    }
}
