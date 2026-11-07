//! Launching a version: platform mapping, rules, the version model, library
//! planning, argument building and the ports that resolve, extract and run it.

mod artifact;
mod assets;
mod command;
mod crash;
pub mod log4j;
mod masking;
mod paths;
mod plan;
mod platform;
mod ports;
mod rules;
mod version;
mod version_id;

pub use artifact::{Artifact, ArtifactError, MavenCoordinate, RelativePath};
pub use assets::{AssetIndex, AssetObject};
pub use command::{LaunchCommand, LaunchContext, LaunchIdentity, build_launch_command, substitute};
pub use crash::{
    JVM_CRASH_LOG_PREFIX, is_crash_report_name, is_jvm_crash_log_name, is_plain_file_name,
    redact_crash_text,
};
pub use masking::MaskingLogSink;
pub use paths::LauncherPaths;
pub use plan::{LibraryPlan, NativeJar, plan_libraries};
pub use platform::{Arch, OsName, Platform};
pub use ports::{
    ExtractError, GameExit, GameProcessError, GameProcessRunner, LogSink, LogStream,
    NativesExtractor, ProcessSpec, RunningGames, VersionSource, VersionSourceError,
};
pub use rules::{Features, OsCondition, Rule, RuleAction, rules_allow};
pub use version::{
    ArgumentToken, AssetIndexRef, LegacyNatives, Library, LoggingConfig, VersionKind,
    VersionManifest, VersionSpec, VersionSummary,
};
pub use version_id::{VersionId, VersionIdError};
