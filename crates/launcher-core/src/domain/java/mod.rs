//! Java runtimes: what a game version requires and what is installed.

mod ports;
mod version;

pub use ports::{JavaProbe, JavaProbeError};
pub use version::{JavaVersion, JavaVersionError, parse_java_version_output};

use std::path::PathBuf;

/// The runtime a Minecraft version JSON asks for (`javaVersion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRequirement {
    /// Mojang runtime component, e.g. `java-runtime-delta`.
    pub component: String,
    pub major: u32,
}

impl JavaRequirement {
    /// Versions before 1.17 have no `javaVersion` and run on Java 8.
    pub fn legacy() -> Self {
        Self {
            component: "jre-legacy".to_owned(),
            major: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaSource {
    /// Downloaded by the launcher (Mojang runtime manifest or Adoptium).
    Managed,
    /// Found on the system (PATH, JAVA_HOME, common install folders).
    System,
    /// Chosen by the user for one instance.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntime {
    pub executable: PathBuf,
    pub version: JavaVersion,
    pub source: JavaSource,
}

impl JavaRuntime {
    pub fn satisfies(&self, requirement: &JavaRequirement) -> bool {
        self.version.major == requirement.major
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runtime_satisfies_only_its_exact_major_version() {
        let runtime = JavaRuntime {
            executable: PathBuf::from("java"),
            version: parse_java_version_output("openjdk version \"21.0.8\" 2025-07-15").unwrap(),
            source: JavaSource::System,
        };
        assert!(runtime.satisfies(&JavaRequirement {
            component: "java-runtime-delta".into(),
            major: 21
        }));
        assert!(!runtime.satisfies(&JavaRequirement {
            component: "java-runtime-epsilon".into(),
            major: 25
        }));
        assert!(!runtime.satisfies(&JavaRequirement::legacy()));
    }
}
