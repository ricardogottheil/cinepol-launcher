//! Forge and NeoForge installer processors: the Java tools an installer runs to
//! produce the patched game jar. This module turns an `install_profile.json`
//! (spec 1) into concrete commands; running them is the `ProcessorRunner` port.
//!
//! Substitution, as the official installers do it:
//! - `{KEY}`: a built-in (`SIDE`, `MINECRAFT_JAR`, `ROOT`, `INSTALLER`,
//!   `LIBRARY_DIR`, `MINECRAFT_VERSION`) or a `data` entry, which is itself
//!   `[group:artifact:version[:classifier][@ext]]` (a library path),
//!   `'literal'`, or `/path/in/the/installer` (a file extracted beforehand).
//! - `[coords]` as a whole argument: the library path.
//!
//! A processor without declared outputs is still expected to produce the file
//! after its `--output` argument (checked for presence, not for a hash).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::MavenCoordinate;

/// Only the client is installed.
pub const CLIENT_SIDE: &str = "client";

/// One processor of an install profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessorSpec {
    pub jar: MavenCoordinate,
    pub classpath: Vec<MavenCoordinate>,
    pub args: Vec<String>,
    /// Output path expression -> expected sha1 expression.
    pub outputs: Vec<(String, String)>,
    /// `None` runs on every side.
    pub sides: Option<Vec<String>>,
}

/// Where the built-ins point for this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessorEnvironment {
    pub minecraft_version: String,
    pub minecraft_jar: PathBuf,
    /// The launcher's data root (`{ROOT}`).
    pub root: PathBuf,
    pub libraries_dir: PathBuf,
    pub installer: PathBuf,
    /// Installer files (`/data/client.lzma`) already extracted, by their path in the installer.
    pub data_files: BTreeMap<String, PathBuf>,
}

/// A processor ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedProcessor {
    pub jar: PathBuf,
    /// The jar first, then its classpath, as the installers build it.
    pub classpath: Vec<PathBuf>,
    pub args: Vec<String>,
    pub outputs: Vec<PlannedOutput>,
}

/// A file a processor must produce, and its sha1 when the profile declares one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutput {
    pub path: PathBuf,
    pub sha1: Option<Checksum>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProcessorPlanError {
    #[error("unknown substitution {{{0}}} in the install profile")]
    UnknownKey(String),
    #[error("installer file {0} was not extracted")]
    MissingDataFile(String),
    #[error("invalid library reference {0}")]
    InvalidCoordinate(String),
    #[error("a processor output escapes the data directory: {0}")]
    OutputOutsideRoot(String),
    #[error("a processor output declares an invalid sha1: {0}")]
    InvalidChecksum(String),
}

/// The client processors, in order, with every substitution resolved.
pub fn plan_processors(
    processors: &[ProcessorSpec],
    data: &BTreeMap<String, String>,
    env: &ProcessorEnvironment,
) -> Result<Vec<PlannedProcessor>, ProcessorPlanError> {
    let resolver = Resolver { data, env };
    processors
        .iter()
        .filter(|processor| {
            processor
                .sides
                .as_ref()
                .is_none_or(|sides| sides.iter().any(|side| side == CLIENT_SIDE))
        })
        .map(|processor| {
            let jar = library_path(env, &processor.jar);
            let mut classpath = vec![jar.clone()];
            classpath.extend(processor.classpath.iter().map(|c| library_path(env, c)));
            let args: Vec<String> = processor
                .args
                .iter()
                .map(|arg| resolver.argument(arg))
                .collect::<Result<_, _>>()?;
            let outputs = if processor.outputs.is_empty() {
                implied_outputs(&args, env)
            } else {
                processor
                    .outputs
                    .iter()
                    .map(|(path, sha1)| resolver.output(path, sha1))
                    .collect::<Result<_, _>>()?
            };
            Ok(PlannedProcessor {
                jar,
                classpath,
                args,
                outputs,
            })
        })
        .collect()
}

struct Resolver<'a> {
    data: &'a BTreeMap<String, String>,
    env: &'a ProcessorEnvironment,
}

impl Resolver<'_> {
    /// An argument: a whole `[coords]`, or text with `{KEY}` placeholders.
    fn argument(&self, raw: &str) -> Result<String, ProcessorPlanError> {
        if let Some(coords) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            return self.coordinate(coords);
        }
        let mut resolved = String::new();
        let mut rest = raw;
        while let Some(start) = rest.find('{') {
            let Some(length) = rest[start..].find('}') else {
                break;
            };
            resolved.push_str(&rest[..start]);
            resolved.push_str(&self.key(&rest[start + 1..start + length])?);
            rest = &rest[start + length + 1..];
        }
        resolved.push_str(rest);
        Ok(resolved)
    }

    fn key(&self, key: &str) -> Result<String, ProcessorPlanError> {
        let env = self.env;
        Ok(match key {
            "SIDE" => CLIENT_SIDE.to_owned(),
            "MINECRAFT_JAR" => text(&env.minecraft_jar),
            "ROOT" => text(&env.root),
            "INSTALLER" => text(&env.installer),
            "LIBRARY_DIR" => text(&env.libraries_dir),
            "MINECRAFT_VERSION" => env.minecraft_version.clone(),
            _ => {
                let value = self
                    .data
                    .get(key)
                    .ok_or_else(|| ProcessorPlanError::UnknownKey(key.to_owned()))?;
                self.data_value(value)?
            }
        })
    }

    /// A `data` entry: `[coords]`, `'literal'` or `/file/in/the/installer`.
    fn data_value(&self, value: &str) -> Result<String, ProcessorPlanError> {
        if let Some(coords) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            self.coordinate(coords)
        } else if let Some(literal) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            Ok(literal.to_owned())
        } else if value.starts_with('/') {
            self.env
                .data_files
                .get(value)
                .map(|path| text(path))
                .ok_or_else(|| ProcessorPlanError::MissingDataFile(value.to_owned()))
        } else {
            Ok(value.to_owned())
        }
    }

    fn coordinate(&self, coords: &str) -> Result<String, ProcessorPlanError> {
        let coordinate = MavenCoordinate::parse(coords)
            .map_err(|_| ProcessorPlanError::InvalidCoordinate(coords.to_owned()))?;
        Ok(text(&library_path(self.env, &coordinate)))
    }

    fn output(&self, path: &str, sha1: &str) -> Result<PlannedOutput, ProcessorPlanError> {
        let path = PathBuf::from(self.argument(path)?);
        if !inside(&path, &self.env.root) {
            return Err(ProcessorPlanError::OutputOutsideRoot(text(&path)));
        }
        let sha1 = self.argument(sha1)?;
        let sha1 = if sha1.is_empty() {
            None
        } else {
            Some(
                Checksum::parse(HashAlgorithm::Sha1, &sha1)
                    .map_err(|_| ProcessorPlanError::InvalidChecksum(sha1.clone()))?,
            )
        };
        Ok(PlannedOutput { path, sha1 })
    }
}

/// A processor that declares no outputs (NeoForge) still names one with
/// `--output`: it must exist afterwards, so a deleted file is rebuilt.
fn implied_outputs(args: &[String], env: &ProcessorEnvironment) -> Vec<PlannedOutput> {
    args.windows(2)
        .filter(|pair| pair[0] == "--output")
        .map(|pair| PathBuf::from(&pair[1]))
        .filter(|path| inside(path, &env.root))
        .map(|path| PlannedOutput { path, sha1: None })
        .collect()
}

/// `path` is `root` followed by parts that never climb (`..`) back out. The
/// root itself may hold `..` (a data dir given as `crates/x/../../data`).
fn inside(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rest| {
        !rest
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    })
}

fn library_path(env: &ProcessorEnvironment, coordinate: &MavenCoordinate) -> PathBuf {
    coordinate.path().under(&env.libraries_dir)
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinate(raw: &str) -> MavenCoordinate {
        MavenCoordinate::parse(raw).unwrap()
    }

    fn env() -> ProcessorEnvironment {
        ProcessorEnvironment {
            minecraft_version: "26.2".into(),
            minecraft_jar: PathBuf::from("/data/versions/26.2/26.2.jar"),
            root: PathBuf::from("/data"),
            libraries_dir: PathBuf::from("/data/libraries"),
            installer: PathBuf::from("/data/cache/forge-installer.jar"),
            data_files: BTreeMap::from([(
                "/data/client.lzma".to_owned(),
                PathBuf::from("/data/cache/forge/client.lzma"),
            )]),
        }
    }

    /// The real Forge 26.2-65.1.5 profile: one client processor among server ones.
    fn forge_profile() -> (Vec<ProcessorSpec>, BTreeMap<String, String>) {
        let tools = vec![
            coordinate("net.sf.jopt-simple:jopt-simple:6.0-alpha-3"),
            coordinate("com.github.jponge:lzma-java:1.3"),
        ];
        let processors = vec![
            ProcessorSpec {
                jar: coordinate("net.minecraftforge:installertools:1.4.5"),
                classpath: tools.clone(),
                args: vec![
                    "--task".into(),
                    "BUNDLER_EXTRACT".into(),
                    "--input".into(),
                    "{MINECRAFT_JAR}".into(),
                    "--output".into(),
                    "{ROOT}/libraries/".into(),
                ],
                outputs: vec![],
                sides: Some(vec!["server".into()]),
            },
            ProcessorSpec {
                jar: coordinate("net.minecraftforge:binarypatcher:1.3.4"),
                classpath: tools,
                args: vec![
                    "--clean".into(),
                    "{MINECRAFT_JAR}".into(),
                    "--output".into(),
                    "{PATCHED}".into(),
                    "--apply".into(),
                    "{BINPATCH}".into(),
                    "--data".into(),
                    "--marker".into(),
                    ".forge_patched_minecraft".into(),
                ],
                outputs: vec![("{PATCHED}".into(), "{PATCHED_SHA}".into())],
                sides: Some(vec!["client".into()]),
            },
        ];
        let data = BTreeMap::from([
            ("BINPATCH".to_owned(), "/data/client.lzma".to_owned()),
            (
                "PATCHED".to_owned(),
                "[net.minecraftforge:forge:26.2-65.1.5:client]".to_owned(),
            ),
            (
                "PATCHED_SHA".to_owned(),
                "'be81e6d24785e467d30f30bcf31c18733233d3fe'".to_owned(),
            ),
        ]);
        (processors, data)
    }

    #[test]
    fn plans_only_the_client_processor_of_the_real_forge_profile() {
        let (processors, data) = forge_profile();
        let planned = plan_processors(&processors, &data, &env()).unwrap();

        assert_eq!(planned.len(), 1);
        let patcher = &planned[0];
        assert_eq!(
            patcher.jar,
            PathBuf::from(
                "/data/libraries/net/minecraftforge/binarypatcher/1.3.4/binarypatcher-1.3.4.jar"
            )
        );
        assert_eq!(patcher.classpath.len(), 3, "the jar, then its classpath");
        assert_eq!(patcher.classpath[0], patcher.jar);
        let patched =
            "/data/libraries/net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-client.jar";
        assert_eq!(
            patcher.args,
            [
                "--clean",
                "/data/versions/26.2/26.2.jar",
                "--output",
                patched,
                "--apply",
                "/data/cache/forge/client.lzma",
                "--data",
                "--marker",
                ".forge_patched_minecraft",
            ]
        );
        assert_eq!(
            patcher.outputs,
            [PlannedOutput {
                path: PathBuf::from(patched),
                sha1: Some(
                    Checksum::parse(
                        HashAlgorithm::Sha1,
                        "be81e6d24785e467d30f30bcf31c18733233d3fe"
                    )
                    .unwrap()
                ),
            }]
        );
    }

    #[test]
    fn resolves_built_ins_inside_arguments_and_whole_coordinate_arguments() {
        let processors = vec![ProcessorSpec {
            jar: coordinate("net.neoforged.installertools:installertools:4.0.17:fatjar"),
            classpath: vec![],
            args: vec![
                "--side".into(),
                "{SIDE}".into(),
                "--extract-libraries-to".into(),
                "{ROOT}/libraries/".into(),
                "--version".into(),
                "{MINECRAFT_VERSION}".into(),
                "--installer".into(),
                "{INSTALLER}".into(),
                "--libraries".into(),
                "{LIBRARY_DIR}".into(),
                "--mappings".into(),
                "[de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip]".into(),
                "--version-literal".into(),
                "{MCP_VERSION}".into(),
            ],
            outputs: vec![],
            sides: None,
        }];
        let data = BTreeMap::from([("MCP_VERSION".to_owned(), "'26.2-2'".to_owned())]);
        let planned = plan_processors(&processors, &data, &env()).unwrap();
        assert_eq!(
            planned[0].args,
            [
                "--side",
                "client",
                "--extract-libraries-to",
                "/data/libraries/",
                "--version",
                "26.2",
                "--installer",
                "/data/cache/forge-installer.jar",
                "--libraries",
                "/data/libraries",
                "--mappings",
                "/data/libraries/de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip",
                "--version-literal",
                "26.2-2",
            ]
        );
        assert!(
            planned[0].outputs.is_empty(),
            "NeoForge declares no output hashes"
        );
    }

    #[test]
    fn an_undeclared_output_after_dash_dash_output_is_still_checked_for_presence() {
        // NeoForge 26.2.0.93 declares no outputs; its patched client is `--output`.
        let processors = vec![ProcessorSpec {
            jar: coordinate("net.neoforged.installertools:installertools:4.0.17:fatjar"),
            classpath: vec![],
            args: vec![
                "--task".into(),
                "PROCESS_MINECRAFT_JAR".into(),
                "--input".into(),
                "{MINECRAFT_JAR}".into(),
                "--output".into(),
                "{PATCHED}".into(),
            ],
            outputs: vec![],
            sides: None,
        }];
        let data = BTreeMap::from([(
            "PATCHED".to_owned(),
            "[net.neoforged:minecraft-client-patched:26.2.0.93]".to_owned(),
        )]);
        let planned = plan_processors(&processors, &data, &env()).unwrap();
        assert_eq!(
            planned[0].outputs,
            [PlannedOutput {
                path: PathBuf::from(
                    "/data/libraries/net/neoforged/minecraft-client-patched/26.2.0.93/minecraft-client-patched-26.2.0.93.jar"
                ),
                sha1: None,
            }]
        );
    }

    #[test]
    fn a_data_root_that_itself_holds_dot_dot_is_fine() {
        let mut env = env();
        env.root = PathBuf::from("/work/crates/core/../../data");
        env.libraries_dir = env.root.join("libraries");
        let (processors, data) = forge_profile();
        let planned = plan_processors(&processors, &data, &env).unwrap();
        assert_eq!(
            planned[0].outputs[0].path,
            env.libraries_dir
                .join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-client.jar")
        );

        // A `..` the profile adds after the root still escapes.
        let escaping = vec![ProcessorSpec {
            outputs: vec![(
                "{ROOT}/../outside.jar".into(),
                "'be81e6d24785e467d30f30bcf31c18733233d3fe'".into(),
            )],
            ..processors[1].clone()
        }];
        assert!(matches!(
            plan_processors(&escaping, &data, &env),
            Err(ProcessorPlanError::OutputOutsideRoot(_))
        ));
    }

    #[test]
    fn refuses_unknown_keys_missing_files_and_unsafe_outputs() {
        let one = |args: Vec<&str>, outputs: Vec<(&str, &str)>| {
            vec![ProcessorSpec {
                jar: coordinate("net.minecraftforge:binarypatcher:1.3.4"),
                classpath: vec![],
                args: args.into_iter().map(str::to_owned).collect(),
                outputs: outputs
                    .into_iter()
                    .map(|(a, b)| (a.to_owned(), b.to_owned()))
                    .collect(),
                sides: None,
            }]
        };
        let data = BTreeMap::from([
            ("MISSING_FILE".to_owned(), "/data/server.lzma".to_owned()),
            ("BAD_COORD".to_owned(), "[../evil:x:1]".to_owned()),
            ("OUTSIDE".to_owned(), "'/etc/passwd'".to_owned()),
            ("BAD_SHA".to_owned(), "'not-a-sha'".to_owned()),
            (
                "PATCHED".to_owned(),
                "[net.minecraftforge:forge:26.2-65.1.5:client]".to_owned(),
            ),
        ]);
        let env = env();
        assert_eq!(
            plan_processors(&one(vec!["{NOPE}"], vec![]), &data, &env),
            Err(ProcessorPlanError::UnknownKey("NOPE".into()))
        );
        assert_eq!(
            plan_processors(&one(vec!["{MISSING_FILE}"], vec![]), &data, &env),
            Err(ProcessorPlanError::MissingDataFile(
                "/data/server.lzma".into()
            ))
        );
        assert!(matches!(
            plan_processors(&one(vec!["{BAD_COORD}"], vec![]), &data, &env),
            Err(ProcessorPlanError::InvalidCoordinate(_))
        ));
        assert!(matches!(
            plan_processors(
                &one(
                    vec![],
                    vec![("{OUTSIDE}", "'0000000000000000000000000000000000000000'")]
                ),
                &data,
                &env
            ),
            Err(ProcessorPlanError::OutputOutsideRoot(_))
        ));
        assert!(matches!(
            plan_processors(&one(vec![], vec![("{PATCHED}", "{BAD_SHA}")]), &data, &env),
            Err(ProcessorPlanError::InvalidChecksum(_))
        ));
    }
}
