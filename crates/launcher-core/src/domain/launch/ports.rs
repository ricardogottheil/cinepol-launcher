//! Ports for resolving, installing and running a version.

use std::future::Future;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::instance::InstanceId;

use super::{AssetIndex, AssetIndexRef, LaunchCommand, NativeJar, VersionManifest, VersionSpec};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VersionSourceError {
    #[error("Minecraft version {0} does not exist")]
    UnknownVersion(String),
    #[error("version metadata request failed: {0}")]
    Network(String),
    #[error("version metadata is invalid: {0}")]
    Invalid(String),
}

/// Mojang's version catalog. Implementations cache version JSONs on disk so a
/// downloaded version keeps launching offline.
pub trait VersionSource: Send + Sync {
    fn manifest(&self) -> impl Future<Output = Result<VersionManifest, VersionSourceError>> + Send;
    /// The version JSON, with `inheritsFrom` already merged.
    fn version(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<VersionSpec, VersionSourceError>> + Send;
    fn asset_index(
        &self,
        index: &AssetIndexRef,
    ) -> impl Future<Output = Result<AssetIndex, VersionSourceError>> + Send;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExtractError {
    #[error("archive entry {0:?} would escape the destination")]
    UnsafeEntry(String),
    #[error("archive {path} could not be read: {reason}")]
    Corrupt { path: String, reason: String },
    #[error("extraction failed: {0}")]
    Io(String),
}

/// Extracts legacy natives jars (zip) into a directory, honoring `exclude`
/// prefixes and rejecting entries that escape the destination (zip-slip).
pub trait NativesExtractor: Send + Sync {
    fn extract(
        &self,
        jars: &[(PathBuf, NativeJar)],
        destination: &Path,
    ) -> impl Future<Output = Result<(), ExtractError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Receives the game's output line by line.
pub trait LogSink: Send + Sync + 'static {
    fn line(&self, stream: LogStream, line: String);
    /// A decoded entry (see `Log4jSink`); sinks that only show text get its message.
    fn entry(&self, entry: super::log4j::LogEntry) {
        self.line(entry.stream, entry.message);
    }
}

/// How the game process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameExit {
    /// `None` when killed by a signal.
    pub code: Option<i32>,
    pub killed_by_launcher: bool,
}

impl GameExit {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// What to start: the Java executable, the arguments and where.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub java: PathBuf,
    pub command: LaunchCommand,
    pub working_dir: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameProcessError {
    #[error("could not start the game: {0}")]
    Spawn(String),
    #[error("lost track of the game process: {0}")]
    Wait(String),
}

/// Starts the game, streams its output to `logs`, and resolves when it exits.
/// Cancelling `kill` terminates the process.
pub trait GameProcessRunner: Send + Sync {
    fn run<S: LogSink>(
        &self,
        spec: ProcessSpec,
        logs: S,
        kill: CancellationToken,
    ) -> impl Future<Output = Result<GameExit, GameProcessError>> + Send;
}

/// Instances with a game session in progress (installing or playing), so one
/// instance never runs twice at the same time.
pub trait RunningGames: Send + Sync {
    /// Marks `instance` as busy, with `kill` as the signal that stops it.
    /// Returns `false` when it is already busy.
    fn claim(&self, instance: &InstanceId, kill: &CancellationToken) -> bool;
    /// Marks `instance` as free again.
    fn release(&self, instance: &InstanceId);
    /// Whether any instance is busy.
    fn any_busy(&self) -> bool;
    /// Stops new claims, but only when no instance is busy (checked in the
    /// same step). Returns `false`, and freezes nothing, when one is busy.
    /// Used while the launcher installs an update of itself.
    fn freeze(&self) -> bool;
    /// Lets claims through again after `freeze`.
    fn unfreeze(&self);
}
