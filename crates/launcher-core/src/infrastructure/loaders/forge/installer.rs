//! Installing Forge and NeoForge from their installer jar.
//!
//! `install`:
//! 1. fetches the installer's `.sha1` sidecar, reuses the stored installer
//!    when it still matches, and otherwise downloads and verifies it; the
//!    installer and its sha1 are kept under `libraries/` for `finish`;
//! 2. extracts it (zip-slip safe) into a scratch directory under `temp/`;
//! 3. reads `install_profile.json` (spec 1 only) and `version.json`, checks
//!    both name this profile and its game version, and stores `version.json`
//!    atomically at `versions/<id>/<id>.json`;
//! 4. copies the libraries the installer ships in `maven/` (those without a
//!    URL), verifying their sha1, and returns the profile libraries as tools.
//!
//! Offline, a stored installer that still matches its stored sha1 is used.
//!
//! `finish` re-verifies the stored installer, extracts it again, plans the
//! client processors and runs the ones whose work is not done yet: a
//! processor is skipped when its declared outputs all exist and match, and
//! processors without declared outputs are skipped when the marker
//! `versions/<id>/.processors-done` holds the fingerprint of the same plan.
//! Once one processor runs, every later one runs too. Each run is followed by
//! the verification of every output with a declared sha1.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use super::Flavor;
use super::manifest::read_main_class;
use super::profile::{InstallProfile, local_libraries, parse_install_profile};
use crate::domain::instance::GameVersion;
use crate::domain::launch::{
    Artifact, ExtractError, LauncherPaths, MavenCoordinate, RelativePath, VersionId,
};
use crate::domain::loader::{
    LoaderEnvironment, LoaderInstallError, LoaderKind, LoaderProfile, LoaderSpec, PlannedProcessor,
    ProcessorCommand, ProcessorEnvironment, ProcessorRunError, ProcessorRunner, plan_processors,
    profile_id,
};
use crate::domain::ports::{HttpClient, HttpError, ProgressReporter, ProgressUpdate};
use crate::infrastructure::archive::extract_zip_async;
use crate::infrastructure::fs::write_atomic;
use crate::infrastructure::loaders::meta::{check_header, every_library_hashed, parse_sidecar};

/// Written next to the version JSON once every processor succeeded.
const MARKER: &str = ".processors-done";
const INSTALL_PROFILE: &str = "install_profile.json";

/// Where one loader version's files live.
#[derive(Debug)]
struct Target {
    kind: LoaderKind,
    id: VersionId,
    loader: String,
    /// The installer jar, kept for `finish`.
    installer: PathBuf,
    /// The verified sha1 of the installer.
    installer_sha1: PathBuf,
    installer_url: String,
    version_json: PathBuf,
    marker: PathBuf,
    /// Scratch space for the extracted installer.
    work_dir: PathBuf,
}

impl Target {
    fn new(
        paths: &LauncherPaths,
        flavor: &Flavor,
        loader: &LoaderSpec,
        game: &GameVersion,
    ) -> Result<Self, LoaderInstallError> {
        let kind = flavor.kind;
        let version = loader
            .version()
            .ok_or_else(|| invalid(format!("{kind:?} needs a loader version")))?;
        let unsafe_id = || {
            invalid(format!(
                "{kind:?} {version} for Minecraft {} has no safe profile id",
                game.as_str()
            ))
        };
        let id = profile_id(loader, game).ok_or_else(unsafe_id)?;
        let version_json = paths.version_json(id.as_str()).ok_or_else(unsafe_id)?;
        let maven_version = flavor.maven_version(game, version);
        let coordinate = MavenCoordinate::parse(&format!(
            "{}:{}:{maven_version}:installer",
            flavor.group, flavor.artifact
        ))
        .map_err(|error| invalid(format!("{kind:?} installer: {error}")))?;
        let relative = coordinate.path();
        let installer = relative.under(&paths.libraries_dir());
        let file_name = relative.as_str().rsplit('/').next().unwrap_or_default();
        Ok(Self {
            kind,
            loader: version.as_str().to_owned(),
            installer_sha1: installer.with_file_name(format!("{file_name}.sha1")),
            installer,
            installer_url: format!("{}/{relative}", flavor.repository),
            marker: version_json.with_file_name(MARKER),
            version_json,
            work_dir: paths.temp_dir().join("loaders").join(id.as_str()),
            id,
        })
    }

    fn unknown(&self, game: &GameVersion) -> LoaderInstallError {
        LoaderInstallError::UnknownLoaderVersion(
            self.kind,
            self.loader.clone(),
            game.as_str().to_owned(),
        )
    }
}

fn invalid(what: impl std::fmt::Display) -> LoaderInstallError {
    LoaderInstallError::Invalid(what.to_string())
}

fn storage(path: &Path, error: impl std::fmt::Display) -> LoaderInstallError {
    LoaderInstallError::Storage(format!("{}: {error}", path.display()))
}

pub(in crate::infrastructure::loaders) async fn install<H: HttpClient>(
    http: &H,
    paths: &LauncherPaths,
    flavor: &Flavor,
    loader: &LoaderSpec,
    game: &GameVersion,
) -> Result<LoaderProfile, LoaderInstallError> {
    let target = Target::new(paths, flavor, loader, game)?;
    obtain_installer(http, &target, game).await?;
    let result = install_from_installer(paths, &target, game).await;
    clean_up(&target).await;
    result
}

#[allow(clippy::too_many_arguments)]
pub(in crate::infrastructure::loaders) async fn finish<P: ProcessorRunner, R: ProgressReporter>(
    paths: &LauncherPaths,
    runner: &P,
    flavor: &Flavor,
    loader: &LoaderSpec,
    game: &GameVersion,
    environment: LoaderEnvironment<'_>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<(), LoaderInstallError> {
    let target = Target::new(paths, flavor, loader, game)?;
    if cancel.is_cancelled() {
        return Err(LoaderInstallError::Cancelled);
    }
    if !stored_installer_is_valid(&target).await {
        return Err(invalid(format!(
            "the {:?} installer of {} is missing or damaged; install the loader again",
            target.kind, target.id
        )));
    }
    let result = run_processors(paths, runner, &target, game, environment, reporter, cancel).await;
    clean_up(&target).await;
    result
}

// ------------------------------------------------------------ the installer

/// Answers that mean the version does not exist on the maven.
fn is_missing(error: &HttpError) -> bool {
    matches!(
        error,
        HttpError::NotFound(_)
            | HttpError::Status {
                status: 400..=499,
                ..
            }
    )
}

/// Makes sure the verified installer is stored at `target.installer`.
async fn obtain_installer<H: HttpClient>(
    http: &H,
    target: &Target,
    game: &GameVersion,
) -> Result<(), LoaderInstallError> {
    let sidecar_url = format!("{}.sha1", target.installer_url);
    let expected = match http.get_bytes(&sidecar_url).await {
        Ok(body) => {
            parse_sidecar(&body).ok_or_else(|| invalid(format!("{sidecar_url} is not a sha1")))?
        }
        Err(error) if is_missing(&error) => return Err(target.unknown(game)),
        Err(error) => {
            if stored_installer_is_valid(target).await {
                tracing::warn!(id = %target.id, %error, "loader maven unreachable; using the stored installer");
                return Ok(());
            }
            return Err(LoaderInstallError::Network(error.to_string()));
        }
    };
    if file_sha1(&target.installer)
        .await
        .is_ok_and(|actual| actual == expected)
    {
        if stored_sha1(target).await.as_deref() != Some(expected.as_str()) {
            write_sha1(target, &expected).await?;
        }
        return Ok(());
    }
    let bytes = http
        .get_bytes(&target.installer_url)
        .await
        .map_err(|error| {
            if is_missing(&error) {
                target.unknown(game)
            } else {
                LoaderInstallError::Network(error.to_string())
            }
        })?;
    let actual = hex::encode(Sha1::digest(&bytes));
    if actual != expected {
        return Err(invalid(format!(
            "{} does not match its sha1 (expected {expected}, got {actual})",
            target.installer_url
        )));
    }
    write_atomic(&target.installer, &bytes)
        .await
        .map_err(|error| storage(&target.installer, error))?;
    write_sha1(target, &expected).await
}

async fn write_sha1(target: &Target, sha1: &str) -> Result<(), LoaderInstallError> {
    write_atomic(&target.installer_sha1, sha1.as_bytes())
        .await
        .map_err(|error| storage(&target.installer_sha1, error))
}

async fn stored_sha1(target: &Target) -> Option<String> {
    let body = tokio::fs::read(&target.installer_sha1).await.ok()?;
    parse_sidecar(&body)
}

/// The stored installer still matches the sha1 it was verified against.
async fn stored_installer_is_valid(target: &Target) -> bool {
    let Some(expected) = stored_sha1(target).await else {
        return false;
    };
    file_sha1(&target.installer)
        .await
        .is_ok_and(|actual| actual == expected)
}

/// Extracts the stored installer into a fresh scratch directory.
async fn extract_installer(target: &Target) -> Result<PathBuf, LoaderInstallError> {
    remove_dir(&target.work_dir).await?;
    let dir = target.work_dir.join("installer");
    extract_zip_async(target.installer.clone(), dir.clone(), Vec::new())
        .await
        .map_err(|error| match error {
            ExtractError::UnsafeEntry(name) => {
                invalid(format!("the installer holds an unsafe entry {name:?}"))
            }
            ExtractError::Corrupt { reason, .. } => {
                invalid(format!("the installer is not a valid jar: {reason}"))
            }
            ExtractError::Io(reason) => LoaderInstallError::Storage(reason),
        })?;
    Ok(dir)
}

async fn remove_dir(path: &Path) -> Result<(), LoaderInstallError> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage(path, error)),
    }
}

async fn clean_up(target: &Target) {
    if let Err(error) = remove_dir(&target.work_dir).await {
        tracing::warn!(%error, "could not remove the extracted installer");
    }
}

async fn read_entry(dir: &Path, entry: &RelativePath) -> Result<Vec<u8>, LoaderInstallError> {
    tokio::fs::read(entry.under(dir))
        .await
        .map_err(|error| invalid(format!("the installer has no readable {entry}: {error}")))
}

/// The install profile, checked against the version being installed.
async fn read_profile(
    target: &Target,
    dir: &Path,
    game: &GameVersion,
) -> Result<InstallProfile, LoaderInstallError> {
    let entry = RelativePath::parse(INSTALL_PROFILE).map_err(invalid)?;
    let profile = parse_install_profile(target.kind, &read_entry(dir, &entry).await?)?;
    if profile.version != target.id.as_str() {
        return Err(invalid(format!(
            "the installer installs {:?}, not {}",
            profile.version, target.id
        )));
    }
    if profile.minecraft != game.as_str() {
        return Err(invalid(format!(
            "the installer is for Minecraft {:?}, not {}",
            profile.minecraft,
            game.as_str()
        )));
    }
    Ok(profile)
}

async fn install_from_installer(
    paths: &LauncherPaths,
    target: &Target,
    game: &GameVersion,
) -> Result<LoaderProfile, LoaderInstallError> {
    let dir = extract_installer(target).await?;
    let profile = read_profile(target, &dir, game).await?;
    let version_bytes = read_entry(&dir, &profile.json).await?;
    let version: Value = serde_json::from_slice(&version_bytes)
        .map_err(|error| invalid(format!("version.json: {error}")))?;
    check_header(&version, &target.id, game)?;
    every_library_hashed(&version)?;

    let libraries_dir = paths.libraries_dir();
    for tool in profile.libraries.iter().filter(|tool| tool.url.is_empty()) {
        if !copy_local_library(&dir, &libraries_dir, tool).await? {
            return Err(invalid(format!(
                "tool {} has no URL and is not in the installer",
                tool.path
            )));
        }
    }
    // The ones the installer does not ship are processor outputs.
    for library in local_libraries(&version)? {
        copy_local_library(&dir, &libraries_dir, &library).await?;
    }

    write_atomic(&target.version_json, &version_bytes)
        .await
        .map_err(|error| storage(&target.version_json, error))?;
    Ok(LoaderProfile {
        id: target.id.clone(),
        tools: profile.libraries,
    })
}

/// Copies `maven/<path>` from the extracted installer into the libraries,
/// after checking its sha1 when one is declared. `false` when the installer
/// does not ship it.
async fn copy_local_library(
    dir: &Path,
    libraries_dir: &Path,
    artifact: &Artifact,
) -> Result<bool, LoaderInstallError> {
    let source = artifact.path.under(&dir.join("maven"));
    let bytes = match tokio::fs::read(&source).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(storage(&source, error)),
    };
    if let Some(checksum) = &artifact.checksum
        && !checksum.matches(&hex::encode(Sha1::digest(&bytes)))
    {
        return Err(invalid(format!(
            "{} in the installer does not match its sha1",
            artifact.path
        )));
    }
    let destination = artifact.path.under(libraries_dir);
    write_atomic(&destination, &bytes)
        .await
        .map_err(|error| storage(&destination, error))?;
    Ok(true)
}

// ------------------------------------------------------------ processors

async fn run_processors<P: ProcessorRunner, R: ProgressReporter>(
    paths: &LauncherPaths,
    runner: &P,
    target: &Target,
    game: &GameVersion,
    environment: LoaderEnvironment<'_>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<(), LoaderInstallError> {
    let dir = extract_installer(target).await?;
    let profile = read_profile(target, &dir, game).await?;
    // `data` entries such as `/data/client.lzma` name files in the installer.
    let data_files = profile
        .data
        .values()
        .filter_map(|value| {
            let relative = RelativePath::parse(value.strip_prefix('/')?).ok()?;
            let path = relative.under(&dir);
            path.is_file().then(|| (value.clone(), path))
        })
        .collect();
    let processor_environment = ProcessorEnvironment {
        minecraft_version: game.as_str().to_owned(),
        minecraft_jar: environment.client_jar.to_path_buf(),
        root: paths.root().to_path_buf(),
        libraries_dir: paths.libraries_dir(),
        installer: target.installer.clone(),
        data_files,
    };
    let planned = plan_processors(&profile.processors, &profile.data, &processor_environment)
        .map_err(invalid)?;

    let fingerprint = fingerprint(&planned);
    let marker_matches = tokio::fs::read_to_string(&target.marker)
        .await
        .is_ok_and(|stored| stored.trim() == fingerprint);
    let total = planned.len() as u64;
    reporter.report(ProgressUpdate::new(0, Some(total)));
    let mut ran = false;
    for (index, processor) in planned.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(LoaderInstallError::Cancelled);
        }
        let done = !ran
            && if processor.outputs.is_empty() {
                marker_matches
            } else {
                outputs_verified(processor).await
            };
        if !done {
            if !ran {
                remove_file(&target.marker).await?;
                ran = true;
            }
            run(runner, environment.java, paths.root(), processor, cancel).await?;
            verify_outputs(processor).await?;
        }
        reporter.report(ProgressUpdate::new(index as u64 + 1, Some(total)));
    }
    if ran || !marker_matches {
        write_atomic(&target.marker, fingerprint.as_bytes())
            .await
            .map_err(|error| storage(&target.marker, error))?;
    }
    Ok(())
}

async fn run<P: ProcessorRunner>(
    runner: &P,
    java: &Path,
    root: &Path,
    processor: &PlannedProcessor,
    cancel: &CancellationToken,
) -> Result<(), LoaderInstallError> {
    let main_class = read_main_class(&processor.jar).await?;
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|error| storage(root, error))?;
    let tool = processor
        .jar
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    tracing::info!(%tool, %main_class, "running an installer processor");
    let command = ProcessorCommand {
        java: java.to_path_buf(),
        classpath: processor.classpath.clone(),
        main_class,
        args: processor.args.clone(),
        working_dir: root.to_path_buf(),
    };
    runner
        .run(&command, cancel)
        .await
        .map_err(|error| match error {
            ProcessorRunError::Cancelled => LoaderInstallError::Cancelled,
            other => LoaderInstallError::ProcessorFailed(format!("{tool}: {other}")),
        })
}

/// Every declared output exists and matches its declared sha1.
async fn outputs_verified(processor: &PlannedProcessor) -> bool {
    for output in &processor.outputs {
        let verified = match &output.sha1 {
            Some(expected) => file_sha1(&output.path)
                .await
                .is_ok_and(|actual| expected.matches(&actual)),
            None => tokio::fs::metadata(&output.path)
                .await
                .is_ok_and(|metadata| metadata.is_file()),
        };
        if !verified {
            return false;
        }
    }
    true
}

/// A missing output is a failed processor; a wrong one is removed.
async fn verify_outputs(processor: &PlannedProcessor) -> Result<(), LoaderInstallError> {
    for output in &processor.outputs {
        let path = &output.path;
        let actual = match file_sha1(path).await {
            Ok(actual) => actual,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(LoaderInstallError::ProcessorFailed(format!(
                    "{} was not produced",
                    path.display()
                )));
            }
            Err(error) => return Err(storage(path, error)),
        };
        if let Some(expected) = &output.sha1
            && !expected.matches(&actual)
        {
            if let Err(error) = tokio::fs::remove_file(path).await {
                tracing::warn!(path = %path.display(), %error, "could not remove a bad processor output");
            }
            return Err(LoaderInstallError::OutputMismatch(format!(
                "{} has sha1 {actual}, expected {}",
                path.display(),
                expected.value()
            )));
        }
    }
    Ok(())
}

async fn remove_file(path: &Path) -> Result<(), LoaderInstallError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage(path, error)),
    }
}

/// A sha1 over every planned command, so a changed plan runs again.
fn fingerprint(planned: &[PlannedProcessor]) -> String {
    fn feed(hasher: &mut Sha1, tag: &str, value: &str) {
        hasher.update(tag.as_bytes());
        hasher.update(b"\0");
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut hasher = Sha1::new();
    for processor in planned {
        feed(&mut hasher, "jar", &text(&processor.jar));
        for entry in &processor.classpath {
            feed(&mut hasher, "classpath", &text(entry));
        }
        for arg in &processor.args {
            feed(&mut hasher, "arg", arg);
        }
        for output in &processor.outputs {
            feed(&mut hasher, "output", &text(&output.path));
            feed(
                &mut hasher,
                "sha1",
                output.sha1.as_ref().map_or("", |sha1| sha1.value()),
            );
        }
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// The sha1 of a file, read in chunks on the blocking pool.
async fn file_sha1(path: &Path) -> io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha1::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(io::Error::other)?
}
