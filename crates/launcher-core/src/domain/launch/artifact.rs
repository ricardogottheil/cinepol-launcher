//! Downloadable files: validated relative paths, Maven coordinates, artifacts.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::integrity::Checksum;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArtifactError {
    #[error("unsafe relative path {0:?}")]
    UnsafePath(String),
    #[error("invalid Maven coordinate {0:?}")]
    InvalidCoordinate(String),
}

/// A forward-slash relative path that cannot escape its root: no absolute
/// paths, drive letters, backslashes, empty, `.` or `..` segments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(raw: &str) -> Result<Self, ArtifactError> {
        let unsafe_path = raw.is_empty()
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains(':')
            || raw.contains('\0')
            || raw
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..");
        if unsafe_path {
            Err(ArtifactError::UnsafePath(raw.to_owned()))
        } else {
            Ok(Self(raw.to_owned()))
        }
    }

    /// Whether `raw` is a single safe file name: a relative path with one
    /// segment, so joining it under a folder stays in that folder.
    pub fn is_file_name(raw: &str) -> bool {
        Self::parse(raw).is_ok() && !raw.contains('/')
    }

    /// For paths built only from fixed segments and validated hex digests.
    pub(super) fn from_hex_digest_path(path: String) -> Self {
        Self(path)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Joins onto `root` segment by segment, using the OS separator.
    pub fn under(&self, root: &Path) -> PathBuf {
        self.0
            .split('/')
            .fold(root.to_path_buf(), |path, segment| path.join(segment))
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `group:artifact:version[:classifier][@extension]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenCoordinate {
    pub fn parse(raw: &str) -> Result<Self, ArtifactError> {
        let invalid = || ArtifactError::InvalidCoordinate(raw.to_owned());
        let (coords, extension) = match raw.split_once('@') {
            Some((coords, ext)) => (coords, ext.to_owned()),
            None => (raw, "jar".to_owned()),
        };
        let parts: Vec<&str> = coords.split(':').collect();
        let (group, artifact, version, classifier) = match parts.as_slice() {
            [g, a, v] => (*g, *a, *v, None),
            [g, a, v, c] => (*g, *a, *v, Some((*c).to_owned())),
            _ => return Err(invalid()),
        };
        let safe = |part: &str| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        };
        let all = [group, artifact, version, extension.as_str()];
        if !all.iter().all(|part| safe(part)) || classifier.as_deref().is_some_and(|c| !safe(c)) {
            return Err(invalid());
        }
        Ok(Self {
            group: group.to_owned(),
            artifact: artifact.to_owned(),
            version: version.to_owned(),
            classifier,
            extension,
        })
    }

    /// Repository layout path: `org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-linux.jar`.
    pub fn path(&self) -> RelativePath {
        let classifier = self
            .classifier
            .as_deref()
            .map(|c| format!("-{c}"))
            .unwrap_or_default();
        RelativePath(format!(
            "{}/{}/{}/{}-{}{}.{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            classifier,
            self.extension
        ))
    }

    /// Identity used to deduplicate libraries: group, artifact and classifier.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.group,
            self.artifact,
            self.classifier.as_deref().unwrap_or("")
        )
    }
}

impl fmt::Display for MavenCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.group, self.artifact, self.version)?;
        if let Some(classifier) = &self.classifier {
            write!(f, ":{classifier}")?;
        }
        if self.extension != "jar" {
            write!(f, "@{}", self.extension)?;
        }
        Ok(())
    }
}

/// A file to download into a launcher directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: RelativePath,
    pub url: String,
    /// Declared digest; launching refuses libraries whose digest does not match.
    pub checksum: Option<Checksum>,
    pub size: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_reject_escapes() {
        assert!(RelativePath::parse("org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar").is_ok());
        for raw in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../b",
            "a//b",
            "C:/x",
            "a\\b",
            "./a",
            "a/.",
        ] {
            assert!(RelativePath::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn relative_paths_join_with_the_os_separator() {
        let path = RelativePath::parse("a/b/c.jar").unwrap();
        assert_eq!(
            path.under(Path::new("root")),
            Path::new("root").join("a").join("b").join("c.jar")
        );
    }

    #[test]
    fn maven_coordinates_map_to_repository_paths() {
        let cases = [
            (
                "org.lwjgl:lwjgl:3.4.1",
                "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar",
            ),
            (
                "org.lwjgl:lwjgl:3.4.1:natives-macos-arm64",
                "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-macos-arm64.jar",
            ),
            (
                "de.oceanlabs.mcp:mcp_config:1.21-20240613@zip",
                "de/oceanlabs/mcp/mcp_config/1.21-20240613/mcp_config-1.21-20240613.zip",
            ),
        ];
        for (coordinate, path) in cases {
            let parsed = MavenCoordinate::parse(coordinate).unwrap();
            assert_eq!(parsed.path().as_str(), path);
            assert_eq!(parsed.to_string(), coordinate);
        }
    }

    #[test]
    fn maven_coordinates_reject_traversal_and_bad_shapes() {
        for raw in [
            "a:b",
            "a:b:c:d:e",
            "..:b:1",
            "a:b:1:../../x",
            "a/b:c:1",
            "a:b:1@../x",
            "a: b:1",
        ] {
            assert!(MavenCoordinate::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn dedup_key_ignores_the_version() {
        let old = MavenCoordinate::parse("org.ow2.asm:asm:9.6").unwrap();
        let new = MavenCoordinate::parse("org.ow2.asm:asm:9.8").unwrap();
        assert_eq!(old.key(), new.key());
    }
}
