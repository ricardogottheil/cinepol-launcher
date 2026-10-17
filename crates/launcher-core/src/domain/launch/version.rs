//! The resolved model of a Minecraft version JSON (vanilla or a loader
//! profile that `inheritsFrom` one).

use std::collections::BTreeMap;

use super::{Artifact, MavenCoordinate, Rule};
use crate::domain::instance::GameVersion;
use crate::domain::java::JavaRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionKind {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

impl VersionKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Snapshot => "snapshot",
            Self::OldBeta => "old_beta",
            Self::OldAlpha => "old_alpha",
        }
    }
}

/// One entry of Mojang's version manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSummary {
    pub id: GameVersion,
    pub kind: VersionKind,
    /// RFC 3339 release time, kept as text for display and sorting.
    pub release_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionManifest {
    pub latest_release: GameVersion,
    pub latest_snapshot: GameVersion,
    /// Newest first, as Mojang publishes it.
    pub versions: Vec<VersionSummary>,
}

/// A launch argument: always present, or only when its rules allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentToken {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        values: Vec<String>,
    },
}

/// Legacy natives: a classifier per OS, extracted into the natives directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyNatives {
    /// `os.name` -> classifier, which may contain `${arch}` ("natives-windows-${arch}").
    pub classifiers: BTreeMap<String, String>,
    /// Classifier -> downloadable jar.
    pub artifacts: BTreeMap<String, Artifact>,
    /// Path prefixes not to extract (usually `META-INF/`).
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub name: MavenCoordinate,
    /// The jar for the classpath; `None` for natives-only entries.
    pub artifact: Option<Artifact>,
    pub natives: Option<LegacyNatives>,
    pub rules: Vec<Rule>,
}

/// A logging configuration file the game reads (log4j2 XML).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    /// JVM argument with `${path}`, e.g. `-Dlog4j.configurationFile=${path}`.
    pub argument: String,
    pub file: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetIndexRef {
    pub id: String,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSpec {
    pub id: String,
    pub kind: VersionKind,
    pub main_class: String,
    pub game_arguments: Vec<ArgumentToken>,
    /// Empty for legacy versions; the launcher then supplies defaults.
    pub jvm_arguments: Vec<ArgumentToken>,
    /// Pre-1.13 `minecraftArguments`, already split on spaces.
    pub legacy_game_arguments: Option<Vec<String>>,
    pub libraries: Vec<Library>,
    pub client_jar: Option<Artifact>,
    pub asset_index: Option<AssetIndexRef>,
    pub java: Option<JavaRequirement>,
    pub logging: Option<LoggingConfig>,
    /// The parent version id for loader profiles.
    pub inherits_from: Option<String>,
}

impl VersionSpec {
    /// Applies a loader profile (`self`) over the version it inherits from.
    /// The child's libraries come first so they win classpath order and
    /// deduplication; its arguments are appended; anything it leaves unset
    /// comes from the parent.
    pub fn merged_over(self, parent: VersionSpec) -> VersionSpec {
        let mut libraries = self.libraries;
        libraries.extend(parent.libraries);

        let legacy_game_arguments = self.legacy_game_arguments.or(parent.legacy_game_arguments);
        let game_arguments = if legacy_game_arguments.is_some() && self.game_arguments.is_empty() {
            parent.game_arguments
        } else {
            [parent.game_arguments, self.game_arguments].concat()
        };

        VersionSpec {
            id: self.id,
            kind: self.kind,
            main_class: if self.main_class.is_empty() {
                parent.main_class
            } else {
                self.main_class
            },
            game_arguments,
            jvm_arguments: [parent.jvm_arguments, self.jvm_arguments].concat(),
            legacy_game_arguments,
            libraries,
            client_jar: self.client_jar.or(parent.client_jar),
            asset_index: self.asset_index.or(parent.asset_index),
            java: self.java.or(parent.java),
            logging: self.logging.or(parent.logging),
            inherits_from: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::launch::RelativePath;

    fn library(coordinate: &str) -> Library {
        let name = MavenCoordinate::parse(coordinate).unwrap();
        Library {
            artifact: Some(Artifact {
                path: name.path(),
                url: format!("https://repo/{}", name.path()),
                checksum: None,
                size: None,
            }),
            name,
            natives: None,
            rules: vec![],
        }
    }

    fn spec(id: &str, main_class: &str, libraries: Vec<Library>) -> VersionSpec {
        VersionSpec {
            id: id.into(),
            kind: VersionKind::Release,
            main_class: main_class.into(),
            game_arguments: vec![ArgumentToken::Plain(format!("--from-{id}"))],
            jvm_arguments: vec![ArgumentToken::Plain(format!("-Dfrom.{id}=1"))],
            legacy_game_arguments: None,
            libraries,
            client_jar: None,
            asset_index: None,
            java: None,
            logging: None,
            inherits_from: None,
        }
    }

    #[test]
    fn a_loader_profile_merges_over_its_parent() {
        let mut parent = spec(
            "26.2",
            "net.minecraft.client.main.Main",
            vec![library("org.ow2.asm:asm:9.6")],
        );
        parent.client_jar = Some(Artifact {
            path: RelativePath::parse("versions/26.2/26.2.jar").unwrap(),
            url: "https://piston-data/client.jar".into(),
            checksum: None,
            size: None,
        });
        parent.java = Some(JavaRequirement {
            component: "java-runtime-epsilon".into(),
            major: 25,
        });
        let mut child = spec(
            "fabric-loader-0.17.2-26.2",
            "net.fabricmc.loader.impl.launch.knot.KnotClient",
            vec![library("org.ow2.asm:asm:9.8")],
        );
        child.inherits_from = Some("26.2".into());

        let merged = child.merged_over(parent);

        assert_eq!(merged.id, "fabric-loader-0.17.2-26.2");
        assert_eq!(
            merged.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(
            merged.libraries[0].name.version, "9.8",
            "child libraries come first"
        );
        assert_eq!(merged.libraries.len(), 2);
        assert_eq!(
            merged.game_arguments,
            vec![
                ArgumentToken::Plain("--from-26.2".into()),
                ArgumentToken::Plain("--from-fabric-loader-0.17.2-26.2".into())
            ]
        );
        assert!(merged.client_jar.is_some());
        assert_eq!(merged.java.map(|java| java.major), Some(25));
        assert_eq!(merged.inherits_from, None);
    }

    #[test]
    fn an_empty_child_main_class_keeps_the_parent_one() {
        let merged = spec("child", "", vec![]).merged_over(spec(
            "parent",
            "net.minecraft.client.main.Main",
            vec![],
        ));
        assert_eq!(merged.main_class, "net.minecraft.client.main.Main");
    }
}
