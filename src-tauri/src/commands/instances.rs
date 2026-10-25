use launcher_core::application::create_instance::{NewInstance, create_instance as create};
use launcher_core::application::install_instance::install_instance as install;
use launcher_core::application::launch_instance::{
    LaunchDeps, LaunchRequest, launch_instance as launch,
};
use launcher_core::application::list_instances::list_instances as list;
use launcher_core::application::prepare_cinepol_instance::cinepol_server_entry;
use launcher_core::domain::instance::InstanceOrigin;
use launcher_core::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
use launcher_core::domain::server_list::ServerAddress;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use super::parse_instance_id;
use crate::dto::{InstallSummaryDto, InstanceDto, LaunchOutcomeDto, LogLineDto, ProgressEvent};
use crate::error::{AppError, ErrorCode};
use crate::events::GAME_EXITED;
use crate::logs::ChannelLogSink;
use crate::progress::ChannelReporter;
use crate::state::AppState;

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<InstanceDto>, AppError> {
    Ok(list(&state.instances)
        .await?
        .iter()
        .map(InstanceDto::from)
        .collect())
}

/// Creates an instance with CINEPOL first in its server list. `loader` is
/// `vanilla`, `fabric`, `quilt`, `forge` or `neoforge`; all but vanilla need a
/// `loader_version`.
#[tauri::command]
pub async fn create_instance(
    state: State<'_, AppState>,
    name: String,
    game_version: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<InstanceDto, AppError> {
    let loader = parse_loader(&loader, loader_version.as_deref())?;
    let request = NewInstance {
        name: &name,
        game_version: &game_version,
        loader,
        origin: InstanceOrigin::Custom,
        seed_server: cinepol_server_entry(&state.current_config()),
    };
    Ok(InstanceDto::from(
        &create(request, &state.instances, &state.servers).await?,
    ))
}

/// Downloads everything the instance needs; cancellable with `cancel_operation`.
#[tauri::command]
pub async fn install_instance(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<InstallSummaryDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    // Claimed as a running game too: no install over a running game, no
    // launch over an install, and kill_instance stops the install.
    let Ok(_claim) = state.games.hold(id.as_str(), token.clone()) else {
        state.operations.finish(&operation_id);
        return Err(AppError::new(
            ErrorCode::AlreadyRunning,
            format!("{id} is already running"),
        ));
    };
    let result = install(
        &id,
        &state.install_deps(),
        &ChannelReporter::new(on_progress),
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    Ok(InstallSummaryDto::from(&result?))
}

/// Starts the game and resolves when it exits; output streams through `on_log`.
/// With `join_server` the game connects to that server as it starts.
#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    join_server: Option<String>,
    on_log: Channel<LogLineDto>,
) -> Result<LaunchOutcomeDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    // The address becomes a game argument: only a valid one gets through.
    let join_server = join_server
        .map(|raw| ServerAddress::parse(&raw))
        .transpose()
        .map_err(|error| AppError::new(ErrorCode::InvalidInput, error.to_string()))?
        .map(|address| address.to_string());
    let kill = state.games.start(id.as_str()).map_err(|_| {
        AppError::new(
            ErrorCode::AlreadyRunning,
            format!("{id} is already running"),
        )
    })?;
    let deps = LaunchDeps {
        instances: &state.instances,
        accounts: &state.accounts,
        versions: &state.versions,
        java: &state.java,
        natives: &state.natives,
        runner: &state.runner,
        clock: &state.clock,
        fs: &state.fs,
        paths: &state.paths,
        platform: &state.platform,
        sessions: &state.sessions(),
        launcher_name: crate::state::LAUNCHER_NAME,
        launcher_version: &state.launcher_version,
    };
    let request = LaunchRequest {
        instance: id.clone(),
        account: None,
        join_server,
    };
    let result = launch(&request, &deps, ChannelLogSink::new(on_log), kill).await;
    state.games.finish(id.as_str());
    let outcome = LaunchOutcomeDto::new(id.to_string(), &result?);
    // Other windows and the tray learn about the exit without holding the channel.
    let _ = app.emit(GAME_EXITED, &outcome);
    Ok(outcome)
}

/// Stops a running game. Returns `false` when it is not running.
#[tauri::command]
pub fn kill_instance(state: State<'_, AppState>, instance_id: String) -> Result<bool, AppError> {
    let id = parse_instance_id(&instance_id)?;
    Ok(state.games.cancel(id.as_str()))
}

/// Validates a loader choice from the UI.
fn parse_loader(kind: &str, version: Option<&str>) -> Result<LoaderSpec, AppError> {
    let kind = LoaderKind::from_id(kind)
        .ok_or_else(|| AppError::invalid_input(format!("unsupported loader {kind:?}")))?;
    let version = version
        .map(|raw| LoaderVersion::parse(raw.trim()))
        .transpose()
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    LoaderSpec::new(kind, version).map_err(|error| AppError::invalid_input(error.to_string()))
}

/// Opens one of the instance's crash reports with the system's default app.
/// Only a plain file name inside the instance's `crash-reports` folder is accepted.
#[tauri::command]
pub fn open_crash_report(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;

    let id = parse_instance_id(&instance_id)?;
    if !is_report_name(&file_name) {
        return Err(AppError::invalid_input("not a crash report name"));
    }
    let crash_reports = state
        .paths
        .game_dir(&id)
        .join(launcher_core::application::launch_instance::CRASH_REPORTS_DIR);
    let Some(path) = report_path(&crash_reports, &file_name) else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            format!("{file_name} was not found"),
        ));
    };
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))
}

/// Opens the instance's game folder (mods, saves, resource packs) in the
/// system's file manager, creating it when the instance was never installed.
/// Only folders of instances that exist are opened.
#[tauri::command]
pub fn open_instance_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;

    let id = parse_instance_id(&instance_id)?;
    if !state.paths.instance_dir(&id).is_dir() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            format!("instance {instance_id} was not found"),
        ));
    }
    let game_dir = state.paths.game_dir(&id);
    std::fs::create_dir_all(&game_dir)
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    app.opener()
        .open_path(game_dir.to_string_lossy(), None::<&str>)
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))
}

/// A plain file name: no separators, drive letters, NUL or dot segments.
fn is_report_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':', '\0']) && name != "." && name != ".."
}

/// The report itself: a regular `.txt` file (not a symlink) whose real path
/// is inside `crash_reports`, so nothing else can be opened with the default app.
fn report_path(crash_reports: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let path = crash_reports.join(name);
    let is_txt = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"));
    // `symlink_metadata` does not follow links, so a link is never "a file".
    let regular = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
    if !is_txt || !regular {
        return None;
    }
    let real = std::fs::canonicalize(&path).ok()?;
    let folder = std::fs::canonicalize(crash_reports).ok()?;
    real.starts_with(&folder).then_some(real)
}

#[cfg(test)]
mod tests {
    use super::{is_report_name, report_path};

    #[test]
    fn only_regular_txt_reports_inside_the_folder_open() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("crash-reports");
        std::fs::create_dir(&reports).unwrap();
        std::fs::write(reports.join("crash-a.txt"), b"report").unwrap();
        std::fs::write(reports.join("run.command"), b"#!/bin/sh").unwrap();
        let outside = dir.path().join("secret.txt");
        std::fs::write(&outside, b"secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, reports.join("link.txt")).unwrap();

        let opened = report_path(&reports, "crash-a.txt").unwrap();
        assert_eq!(opened.file_name().unwrap(), "crash-a.txt");
        assert_eq!(report_path(&reports, "run.command"), None);
        assert_eq!(report_path(&reports, "missing.txt"), None);
        #[cfg(unix)]
        assert_eq!(report_path(&reports, "link.txt"), None);
    }

    #[test]
    fn crash_report_names_are_plain_file_names() {
        assert!(is_report_name("crash-2026-10-19_12.00.00-client.txt"));
        for bad in [
            "", ".", "..", "../x.txt", "a/b.txt", "a\\b.txt", "C:x.txt", "a\0b",
        ] {
            assert!(!is_report_name(bad), "{bad:?}");
        }
    }
}
