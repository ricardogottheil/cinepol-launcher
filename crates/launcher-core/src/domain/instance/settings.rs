use std::collections::BTreeMap;
use std::path::PathBuf;

use thiserror::Error;

/// Minecraft version id: release ("1.21.11", "26.2"), snapshot ("25w14a"),
/// pre-release ("1.21-pre1"). Only checked for a safe charset here; whether it
/// exists is the version manifest's job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GameVersion(String);

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid Minecraft version {0:?}")]
pub struct GameVersionError(pub String);

impl GameVersion {
    pub fn parse(raw: &str) -> Result<Self, GameVersionError> {
        let valid = !raw.is_empty()
            && raw.len() <= 32
            && raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | ' '))
            && !raw.starts_with(['.', ' '])
            && !raw.contains("..");
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(GameVersionError(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub const MIN_MEMORY_MB: u32 = 512;
pub const MAX_MEMORY_MB: u32 = 65_536;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("memory must be between {MIN_MEMORY_MB} and {MAX_MEMORY_MB} MB")]
    MemoryOutOfRange,
    #[error("minimum memory cannot exceed maximum memory")]
    MemoryInverted,
    #[error("window size must be at least 1x1")]
    WindowSize,
    #[error("JVM argument {0:?} contains a control character or is empty")]
    JvmArgument(String),
    #[error("JVM argument {0:?} is managed by the launcher (memory, classpath, natives)")]
    ReservedJvmArgument(String),
    #[error("too many JVM arguments (max {MAX_JVM_ARGS})")]
    TooManyJvmArguments,
    #[error("invalid environment variable name {0:?}")]
    EnvName(String),
    #[error("environment variable {0:?} has a value with a NUL character")]
    EnvValue(String),
}

/// Heap bounds passed as `-Xms` / `-Xmx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySettings {
    min_mb: u32,
    max_mb: u32,
}

impl MemorySettings {
    pub fn new(min_mb: u32, max_mb: u32) -> Result<Self, SettingsError> {
        let range = MIN_MEMORY_MB..=MAX_MEMORY_MB;
        if !range.contains(&min_mb) || !range.contains(&max_mb) {
            return Err(SettingsError::MemoryOutOfRange);
        }
        if min_mb > max_mb {
            return Err(SettingsError::MemoryInverted);
        }
        Ok(Self { min_mb, max_mb })
    }

    pub fn min_mb(&self) -> u32 {
        self.min_mb
    }

    pub fn max_mb(&self) -> u32 {
        self.max_mb
    }
}

impl Default for MemorySettings {
    /// 2-4 GB: enough for vanilla and light modpacks.
    fn default() -> Self {
        Self {
            min_mb: 2048,
            max_mb: 4096,
        }
    }
}

pub const MAX_JVM_ARGS: usize = 128;
/// Arguments the launcher sets itself; letting users override them breaks launches.
const RESERVED_JVM_PREFIXES: [&str; 6] = [
    "-Xms",
    "-Xmx",
    "-cp",
    "-classpath",
    "-Djava.library.path",
    "--class-path",
];

/// Extra JVM arguments, one per element (never split on spaces by us).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JvmArgs(Vec<String>);

impl JvmArgs {
    pub fn parse(args: Vec<String>) -> Result<Self, SettingsError> {
        if args.len() > MAX_JVM_ARGS {
            return Err(SettingsError::TooManyJvmArguments);
        }
        for arg in &args {
            if arg.trim().is_empty() || arg.chars().any(char::is_control) {
                return Err(SettingsError::JvmArgument(arg.clone()));
            }
            if RESERVED_JVM_PREFIXES
                .iter()
                .any(|prefix| arg.starts_with(prefix))
            {
                return Err(SettingsError::ReservedJvmArgument(arg.clone()));
            }
        }
        Ok(Self(args))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// Environment variables for the game process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvVars(BTreeMap<String, String>);

impl EnvVars {
    pub fn parse(vars: BTreeMap<String, String>) -> Result<Self, SettingsError> {
        for (name, value) in &vars {
            let mut chars = name.chars();
            let valid_name = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_name {
                return Err(SettingsError::EnvName(name.clone()));
            }
            if value.contains('\0') {
                return Err(SettingsError::EnvValue(name.clone()));
            }
        }
        Ok(Self(vars))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    width: u32,
    height: u32,
}

impl WindowSize {
    pub fn new(width: u32, height: u32) -> Result<Self, SettingsError> {
        if width == 0 || height == 0 {
            return Err(SettingsError::WindowSize);
        }
        Ok(Self { width, height })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Which Java runs the instance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum JavaSelection {
    /// The launcher picks and downloads the runtime the version requires.
    #[default]
    Managed,
    /// A user-chosen `java` executable, validated with `java -version`.
    Custom(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_versions_accept_releases_snapshots_and_prereleases() {
        for raw in [
            "1.21.11",
            "26.2",
            "25w14a",
            "1.21-pre1",
            "1.20.1-rc1",
            "1.14.4 Pre-Release 1",
        ] {
            assert!(GameVersion::parse(raw).is_ok(), "{raw}");
        }
        for raw in ["", "../1.21", "1.21/x", "1.21;rm", ".hidden"] {
            assert!(GameVersion::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn memory_must_be_in_range_and_ordered() {
        assert!(MemorySettings::new(1024, 4096).is_ok());
        assert_eq!(
            MemorySettings::new(256, 4096),
            Err(SettingsError::MemoryOutOfRange)
        );
        assert_eq!(
            MemorySettings::new(1024, 100_000),
            Err(SettingsError::MemoryOutOfRange)
        );
        assert_eq!(
            MemorySettings::new(4096, 2048),
            Err(SettingsError::MemoryInverted)
        );
    }

    #[test]
    fn jvm_args_reject_control_characters_and_launcher_managed_flags() {
        assert!(
            JvmArgs::parse(vec!["-XX:+UseG1GC".into(), "-Dfile.encoding=UTF-8".into()]).is_ok()
        );
        assert!(matches!(
            JvmArgs::parse(vec!["-Da=1\n-Xmx99g".into()]),
            Err(SettingsError::JvmArgument(_))
        ));
        assert!(matches!(
            JvmArgs::parse(vec![" ".into()]),
            Err(SettingsError::JvmArgument(_))
        ));
        for reserved in ["-Xmx8G", "-Xms1G", "-cp", "-Djava.library.path=/tmp"] {
            assert!(
                matches!(
                    JvmArgs::parse(vec![reserved.into()]),
                    Err(SettingsError::ReservedJvmArgument(_))
                ),
                "{reserved}"
            );
        }
        assert_eq!(
            JvmArgs::parse(vec!["-Da".into(); MAX_JVM_ARGS + 1]),
            Err(SettingsError::TooManyJvmArguments)
        );
    }

    #[test]
    fn env_vars_need_portable_names_and_no_nul_values() {
        let ok = BTreeMap::from([("MESA_GL_VERSION_OVERRIDE".to_owned(), "4.5".to_owned())]);
        assert!(EnvVars::parse(ok).is_ok());
        for bad in ["1ABC", "A-B", "", "A B"] {
            let vars = BTreeMap::from([(bad.to_owned(), "x".to_owned())]);
            assert_eq!(
                EnvVars::parse(vars),
                Err(SettingsError::EnvName(bad.to_owned()))
            );
        }
        let nul = BTreeMap::from([("A".to_owned(), "x\0y".to_owned())]);
        assert_eq!(
            EnvVars::parse(nul),
            Err(SettingsError::EnvValue("A".to_owned()))
        );
    }

    #[test]
    fn window_size_must_not_be_zero() {
        assert!(WindowSize::new(854, 480).is_ok());
        assert_eq!(WindowSize::new(0, 480), Err(SettingsError::WindowSize));
    }
}
