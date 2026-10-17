//! Builds the JVM and game arguments of a launch.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use uuid::Uuid;

use super::{ArgumentToken, Features, OsName, Platform, VersionSpec, rules_allow};
use crate::domain::instance::{JvmArgs, MemorySettings, WindowSize};

/// Who the game runs as.
#[derive(Clone)]
pub struct LaunchIdentity {
    pub player_name: String,
    /// Profile UUID; passed without dashes like the official launcher.
    pub uuid: Uuid,
    /// Minecraft access token; any non-empty placeholder for offline play.
    pub access_token: String,
    /// `msa` for Microsoft accounts, `legacy` for offline profiles.
    pub user_type: &'static str,
    pub xuid: String,
}

impl fmt::Debug for LaunchIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchIdentity")
            .field("player_name", &self.player_name)
            .field("uuid", &self.uuid)
            .field("access_token", &"<redacted>")
            .field("user_type", &self.user_type)
            .finish_non_exhaustive()
    }
}

/// Everything a launch needs besides the version itself.
#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub platform: Platform,
    pub identity: LaunchIdentity,
    pub game_dir: PathBuf,
    pub assets_root: PathBuf,
    pub natives_dir: PathBuf,
    pub libraries_dir: PathBuf,
    /// Absolute classpath entries in order (libraries, then the client jar).
    pub classpath: Vec<PathBuf>,
    pub launcher_name: String,
    pub launcher_version: String,
    pub memory: MemorySettings,
    pub extra_jvm_args: JvmArgs,
    pub resolution: Option<WindowSize>,
    /// Server to join on start (Quick Play), `host[:port]`.
    pub quick_play_multiplayer: Option<String>,
    /// Absolute path of the log4j configuration file, when the version ships one.
    pub logging_config: Option<PathBuf>,
}

impl LaunchContext {
    /// Feature flags that switch on conditional arguments.
    pub fn features(&self) -> Features {
        let mut features = Features::new();
        if self.resolution.is_some() {
            features.insert("has_custom_resolution".to_owned());
        }
        if self.quick_play_multiplayer.is_some() {
            features.insert("is_quick_play_multiplayer".to_owned());
        }
        features
    }
}

/// The argument vector after `java`. Its `Debug` output hides the access token.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    secret: String,
}

impl LaunchCommand {
    /// A command with nothing to redact (tests and tools; launches use `build_launch_command`).
    pub fn new(
        jvm_args: Vec<String>,
        main_class: impl Into<String>,
        game_args: Vec<String>,
    ) -> Self {
        Self {
            jvm_args,
            main_class: main_class.into(),
            game_args,
            secret: String::new(),
        }
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = self.jvm_args.clone();
        args.push(self.main_class.clone());
        args.extend(self.game_args.iter().cloned());
        args
    }

    /// The arguments with the access token replaced, safe to log.
    pub fn redacted_args(&self) -> Vec<String> {
        self.args()
            .into_iter()
            .map(|arg| {
                if self.secret.is_empty() {
                    arg
                } else {
                    arg.replace(&self.secret, "<redacted>")
                }
            })
            .collect()
    }
}

impl fmt::Debug for LaunchCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LaunchCommand")
            .field(&self.redacted_args())
            .finish()
    }
}

/// JVM arguments the official launcher adds for versions without `arguments.jvm`.
fn legacy_jvm_defaults(platform: &Platform) -> Vec<String> {
    let mut args = Vec::new();
    if platform.os == OsName::MacOs {
        args.push("-XstartOnFirstThread".to_owned());
    }
    if platform.os == OsName::Windows {
        args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".to_owned());
    }
    if platform.mojang_arch() == "x86" {
        args.push("-Xss1M".to_owned());
    }
    args.extend(
        [
            "-Djava.library.path=${natives_directory}",
            "-Dminecraft.launcher.brand=${launcher_name}",
            "-Dminecraft.launcher.version=${launcher_version}",
            "-cp",
            "${classpath}",
        ]
        .map(str::to_owned),
    );
    args
}

fn expand(tokens: &[ArgumentToken], platform: &Platform, features: &Features) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|token| match token {
            ArgumentToken::Plain(value) => vec![value.clone()],
            ArgumentToken::Conditional { rules, values } => {
                if rules_allow(rules, platform, features) {
                    values.clone()
                } else {
                    vec![]
                }
            }
        })
        .collect()
}

/// Replaces every `${name}` with its value; unknown placeholders stay as they are.
pub fn substitute(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Builds `-Xms/-Xmx`, the player's JVM arguments, the version's JVM arguments
/// (or the legacy defaults), the logging argument, the main class and the game arguments.
pub fn build_launch_command(spec: &VersionSpec, ctx: &LaunchContext) -> LaunchCommand {
    let features = ctx.features();
    let classpath = ctx
        .classpath
        .iter()
        .map(|entry| path_string(entry))
        .collect::<Vec<_>>()
        .join(ctx.platform.classpath_separator());
    let (width, height) = ctx
        .resolution
        .map(|r| (r.width().to_string(), r.height().to_string()))
        .unwrap_or_default();

    let vars: BTreeMap<&str, String> = BTreeMap::from([
        ("auth_player_name", ctx.identity.player_name.clone()),
        ("auth_uuid", ctx.identity.uuid.simple().to_string()),
        ("auth_access_token", ctx.identity.access_token.clone()),
        ("auth_session", ctx.identity.access_token.clone()),
        ("auth_xuid", ctx.identity.xuid.clone()),
        ("user_type", ctx.identity.user_type.to_owned()),
        ("user_properties", "{}".to_owned()),
        ("clientid", String::new()),
        ("version_name", spec.id.clone()),
        ("version_type", spec.kind.id().to_owned()),
        ("game_directory", path_string(&ctx.game_dir)),
        ("assets_root", path_string(&ctx.assets_root)),
        ("game_assets", path_string(&ctx.assets_root)),
        (
            "assets_index_name",
            spec.asset_index
                .as_ref()
                .map(|index| index.id.clone())
                .unwrap_or_default(),
        ),
        ("natives_directory", path_string(&ctx.natives_dir)),
        ("library_directory", path_string(&ctx.libraries_dir)),
        (
            "classpath_separator",
            ctx.platform.classpath_separator().to_owned(),
        ),
        ("classpath", classpath),
        ("launcher_name", ctx.launcher_name.clone()),
        ("launcher_version", ctx.launcher_version.clone()),
        ("resolution_width", width),
        ("resolution_height", height),
        (
            "quickPlayMultiplayer",
            ctx.quick_play_multiplayer.clone().unwrap_or_default(),
        ),
    ]);

    let mut jvm_args = vec![
        format!("-Xms{}M", ctx.memory.min_mb()),
        format!("-Xmx{}M", ctx.memory.max_mb()),
    ];
    jvm_args.extend(ctx.extra_jvm_args.as_slice().iter().cloned());
    let version_jvm = if spec.jvm_arguments.is_empty() {
        legacy_jvm_defaults(&ctx.platform)
    } else {
        expand(&spec.jvm_arguments, &ctx.platform, &features)
    };
    jvm_args.extend(version_jvm.iter().map(|arg| substitute(arg, &vars)));
    if let (Some(logging), Some(path)) = (&spec.logging, &ctx.logging_config) {
        let path_vars = BTreeMap::from([("path", path_string(path))]);
        jvm_args.push(substitute(&logging.argument, &path_vars));
    }

    let mut game_tokens = match &spec.legacy_game_arguments {
        Some(legacy) => legacy.clone(),
        None => expand(&spec.game_arguments, &ctx.platform, &features),
    };
    if spec.legacy_game_arguments.is_some() {
        // Old versions have no conditional arguments, so add the window size by hand.
        if ctx.resolution.is_some() {
            game_tokens.extend(
                [
                    "--width",
                    "${resolution_width}",
                    "--height",
                    "${resolution_height}",
                ]
                .map(str::to_owned),
            );
        }
        if ctx.quick_play_multiplayer.is_some() {
            game_tokens.extend(legacy_server_args(ctx.quick_play_multiplayer.as_deref()));
        }
    }
    let game_args = game_tokens
        .iter()
        .map(|arg| substitute(arg, &vars))
        .collect();

    LaunchCommand {
        jvm_args,
        main_class: spec.main_class.clone(),
        game_args,
        secret: ctx.identity.access_token.clone(),
    }
}

/// `--server host --port port` for versions without Quick Play.
fn legacy_server_args(address: Option<&str>) -> Vec<String> {
    let Some(address) = address else {
        return vec![];
    };
    let (host, port) = match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') || host.starts_with('[') => {
            (host.trim_matches(['[', ']']), port)
        }
        _ => (address, "25565"),
    };
    vec![
        "--server".into(),
        host.to_owned(),
        "--port".into(),
        port.to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::launch::{
        Arch, Artifact, AssetIndexRef, OsCondition, RelativePath, Rule, RuleAction, VersionKind,
    };

    fn spec_modern() -> VersionSpec {
        let os_rule = |name: &str| Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some(name.into()),
                ..Default::default()
            }),
            features: BTreeMap::new(),
        };
        let feature_rule = |name: &str| Rule {
            action: RuleAction::Allow,
            os: None,
            features: BTreeMap::from([(name.to_owned(), true)]),
        };
        VersionSpec {
            id: "26.2".into(),
            kind: VersionKind::Release,
            main_class: "net.minecraft.client.main.Main".into(),
            game_arguments: vec![
                ArgumentToken::Plain("--username".into()),
                ArgumentToken::Plain("${auth_player_name}".into()),
                ArgumentToken::Plain("--uuid".into()),
                ArgumentToken::Plain("${auth_uuid}".into()),
                ArgumentToken::Plain("--accessToken".into()),
                ArgumentToken::Plain("${auth_access_token}".into()),
                ArgumentToken::Plain("--assetIndex".into()),
                ArgumentToken::Plain("${assets_index_name}".into()),
                ArgumentToken::Conditional {
                    rules: vec![feature_rule("has_custom_resolution")],
                    values: vec![
                        "--width".into(),
                        "${resolution_width}".into(),
                        "--height".into(),
                        "${resolution_height}".into(),
                    ],
                },
                ArgumentToken::Conditional {
                    rules: vec![feature_rule("is_quick_play_multiplayer")],
                    values: vec![
                        "--quickPlayMultiplayer".into(),
                        "${quickPlayMultiplayer}".into(),
                    ],
                },
            ],
            jvm_arguments: vec![
                ArgumentToken::Conditional {
                    rules: vec![os_rule("osx")],
                    values: vec!["-XstartOnFirstThread".into()],
                },
                ArgumentToken::Plain("-Djava.library.path=${natives_directory}/java".into()),
                ArgumentToken::Plain("-cp".into()),
                ArgumentToken::Plain("${classpath}".into()),
            ],
            legacy_game_arguments: None,
            libraries: vec![],
            client_jar: None,
            asset_index: Some(AssetIndexRef {
                id: "32".into(),
                artifact: Artifact {
                    path: RelativePath::parse("indexes/32.json").unwrap(),
                    url: String::new(),
                    checksum: None,
                    size: None,
                },
            }),
            java: None,
            logging: None,
            inherits_from: None,
        }
    }

    fn context(platform: Platform) -> LaunchContext {
        LaunchContext {
            platform,
            identity: LaunchIdentity {
                player_name: "polispol1".into(),
                uuid: Uuid::parse_str("613bd4fe-6f7f-3488-8542-a216abf295e0").unwrap(),
                access_token: "secret-token".into(),
                user_type: "legacy",
                xuid: "0".into(),
            },
            game_dir: PathBuf::from("/data/instances/cinepol/minecraft"),
            assets_root: PathBuf::from("/data/assets"),
            natives_dir: PathBuf::from("/data/instances/cinepol/natives"),
            libraries_dir: PathBuf::from("/data/libraries"),
            classpath: vec![
                PathBuf::from("/data/libraries/a.jar"),
                PathBuf::from("/data/versions/26.2/26.2.jar"),
            ],
            launcher_name: "cinepol-launcher".into(),
            launcher_version: "0.1.0".into(),
            memory: MemorySettings::new(1024, 4096).unwrap(),
            extra_jvm_args: JvmArgs::parse(vec!["-XX:+UseG1GC".into()]).unwrap(),
            resolution: None,
            quick_play_multiplayer: None,
            logging_config: None,
        }
    }

    #[test]
    fn substitutes_known_placeholders_and_keeps_unknown_ones() {
        let vars = BTreeMap::from([("a", "1".to_owned())]);
        assert_eq!(substitute("x${a}y${b}z${a", &vars), "x1y${b}z${a");
    }

    #[test]
    fn builds_a_modern_launch_in_the_official_order() {
        let command = build_launch_command(
            &spec_modern(),
            &context(Platform::new(OsName::Linux, Arch::X86_64)),
        );
        assert_eq!(
            command.jvm_args,
            [
                "-Xms1024M",
                "-Xmx4096M",
                "-XX:+UseG1GC",
                "-Djava.library.path=/data/instances/cinepol/natives/java",
                "-cp",
                "/data/libraries/a.jar:/data/versions/26.2/26.2.jar",
            ]
        );
        assert_eq!(command.main_class, "net.minecraft.client.main.Main");
        assert_eq!(
            command.game_args,
            [
                "--username",
                "polispol1",
                "--uuid",
                "613bd4fe6f7f34888542a216abf295e0",
                "--accessToken",
                "secret-token",
                "--assetIndex",
                "32"
            ]
        );
    }

    #[test]
    fn applies_os_rules_and_the_windows_classpath_separator() {
        let mac = build_launch_command(
            &spec_modern(),
            &context(Platform::new(OsName::MacOs, Arch::Arm64)),
        );
        assert!(mac.jvm_args.contains(&"-XstartOnFirstThread".to_owned()));
        let windows = build_launch_command(
            &spec_modern(),
            &context(Platform::new(OsName::Windows, Arch::X86_64)),
        );
        assert!(
            !windows
                .jvm_args
                .contains(&"-XstartOnFirstThread".to_owned())
        );
        assert!(
            windows
                .jvm_args
                .iter()
                .any(|arg| arg.contains(".jar;/data/versions"))
        );
    }

    #[test]
    fn enables_resolution_and_quick_play_arguments_through_features() {
        let mut ctx = context(Platform::new(OsName::Linux, Arch::X86_64));
        ctx.resolution = Some(WindowSize::new(1280, 720).unwrap());
        ctx.quick_play_multiplayer = Some("mc.policarpiers.com".into());
        let args = build_launch_command(&spec_modern(), &ctx).game_args;
        let tail: Vec<&str> = args.iter().skip(8).map(String::as_str).collect();
        assert_eq!(
            tail,
            [
                "--width",
                "1280",
                "--height",
                "720",
                "--quickPlayMultiplayer",
                "mc.policarpiers.com"
            ]
        );
    }

    #[test]
    fn legacy_versions_get_default_jvm_args_and_server_flags() {
        let mut spec = spec_modern();
        spec.jvm_arguments.clear();
        spec.legacy_game_arguments = Some(vec![
            "--username".into(),
            "${auth_player_name}".into(),
            "--userType".into(),
            "${user_type}".into(),
        ]);
        let mut ctx = context(Platform::new(OsName::MacOs, Arch::X86_64));
        ctx.quick_play_multiplayer = Some("mc.policarpiers.com:25570".into());
        let command = build_launch_command(&spec, &ctx);
        assert!(
            command
                .jvm_args
                .contains(&"-XstartOnFirstThread".to_owned())
        );
        assert!(
            command
                .jvm_args
                .contains(&"-Djava.library.path=/data/instances/cinepol/natives".to_owned())
        );
        assert_eq!(
            command.game_args,
            [
                "--username",
                "polispol1",
                "--userType",
                "legacy",
                "--server",
                "mc.policarpiers.com",
                "--port",
                "25570"
            ]
        );
    }

    #[test]
    fn debug_output_and_redacted_args_hide_the_access_token() {
        let command = build_launch_command(
            &spec_modern(),
            &context(Platform::new(OsName::Linux, Arch::X86_64)),
        );
        assert!(command.args().contains(&"secret-token".to_owned()));
        assert!(!format!("{command:?}").contains("secret-token"));
        assert!(
            !command
                .redacted_args()
                .iter()
                .any(|arg| arg.contains("secret-token"))
        );
        assert!(!format!("{:?}", context(Platform::current()).identity).contains("secret-token"));
    }

    #[test]
    fn adds_the_logging_argument_when_configured() {
        let mut spec = spec_modern();
        spec.logging = Some(crate::domain::launch::LoggingConfig {
            argument: "-Dlog4j.configurationFile=${path}".into(),
            file: Artifact {
                path: RelativePath::parse("log_configs/client-1.21.2.xml").unwrap(),
                url: String::new(),
                checksum: None,
                size: None,
            },
        });
        let mut ctx = context(Platform::new(OsName::Linux, Arch::X86_64));
        ctx.logging_config = Some(PathBuf::from("/data/assets/log_configs/client-1.21.2.xml"));
        let command = build_launch_command(&spec, &ctx);
        assert_eq!(
            command.jvm_args.last().map(String::as_str),
            Some("-Dlog4j.configurationFile=/data/assets/log_configs/client-1.21.2.xml")
        );
    }
}
