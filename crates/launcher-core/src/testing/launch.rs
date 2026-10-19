//! Fakes for the launch ports: version source, Java runtimes, natives
//! extraction, the game process and log sinks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

use crate::domain::instance::InstanceId;
use crate::domain::java::{JavaRequirement, JavaRuntime, JavaRuntimeError, JavaRuntimeProvider};
use crate::domain::launch::{
    AssetIndex, AssetIndexRef, ExtractError, GameExit, GameProcessError, GameProcessRunner,
    LogSink, LogStream, NativeJar, NativesExtractor, Platform, ProcessSpec, RunningGames,
    VersionManifest, VersionSource, VersionSourceError, VersionSpec,
};
use crate::domain::ports::{FileSystem, FileSystemError, ProgressReporter, ProgressUpdate};

/// Serves fixed version specs and asset indexes by id.
#[derive(Debug, Clone)]
pub struct FakeVersionSource {
    pub manifest: VersionManifest,
    pub versions: HashMap<String, VersionSpec>,
    pub asset_indexes: HashMap<String, AssetIndex>,
}

impl VersionSource for FakeVersionSource {
    async fn manifest(&self) -> Result<VersionManifest, VersionSourceError> {
        Ok(self.manifest.clone())
    }

    async fn version(&self, id: &str) -> Result<VersionSpec, VersionSourceError> {
        self.versions
            .get(id)
            .cloned()
            .ok_or_else(|| VersionSourceError::UnknownVersion(id.to_owned()))
    }

    async fn asset_index(&self, index: &AssetIndexRef) -> Result<AssetIndex, VersionSourceError> {
        self.asset_indexes
            .get(&index.id)
            .cloned()
            .ok_or_else(|| VersionSourceError::Invalid(format!("no index {}", index.id)))
    }
}

/// Always provides the same runtime and records the requested major versions.
#[derive(Debug)]
pub struct FakeJavaRuntimes {
    pub runtime: Result<JavaRuntime, JavaRuntimeError>,
    requested: Mutex<Vec<u32>>,
}

impl FakeJavaRuntimes {
    pub fn providing(runtime: JavaRuntime) -> Self {
        Self {
            runtime: Ok(runtime),
            requested: Mutex::default(),
        }
    }

    pub fn failing(error: JavaRuntimeError) -> Self {
        Self {
            runtime: Err(error),
            requested: Mutex::default(),
        }
    }

    pub fn requested(&self) -> Vec<u32> {
        self.requested.lock().unwrap().clone()
    }
}

impl JavaRuntimeProvider for FakeJavaRuntimes {
    async fn ensure<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        _platform: &Platform,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<JavaRuntime, JavaRuntimeError> {
        if cancel.is_cancelled() {
            return Err(JavaRuntimeError::Cancelled);
        }
        self.requested.lock().unwrap().push(requirement.major);
        reporter.report(ProgressUpdate::new(1, Some(1)));
        self.runtime.clone()
    }
}

/// Records what it was asked to extract; touches no disk.
#[derive(Debug, Default)]
pub struct RecordingNativesExtractor {
    calls: Mutex<Vec<(Vec<PathBuf>, PathBuf)>>,
}

impl RecordingNativesExtractor {
    pub fn calls(&self) -> Vec<(Vec<PathBuf>, PathBuf)> {
        self.calls.lock().unwrap().clone()
    }
}

impl NativesExtractor for RecordingNativesExtractor {
    async fn extract(
        &self,
        jars: &[(PathBuf, NativeJar)],
        destination: &Path,
    ) -> Result<(), ExtractError> {
        self.calls.lock().unwrap().push((
            jars.iter().map(|(path, _)| path.clone()).collect(),
            destination.to_path_buf(),
        ));
        Ok(())
    }
}

/// Collects log lines; clones share the same buffer.
#[derive(Debug, Clone, Default)]
pub struct RecordingLogSink {
    lines: Arc<Mutex<Vec<(LogStream, String)>>>,
}

impl RecordingLogSink {
    pub fn lines(&self) -> Vec<(LogStream, String)> {
        self.lines.lock().unwrap().clone()
    }
}

impl LogSink for RecordingLogSink {
    fn line(&self, stream: LogStream, line: String) {
        self.lines.lock().unwrap().push((stream, line));
    }
}

/// Emits scripted output, then exits with `exit` (or, when `until_killed`,
/// keeps running until `kill` fires); records every spec it ran.
#[derive(Debug)]
pub struct FakeGameProcess {
    pub output: Vec<(LogStream, String)>,
    pub exit: GameExit,
    pub until_killed: bool,
    specs: Mutex<Vec<ProcessSpec>>,
}

impl FakeGameProcess {
    pub fn exiting_with(code: i32) -> Self {
        Self {
            output: vec![],
            exit: GameExit {
                code: Some(code),
                killed_by_launcher: false,
            },
            until_killed: false,
            specs: Mutex::default(),
        }
    }

    /// A game that only exits when the launcher kills it.
    pub fn running_until_killed() -> Self {
        Self {
            until_killed: true,
            ..Self::exiting_with(0)
        }
    }

    pub fn exiting_with_output(code: i32, output: Vec<(LogStream, String)>) -> Self {
        Self {
            output,
            ..Self::exiting_with(code)
        }
    }

    pub fn specs(&self) -> Vec<ProcessSpec> {
        self.specs.lock().unwrap().clone()
    }
}

impl GameProcessRunner for FakeGameProcess {
    async fn run<S: LogSink>(
        &self,
        spec: ProcessSpec,
        logs: S,
        kill: CancellationToken,
    ) -> Result<GameExit, GameProcessError> {
        self.specs.lock().unwrap().push(spec);
        for (stream, line) in &self.output {
            logs.line(*stream, line.clone());
        }
        if self.until_killed {
            kill.cancelled().await;
        }
        if kill.is_cancelled() {
            return Ok(GameExit {
                code: None,
                killed_by_launcher: true,
            });
        }
        Ok(self.exit)
    }
}

/// Tracks which paths "exist" without touching the disk.
#[derive(Debug, Default)]
pub struct FakeFileSystem {
    paths: Mutex<HashSet<PathBuf>>,
    removed: Mutex<Vec<PathBuf>>,
    copies: Mutex<Vec<(PathBuf, PathBuf)>>,
    fail_copies: std::sync::atomic::AtomicBool,
    modified: Mutex<Vec<(PathBuf, std::time::SystemTime)>>,
}

impl FakeFileSystem {
    pub fn with(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: Mutex::new(paths.into_iter().collect()),
            removed: Mutex::default(),
            copies: Mutex::default(),
            fail_copies: Default::default(),
            modified: Mutex::default(),
        }
    }

    pub fn add(&self, path: PathBuf) {
        self.paths.lock().unwrap().insert(path);
    }

    pub fn removed(&self) -> Vec<PathBuf> {
        self.removed.lock().unwrap().clone()
    }

    /// A file with its modification time, for `newest_file_since`.
    pub fn add_file_at(&self, path: PathBuf, modified: std::time::SystemTime) {
        self.add(path.clone());
        self.modified.lock().unwrap().push((path, modified));
    }

    /// Makes every later `copy_dir_all` fail (leaving nothing behind, as the port promises).
    pub fn fail_copies(&self) {
        self.fail_copies
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Every `copy_dir_all` call, as (from, to).
    pub fn copies(&self) -> Vec<(PathBuf, PathBuf)> {
        self.copies.lock().unwrap().clone()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.lock().unwrap().contains(path)
    }
}

impl FileSystem for FakeFileSystem {
    async fn exists(&self, path: &Path) -> bool {
        self.contains(path)
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        self.add(path.to_path_buf());
        Ok(())
    }

    async fn remove_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        self.paths
            .lock()
            .unwrap()
            .retain(|existing| !existing.starts_with(path));
        self.removed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    async fn newest_file_since(
        &self,
        dir: &Path,
        since: std::time::SystemTime,
    ) -> Result<Option<PathBuf>, FileSystemError> {
        Ok(self
            .modified
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, modified)| path.parent() == Some(dir) && *modified >= since)
            .max_by_key(|(_, modified)| *modified)
            .map(|(path, _)| path.clone()))
    }

    async fn copy_dir_all(&self, from: &Path, to: &Path) -> Result<(), FileSystemError> {
        if self.fail_copies.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(FileSystemError {
                path: to.display().to_string(),
                reason: "permission denied".into(),
            });
        }
        let mut paths = self.paths.lock().unwrap();
        if paths.iter().any(|existing| existing.starts_with(to)) {
            return Err(FileSystemError {
                path: to.display().to_string(),
                reason: "already exists".into(),
            });
        }
        let copied: Vec<PathBuf> = paths
            .iter()
            .filter_map(|existing| Some(to.join(existing.strip_prefix(from).ok()?)))
            .collect();
        paths.extend(copied);
        self.copies
            .lock()
            .unwrap()
            .push((from.to_path_buf(), to.to_path_buf()));
        Ok(())
    }
}

/// A small but complete vanilla-like version: one library, a client jar,
/// an asset index, a logging config and a Java 25 requirement.
pub fn sample_version_spec(id: &str) -> VersionSpec {
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::launch::{
        ArgumentToken, Artifact, Library, LoggingConfig, MavenCoordinate, RelativePath, VersionKind,
    };

    let sha1 = |c: char| Checksum::parse(HashAlgorithm::Sha1, &c.to_string().repeat(40)).unwrap();
    let artifact = |path: &str, c: char| Artifact {
        path: RelativePath::parse(path).unwrap(),
        url: format!("https://example.invalid/{path}"),
        checksum: Some(sha1(c)),
        size: Some(10),
    };
    let name = MavenCoordinate::parse("org.lwjgl:lwjgl:3.4.1").unwrap();
    VersionSpec {
        id: id.to_owned(),
        kind: VersionKind::Release,
        main_class: "net.minecraft.client.main.Main".to_owned(),
        game_arguments: vec![
            ArgumentToken::Plain("--username".into()),
            ArgumentToken::Plain("${auth_player_name}".into()),
            ArgumentToken::Plain("--uuid".into()),
            ArgumentToken::Plain("${auth_uuid}".into()),
            ArgumentToken::Plain("--accessToken".into()),
            ArgumentToken::Plain("${auth_access_token}".into()),
            ArgumentToken::Plain("--xuid".into()),
            ArgumentToken::Plain("${auth_xuid}".into()),
            ArgumentToken::Conditional {
                rules: vec![crate::domain::launch::Rule {
                    action: crate::domain::launch::RuleAction::Allow,
                    os: None,
                    features: std::collections::BTreeMap::from([(
                        "is_quick_play_multiplayer".to_owned(),
                        true,
                    )]),
                }],
                values: vec![
                    "--quickPlayMultiplayer".into(),
                    "${quickPlayMultiplayer}".into(),
                ],
            },
        ],
        jvm_arguments: vec![
            ArgumentToken::Plain("-cp".into()),
            ArgumentToken::Plain("${classpath}".into()),
        ],
        legacy_game_arguments: None,
        libraries: vec![Library {
            artifact: Some(artifact(name.path().as_str(), 'a')),
            name,
            natives: None,
            rules: vec![],
        }],
        client_jar: Some(artifact(&format!("{id}/{id}.jar"), 'b')),
        asset_index: Some(AssetIndexRef {
            id: "32".into(),
            artifact: artifact("indexes/32.json", 'c'),
        }),
        java: Some(JavaRequirement {
            component: "java-runtime-epsilon".into(),
            major: 25,
        }),
        logging: Some(LoggingConfig {
            argument: "-Dlog4j.configurationFile=${path}".into(),
            file: artifact("log_configs/client-1.21.2.xml", 'd'),
        }),
        inherits_from: None,
    }
}

/// An asset index with two objects.
pub fn sample_asset_index() -> AssetIndex {
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::launch::AssetObject;

    let object = |name: &str, c: char| {
        let hash = c.to_string().repeat(40);
        AssetObject {
            name: name.to_owned(),
            url: format!("https://resources.invalid/{}/{hash}", &hash[..2]),
            hash: Checksum::parse(HashAlgorithm::Sha1, &hash).unwrap(),
            size: 5,
        }
    };
    AssetIndex {
        id: "32".into(),
        objects: vec![
            object("icons/icon_16x16.png", 'e'),
            object("lang/es_es.json", 'f'),
        ],
    }
}

/// Remembers which instances are busy and the stop signal each one claimed with.
#[derive(Debug, Default)]
pub struct InMemoryRunningGames {
    busy: Mutex<HashMap<InstanceId, CancellationToken>>,
}

impl InMemoryRunningGames {
    pub fn is_busy(&self, instance: &InstanceId) -> bool {
        self.busy.lock().unwrap().contains_key(instance)
    }
}

impl RunningGames for InMemoryRunningGames {
    fn claim(&self, instance: &InstanceId, kill: &CancellationToken) -> bool {
        let mut busy = self.busy.lock().unwrap();
        if busy.contains_key(instance) {
            return false;
        }
        busy.insert(instance.clone(), kill.clone());
        true
    }

    fn release(&self, instance: &InstanceId) {
        self.busy.lock().unwrap().remove(instance);
    }

    fn any_busy(&self) -> bool {
        !self.busy.lock().unwrap().is_empty()
    }
}
