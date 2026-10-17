//! Which libraries a version needs on this platform.

use std::collections::HashSet;

use super::{Artifact, Features, Platform, VersionSpec, rules_allow};

/// A legacy natives jar to extract into the natives directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeJar {
    pub artifact: Artifact,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryPlan {
    /// Library jars in classpath order (relative to the libraries directory).
    pub classpath: Vec<Artifact>,
    /// Legacy natives to extract (relative to the libraries directory).
    pub natives: Vec<NativeJar>,
}

impl LibraryPlan {
    /// Every library file to download.
    pub fn downloads(&self) -> impl Iterator<Item = &Artifact> {
        self.classpath
            .iter()
            .chain(self.natives.iter().map(|native| &native.artifact))
    }
}

/// Applies rules, picks legacy native classifiers for the platform and
/// deduplicates by `group:artifact:classifier` (the first entry wins, so a
/// loader's newer library beats vanilla's).
pub fn plan_libraries(spec: &VersionSpec, platform: &Platform, features: &Features) -> LibraryPlan {
    let mut plan = LibraryPlan::default();
    let mut on_classpath = HashSet::new();
    let mut natives_seen = HashSet::new();

    for library in spec
        .libraries
        .iter()
        .filter(|library| rules_allow(&library.rules, platform, features))
    {
        if let Some(artifact) = &library.artifact
            && on_classpath.insert(library.name.key())
        {
            plan.classpath.push(artifact.clone());
        }

        let Some(natives) = &library.natives else {
            continue;
        };
        let Some(classifier) = natives.classifiers.get(platform.mojang_os_name()) else {
            continue;
        };
        let classifier = classifier.replace("${arch}", platform.bitness());
        if let Some(artifact) = natives.artifacts.get(&classifier)
            && natives_seen.insert(artifact.path.clone())
        {
            plan.natives.push(NativeJar {
                artifact: artifact.clone(),
                exclude: natives.exclude.clone(),
            });
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::launch::{
        Arch, LegacyNatives, Library, MavenCoordinate, OsCondition, OsName, Rule, RuleAction,
        VersionKind,
    };

    fn artifact_for(name: &MavenCoordinate) -> Artifact {
        Artifact {
            path: name.path(),
            url: format!("https://libraries.example/{}", name.path()),
            checksum: None,
            size: None,
        }
    }

    fn library(coordinate: &str, os: Option<&str>) -> Library {
        let name = MavenCoordinate::parse(coordinate).unwrap();
        let rules = os
            .map(|os| {
                vec![Rule {
                    action: RuleAction::Allow,
                    os: Some(OsCondition {
                        name: Some(os.into()),
                        ..Default::default()
                    }),
                    features: BTreeMap::new(),
                }]
            })
            .unwrap_or_default();
        Library {
            artifact: Some(artifact_for(&name)),
            name,
            natives: None,
            rules,
        }
    }

    fn legacy_natives(coordinate: &str) -> Library {
        let name = MavenCoordinate::parse(coordinate).unwrap();
        let classifiers = BTreeMap::from([
            ("linux".to_owned(), "natives-linux".to_owned()),
            ("osx".to_owned(), "natives-osx".to_owned()),
            ("windows".to_owned(), "natives-windows-${arch}".to_owned()),
        ]);
        let artifacts = [
            "natives-linux",
            "natives-osx",
            "natives-windows-64",
            "natives-windows-32",
        ]
        .into_iter()
        .map(|classifier| {
            let mut native = name.clone();
            native.classifier = Some(classifier.to_owned());
            (classifier.to_owned(), artifact_for(&native))
        })
        .collect();
        Library {
            name,
            artifact: None,
            natives: Some(LegacyNatives {
                classifiers,
                artifacts,
                exclude: vec!["META-INF/".into()],
            }),
            rules: vec![],
        }
    }

    fn spec(libraries: Vec<Library>) -> VersionSpec {
        VersionSpec {
            id: "test".into(),
            kind: VersionKind::Release,
            main_class: "Main".into(),
            game_arguments: vec![],
            jvm_arguments: vec![],
            legacy_game_arguments: None,
            libraries,
            client_jar: None,
            asset_index: None,
            java: None,
            logging: None,
            inherits_from: None,
        }
    }

    fn paths(artifacts: &[Artifact]) -> Vec<&str> {
        artifacts
            .iter()
            .map(|artifact| artifact.path.as_str())
            .collect()
    }

    #[test]
    fn modern_native_jars_follow_os_rules_for_every_architecture() {
        let spec = spec(vec![
            library("org.lwjgl:lwjgl:3.4.1", None),
            library("org.lwjgl:lwjgl:3.4.1:natives-linux", Some("linux")),
            library("org.lwjgl:lwjgl:3.4.1:natives-macos", Some("osx")),
            library("org.lwjgl:lwjgl:3.4.1:natives-macos-arm64", Some("osx")),
            library(
                "org.lwjgl:lwjgl:3.4.1:natives-windows-arm64",
                Some("windows"),
            ),
        ]);
        let mac_arm = plan_libraries(
            &spec,
            &Platform::new(OsName::MacOs, Arch::Arm64),
            &Features::new(),
        );
        assert_eq!(
            mac_arm.classpath.len(),
            3,
            "both macOS jars: LWJGL picks the right one at runtime"
        );
        let linux_arm = plan_libraries(
            &spec,
            &Platform::new(OsName::Linux, Arch::Arm64),
            &Features::new(),
        );
        assert_eq!(
            paths(&linux_arm.classpath),
            [
                "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar",
                "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-linux.jar"
            ]
        );
        assert!(linux_arm.natives.is_empty());
    }

    #[test]
    fn legacy_natives_pick_the_classifier_and_bitness_per_platform() {
        let spec = spec(vec![legacy_natives("org.lwjgl.lwjgl:lwjgl-platform:2.9.4")]);
        let cases = [
            (Platform::new(OsName::Linux, Arch::X86_64), "natives-linux"),
            (Platform::new(OsName::MacOs, Arch::X86_64), "natives-osx"),
            (
                Platform::new(OsName::Windows, Arch::X86_64),
                "natives-windows-64",
            ),
            (
                Platform::new(OsName::Windows, Arch::X86),
                "natives-windows-32",
            ),
        ];
        for (platform, classifier) in cases {
            let plan = plan_libraries(&spec, &platform, &Features::new());
            assert!(plan.classpath.is_empty());
            assert_eq!(plan.natives.len(), 1);
            assert!(
                plan.natives[0]
                    .artifact
                    .path
                    .as_str()
                    .ends_with(&format!("{classifier}.jar"))
            );
            assert_eq!(plan.natives[0].exclude, ["META-INF/"]);
        }
    }

    #[test]
    fn the_first_library_with_a_key_wins() {
        let spec = spec(vec![
            library("org.ow2.asm:asm:9.8", None),
            library("org.ow2.asm:asm:9.6", None),
        ]);
        let plan = plan_libraries(
            &spec,
            &Platform::new(OsName::Linux, Arch::X86_64),
            &Features::new(),
        );
        assert_eq!(paths(&plan.classpath), ["org/ow2/asm/asm/9.8/asm-9.8.jar"]);
        assert_eq!(plan.downloads().count(), 1);
    }
}
