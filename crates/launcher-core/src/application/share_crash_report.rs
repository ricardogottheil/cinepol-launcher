//! Bundles what a crash left behind (system and instance details, the crash
//! report, the JVM crash log and the end of the game output), removes the
//! secrets, and publishes it so the player can hand the staff one link.

use thiserror::Error;

use crate::application::share_diagnostic_report::{
    MAX_REPORT_BYTES, ShareReportError, share_diagnostic_report,
};
use crate::domain::diagnostics::{ReportHost, SharedReport};
use crate::domain::instance::{Instance, JavaSelection};
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::{
    LauncherPaths, Platform, is_crash_report_name, is_jvm_crash_log_name, redact_crash_text,
};
use crate::domain::ports::{FileSystem, FileSystemError, TextCap};

/// Where a crash report says what failed: its beginning.
const CRASH_REPORT_CAP: TextCap = TextCap::Head(160 * 1024);
/// A JVM crash log starts with the failing frame and the command line.
const JVM_CRASH_LOG_CAP: TextCap = TextCap::Head(128 * 1024);
/// The game output ends with what happened last.
const GAME_OUTPUT_CAP: TextCap = TextCap::Tail(160 * 1024);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashShareRequest {
    pub instance: InstanceId,
    pub exit_code: Option<i32>,
    /// File name inside the game folder's `crash-reports/`.
    pub crash_report: Option<String>,
    /// File name of the `hs_err_pid*.log` inside the game folder.
    pub jvm_crash_log: Option<String>,
}

pub struct ShareCrashDeps<'a, I, F, H> {
    pub instances: &'a I,
    pub fs: &'a F,
    pub host: &'a H,
    pub paths: &'a LauncherPaths,
    pub platform: &'a Platform,
    pub launcher_version: &'a str,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShareCrashError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error("not a crash file name: {0}")]
    InvalidName(String),
    #[error(transparent)]
    FileSystem(#[from] FileSystemError),
    #[error(transparent)]
    Share(#[from] ShareReportError),
}

pub async fn share_crash_report<I, F, H>(
    request: &CrashShareRequest,
    deps: &ShareCrashDeps<'_, I, F, H>,
) -> Result<SharedReport, ShareCrashError>
where
    I: InstanceRepository,
    F: FileSystem,
    H: ReportHost,
{
    // The names come from the UI: only crash files of this instance are read.
    if let Some(name) = request
        .crash_report
        .as_deref()
        .filter(|n| !is_crash_report_name(n))
    {
        return Err(ShareCrashError::InvalidName(name.to_owned()));
    }
    if let Some(name) = request
        .jvm_crash_log
        .as_deref()
        .filter(|n| !is_jvm_crash_log_name(n))
    {
        return Err(ShareCrashError::InvalidName(name.to_owned()));
    }
    let instance = deps.instances.get(&request.instance).await?;
    let game_dir = deps.paths.game_dir(&instance.id);

    let mut sections = Vec::new();
    if let Some(name) = &request.crash_report {
        let path = game_dir
            .join(crate::application::launch_instance::CRASH_REPORTS_DIR)
            .join(name);
        let text = deps.fs.read_text(&path, CRASH_REPORT_CAP).await?;
        sections.push(Section::new(
            format!("Crash report: {name}"),
            text,
            Keep::Head,
        ));
    }
    if let Some(name) = &request.jvm_crash_log {
        let text = deps
            .fs
            .read_text(&game_dir.join(name), JVM_CRASH_LOG_CAP)
            .await?;
        sections.push(Section::new(
            format!("JVM crash log: {name}"),
            text,
            Keep::Head,
        ));
    }
    let output = deps.paths.game_output_log(&instance.id);
    let text = deps.fs.read_text(&output, GAME_OUTPUT_CAP).await?;
    let output_name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    sections.push(Section::new(
        format!("Game output: {output_name}"),
        text,
        Keep::Tail,
    ));

    let report = assemble(&header(&instance, request.exit_code, deps), sections);
    Ok(share_diagnostic_report(&report, deps.host).await?)
}

fn header<I, F, H>(
    instance: &Instance,
    exit_code: Option<i32>,
    deps: &ShareCrashDeps<'_, I, F, H>,
) -> String {
    let loader = match instance.loader.version() {
        Some(version) => format!("{} {version}", instance.loader.kind().id()),
        None => instance.loader.kind().id().to_owned(),
    };
    let java = match &instance.settings.java {
        JavaSelection::Managed => "managed by the launcher",
        JavaSelection::Custom(_) => "custom",
    };
    let memory = instance.settings.memory;
    let exit = exit_code.map_or_else(
        || "none (stopped by a signal)".to_owned(),
        |c| c.to_string(),
    );
    format!(
        "Cinepol Launcher crash report\n\
         Launcher version: {}\n\
         System: {} {}\n\
         Instance: {}\n\
         Minecraft: {}\n\
         Loader: {loader}\n\
         Java: {java}\n\
         Memory: {}-{} MB\n\
         Exit code: {exit}\n",
        deps.launcher_version,
        format!("{:?}", deps.platform.os).to_lowercase(),
        format!("{:?}", deps.platform.arch).to_lowercase(),
        instance.name,
        instance.game_version.as_str(),
        memory.min_mb(),
        memory.max_mb(),
    )
}

/// Which end of a section holds what matters when it must be shortened.
#[derive(Clone, Copy)]
enum Keep {
    /// A crash report or JVM crash log says what failed at the top.
    Head,
    /// The game output ends with what happened last.
    Tail,
}

struct Section {
    title: String,
    /// Already redacted; `None` when the file was not found.
    text: Option<String>,
    keep: Keep,
}

impl Section {
    /// Each file is redacted on its own, so nothing one file holds (an
    /// unfinished environment block) can affect the next one.
    fn new(title: String, text: Option<String>, keep: Keep) -> Self {
        Self {
            title,
            text: text.map(|text| redact_crash_text(&text)),
            keep,
        }
    }

    fn render(&self) -> String {
        let mut out = format!("\n=== {} ===\n", self.title);
        match &self.text {
            Some(text) => {
                out.push_str(text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str("(not found)\n"),
        }
        out
    }
}

const CUT_NOTE: &str = "(cut to fit the upload limit)\n";

/// The caps count bytes read, but text that is not UTF-8 grows when decoded.
/// What does not fit is cut from the earlier sections first, at whole lines,
/// so the end of the game output is the last thing to go.
fn assemble(header: &str, mut sections: Vec<Section>) -> String {
    // The header holds names the player chose: it is cleaned like the files.
    let header = redact_crash_text(header);
    let header = header.as_str();
    let size = |sections: &[Section]| {
        header.len() + sections.iter().map(|s| s.render().len()).sum::<usize>()
    };
    for index in 0..sections.len() {
        // Measured again each time: a cut note may be longer than what it replaced.
        let overflow = size(&sections).saturating_sub(MAX_REPORT_BYTES);
        if overflow == 0 {
            break;
        }
        let section = &mut sections[index];
        let Some(text) = section.text.take() else {
            continue;
        };
        let kept = shorten(&text, overflow + CUT_NOTE.len(), section.keep);
        section.text = Some(match section.keep {
            Keep::Head => format!("{kept}{CUT_NOTE}"),
            Keep::Tail => format!("{CUT_NOTE}{kept}"),
        });
    }
    let mut report = header.to_owned();
    for section in &sections {
        report.push_str(&section.render());
    }
    // Last resort: only a header larger than the limit gets here.
    if report.len() > MAX_REPORT_BYTES {
        let mut end = MAX_REPORT_BYTES - CUT_NOTE.len();
        while !report.is_char_boundary(end) {
            end -= 1;
        }
        report.truncate(end);
        report.push_str(CUT_NOTE);
    }
    report
}

/// Removes at least `need` bytes from the end (`Keep::Head`) or the start
/// (`Keep::Tail`) of `text`, at a line boundary.
fn shorten(text: &str, need: usize, keep: Keep) -> String {
    if need >= text.len() {
        return String::new();
    }
    let kept = match keep {
        Keep::Head => {
            let mut end = text.len() - need;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let end = text[..end].rfind('\n').map_or(0, |at| at + 1);
            &text[..end]
        }
        Keep::Tail => {
            let mut start = need;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            let start = text[start..]
                .find('\n')
                .map_or(text.len(), |at| start + at + 1);
            &text[start..]
        }
    };
    kept.to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::diagnostics::ReportShareError;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::launch::{Arch, OsName};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::testing::{FakeFileSystem, FakeReportHost, InMemoryInstanceRepository};

    struct World {
        instances: InMemoryInstanceRepository,
        fs: FakeFileSystem,
        host: FakeReportHost,
        paths: LauncherPaths,
        platform: Platform,
    }

    impl World {
        async fn new() -> Self {
            let instances = InMemoryInstanceRepository::default();
            let loader = LoaderSpec::new(
                LoaderKind::Fabric,
                Some(LoaderVersion::parse("0.16.5").unwrap()),
            )
            .unwrap();
            instances
                .save(&Instance::new(
                    id(),
                    InstanceName::parse("CINEPOL").unwrap(),
                    GameVersion::parse("1.21.1").unwrap(),
                    loader,
                    InstanceOrigin::CinepolServer,
                ))
                .await
                .unwrap();
            Self {
                instances,
                fs: FakeFileSystem::with([]),
                host: FakeReportHost::answering(Ok(SharedReport {
                    url: "https://pastes.dev/aB3dE9fGh2".into(),
                    expires_at: None,
                })),
                paths: LauncherPaths::new("/data"),
                platform: Platform::new(OsName::Windows, Arch::X86_64),
            }
        }

        fn deps(
            &self,
        ) -> ShareCrashDeps<'_, InMemoryInstanceRepository, FakeFileSystem, FakeReportHost>
        {
            ShareCrashDeps {
                instances: &self.instances,
                fs: &self.fs,
                host: &self.host,
                paths: &self.paths,
                platform: &self.platform,
                launcher_version: "0.6.0",
            }
        }

        fn game_dir(&self) -> PathBuf {
            self.paths.game_dir(&id())
        }
    }

    fn id() -> InstanceId {
        InstanceId::parse("cinepol").unwrap()
    }

    fn request() -> CrashShareRequest {
        CrashShareRequest {
            instance: id(),
            exit_code: Some(-1_073_741_819),
            crash_report: None,
            jvm_crash_log: None,
        }
    }

    #[tokio::test]
    async fn bundles_every_piece_of_evidence_without_secrets() {
        let world = World::new().await;
        world.fs.add_text(
            world
                .game_dir()
                .join("crash-reports")
                .join("crash-1-client.txt"),
            "---- Minecraft Crash Report ----\nDescription: Rendering overlay\n",
        );
        world.fs.add_text(
            world.game_dir().join("hs_err_pid42.log"),
            "# EXCEPTION_ACCESS_VIOLATION\njava_command: Main --accessToken eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2ln\nEnvironment Variables:\nAPI_KEY=secret\n\nend\n",
        );
        world.fs.add_text(
            world.paths.game_output_log(&id()),
            "[Render thread/INFO]: Backend library: LWJGL\n",
        );
        let shared = share_crash_report(
            &CrashShareRequest {
                crash_report: Some("crash-1-client.txt".into()),
                jvm_crash_log: Some("hs_err_pid42.log".into()),
                ..request()
            },
            &world.deps(),
        )
        .await
        .unwrap();

        assert_eq!(shared.url, "https://pastes.dev/aB3dE9fGh2");
        let published = world.host.published();
        assert_eq!(published.len(), 1);
        let report = &published[0];
        for expected in [
            "Cinepol Launcher crash report",
            "Launcher version: 0.6.0",
            "System: windows x86_64",
            "Instance: CINEPOL",
            "Minecraft: 1.21.1",
            "Loader: fabric 0.16.5",
            "Exit code: -1073741819",
            "=== Crash report: crash-1-client.txt ===",
            "Description: Rendering overlay",
            "=== JVM crash log: hs_err_pid42.log ===",
            "# EXCEPTION_ACCESS_VIOLATION",
            "--accessToken <redacted>",
            "<environment variables omitted>",
            "=== Game output: game-cinepol.log ===",
            "Backend library: LWJGL",
        ] {
            assert!(
                report.contains(expected),
                "{expected:?} missing in:\n{report}"
            );
        }
        assert!(!report.contains("eyJ"), "{report}");
        assert!(!report.contains("API_KEY"), "{report}");
    }

    #[tokio::test]
    async fn a_crash_without_files_still_shares_its_details() {
        let world = World::new().await;
        share_crash_report(&request(), &world.deps()).await.unwrap();
        let report = &world.host.published()[0];
        assert!(report.contains("Exit code: -1073741819"), "{report}");
        assert!(
            report.contains("=== Game output: game-cinepol.log ===\n(not found)"),
            "{report}"
        );
        assert!(!report.contains("=== Crash report"), "{report}");
    }

    #[tokio::test]
    async fn refuses_names_that_are_not_crash_files_without_uploading() {
        let world = World::new().await;
        for (crash_report, jvm_crash_log) in [
            (Some("../../accounts.json"), None),
            (Some("notes.md"), None),
            (None, Some("options.txt")),
            (None, Some("..\\hs_err_pid1.log")),
        ] {
            let result = share_crash_report(
                &CrashShareRequest {
                    crash_report: crash_report.map(Into::into),
                    jvm_crash_log: jvm_crash_log.map(Into::into),
                    ..request()
                },
                &world.deps(),
            )
            .await;
            assert!(
                matches!(result, Err(ShareCrashError::InvalidName(_))),
                "{result:?}"
            );
        }
        assert!(world.host.published().is_empty());
    }

    #[tokio::test]
    async fn a_token_cut_by_the_size_limit_never_reaches_the_upload() {
        let world = World::new().await;
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJl";
        // The 160 KiB tail window starts three characters into the token.
        let rest = " rest\n";
        let filler_len = 160 * 1024 - (jwt.len() - 3) - rest.len();
        let filler = format!(
            "{}{}",
            "y\n".repeat(filler_len / 2),
            "\n".repeat(filler_len % 2)
        );
        world.fs.add_text(
            world.paths.game_output_log(&id()),
            &format!("{jwt}{rest}{filler}"),
        );
        share_crash_report(&request(), &world.deps()).await.unwrap();
        let report = &world.host.published()[0];
        assert!(
            !report.contains("c2lnbmF0dXJl"),
            "the token's signature leaked"
        );
    }

    /// Lines of a two-byte character: the caps of the fake count characters,
    /// so the decoded bundle is larger than the upload limit.
    fn wide_lines(chars: usize) -> String {
        "\u{e9}\n".repeat(chars / 2)
    }

    #[tokio::test]
    async fn an_oversized_bundle_trims_the_earlier_sections_and_keeps_the_output_end() {
        let world = World::new().await;
        world.fs.add_text(
            world.game_dir().join("crash-reports").join("c.txt"),
            &format!("Description: Rendering overlay\n{}", wide_lines(400_000)),
        );
        world.fs.add_text(
            world.game_dir().join("hs_err_pid1.log"),
            &format!("# EXCEPTION_ACCESS_VIOLATION\n{}", wide_lines(400_000)),
        );
        world.fs.add_text(
            world.paths.game_output_log(&id()),
            &format!("{}THE END\n", wide_lines(400_000)),
        );
        share_crash_report(
            &CrashShareRequest {
                crash_report: Some("c.txt".into()),
                jvm_crash_log: Some("hs_err_pid1.log".into()),
                ..request()
            },
            &world.deps(),
        )
        .await
        .unwrap();

        let report = &world.host.published()[0];
        assert!(report.len() <= MAX_REPORT_BYTES, "{}", report.len());
        assert!(report.starts_with("Cinepol Launcher crash report\n"));
        assert!(
            report.contains("Description: Rendering overlay"),
            "the head of the crash report stays"
        );
        assert!(report.ends_with("THE END\n"), "the end of the output stays");
        let output = &report[report.find("=== Game output").unwrap()..];
        // The whole 160 KiB character window of the output survives (240 KiB here).
        assert!(output.len() > 230_000, "{}", output.len());
        assert!(report.contains("(cut to fit the upload limit)"));
    }

    #[tokio::test]
    async fn an_empty_section_replaced_by_the_cut_note_still_fits() {
        let world = World::new().await;
        world
            .fs
            .add_text(world.game_dir().join("crash-reports").join("c.txt"), "");
        world.fs.add_text(
            world.game_dir().join("hs_err_pid1.log"),
            // Three bytes per character: the two sections alone pass the limit.
            &"\u{20ac}\n".repeat(200_000),
        );
        world.fs.add_text(
            world.paths.game_output_log(&id()),
            &format!("{}THE END\n", "\u{20ac}\n".repeat(200_000)),
        );
        share_crash_report(
            &CrashShareRequest {
                crash_report: Some("c.txt".into()),
                jvm_crash_log: Some("hs_err_pid1.log".into()),
                ..request()
            },
            &world.deps(),
        )
        .await
        .unwrap();
        let report = &world.host.published()[0];
        assert!(report.len() <= MAX_REPORT_BYTES, "{}", report.len());
        assert!(report.ends_with("THE END\n"));
    }

    #[tokio::test]
    async fn the_header_is_redacted_too() {
        let world = World::new().await;
        let jwt = "eyJhbGciOi.eyJzdWIiOi.c2lnbmF0dXJl";
        let mut instance = world.instances.get(&id()).await.unwrap();
        instance.name = InstanceName::parse(&format!("CINEPOL {jwt}")).unwrap();
        world.instances.save(&instance).await.unwrap();
        share_crash_report(&request(), &world.deps()).await.unwrap();
        let report = &world.host.published()[0];
        assert!(report.contains("Instance: CINEPOL <redacted>"), "{report}");
        assert!(!report.contains("c2lnbmF0dXJl"), "{report}");
    }

    #[test]
    fn a_header_larger_than_the_limit_is_cut_to_it() {
        let header = format!(
            "Cinepol Launcher crash report\n{}",
            "h".repeat(MAX_REPORT_BYTES)
        );
        let report = assemble(&header, Vec::new());
        assert!(report.len() <= MAX_REPORT_BYTES, "{}", report.len());
        assert!(report.starts_with("Cinepol Launcher crash report\n"));
        assert!(report.ends_with(CUT_NOTE));
    }

    #[tokio::test]
    async fn passes_upload_failures_on() {
        let mut world = World::new().await;
        world.host =
            FakeReportHost::answering(Err(ReportShareError::Unavailable("offline".into())));
        assert_eq!(
            share_crash_report(&request(), &world.deps()).await,
            Err(ShareCrashError::Share(ShareReportError::Host(
                ReportShareError::Unavailable("offline".into())
            )))
        );
    }
}
