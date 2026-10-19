//! The serializable error every command returns. The frontend switches on
//! `code`; `message` is for logs and developers, never parsed.

use launcher_core::application::account_skin::AccountSkinError;
use launcher_core::application::create_instance::CreateInstanceError;
use launcher_core::application::create_offline_profile::CreateOfflineProfileError;
use launcher_core::application::duplicate_instance::DuplicateInstanceError;
use launcher_core::application::edit_server_list::EditServerListError;
use launcher_core::application::export_mrpack::ExportMrpackError;
use launcher_core::application::install_instance::InstallError;
use launcher_core::application::install_modpack::InstallModpackError;
use launcher_core::application::install_packwiz::InstallPackwizError;
use launcher_core::application::launch_instance::LaunchError;
use launcher_core::application::list_loader_versions::ListLoaderVersionsError;
use launcher_core::application::login_microsoft::LoginError;
use launcher_core::application::modpack_files::PackSyncError;
use launcher_core::application::refresh_session::SessionError;
use launcher_core::application::remove_account::RemoveAccountError;
use launcher_core::application::resolve_manual_downloads::ResolveManualDownloadsError;
use launcher_core::application::run_progress_demo::ProgressDemoError;
use launcher_core::application::update_instance_settings::UpdateInstanceSettingsError;
use launcher_core::application::update_modpack::UpdateModpackError;
use launcher_core::application::update_packwiz::UpdatePackwizError;
use launcher_core::domain::account::AccountRepositoryError;
use launcher_core::domain::account::AuthError;
use launcher_core::domain::instance::InstanceRepositoryError;
use launcher_core::domain::java::JavaRuntimeError;
use launcher_core::domain::launch::VersionSourceError;
use launcher_core::domain::loader::{LoaderInstallError, LoaderMetadataError};
use launcher_core::domain::modpack::mrpack::MrpackError;
use launcher_core::domain::modpack::{ModpackSourceError, PackFileError};
use launcher_core::domain::ports::DownloadError;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Cancelled,
    InvalidInput,
    NotFound,
    AlreadyExists,
    NoAccount,
    PremiumRequired,
    NotInstalled,
    Network,
    JavaUnavailable,
    AlreadyRunning,
    ServerUnreachable,
    /// The Microsoft session expired or was revoked: sign in again.
    SessionExpired,
    /// Microsoft or Xbox refused the sign-in (no Xbox profile, child account, region).
    SignInRefused,
    /// The account does not own Minecraft: Java Edition, or has no profile yet.
    NotEntitled,
    /// The launcher's Azure app is not approved for Minecraft Services yet.
    AppNotApproved,
    /// The OS keychain is missing or refused access.
    KeychainUnavailable,
    /// This build has no `MSA_CLIENT_ID`, so it cannot sign in with Microsoft.
    MicrosoftNotConfigured,
    /// The loader (or that loader version) does not exist for the game version,
    /// or this launcher cannot install it yet.
    LoaderUnavailable,
    /// A file declares no checksum, so it is never downloaded or launched.
    UnverifiedFile,
    /// A catalog asked the launcher to slow down; try again in a moment.
    RateLimited,
    /// A modpack archive or index is broken, unsafe or unsupported.
    InvalidModpack,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub details: Option<String>,
}

impl AppError {
    /// Internal errors are logged here, once, because the UI only shows a
    /// generic message and points to the log. Messages never hold secrets
    /// (core errors are written without request bodies or tokens), and the
    /// redacting writer masks anything token-shaped anyway.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        if code == ErrorCode::Internal {
            tracing::error!(%message, "a command failed");
        }
        Self {
            code,
            message,
            details: None,
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }
}

/// Shell failures (window, IPC, paths) are never the user's fault.
impl From<tauri::Error> for AppError {
    fn from(error: tauri::Error) -> Self {
        Self::new(ErrorCode::Internal, error.to_string())
    }
}

impl From<ProgressDemoError> for AppError {
    fn from(error: ProgressDemoError) -> Self {
        let code = match error {
            ProgressDemoError::InvalidSteps(_) | ProgressDemoError::InvalidInterval => {
                ErrorCode::InvalidInput
            }
            ProgressDemoError::Cancelled => ErrorCode::Cancelled,
        };
        Self::new(code, error.to_string())
    }
}

impl From<AccountRepositoryError> for AppError {
    fn from(error: AccountRepositoryError) -> Self {
        Self::new(ErrorCode::Internal, error.to_string())
    }
}

impl From<InstanceRepositoryError> for AppError {
    fn from(error: InstanceRepositoryError) -> Self {
        let code = match error {
            InstanceRepositoryError::NotFound(_) => ErrorCode::NotFound,
            InstanceRepositoryError::Storage(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

impl From<VersionSourceError> for AppError {
    fn from(error: VersionSourceError) -> Self {
        let code = match error {
            VersionSourceError::UnknownVersion(_) => ErrorCode::NotFound,
            VersionSourceError::Network(_) => ErrorCode::Network,
            VersionSourceError::Invalid(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

impl From<CreateOfflineProfileError> for AppError {
    fn from(error: CreateOfflineProfileError) -> Self {
        let code = match &error {
            CreateOfflineProfileError::InvalidUsername(_) => ErrorCode::InvalidInput,
            CreateOfflineProfileError::PremiumRequired(_) => ErrorCode::PremiumRequired,
            CreateOfflineProfileError::AlreadyExists(_) => ErrorCode::AlreadyExists,
            CreateOfflineProfileError::Storage(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

impl From<CreateInstanceError> for AppError {
    fn from(error: CreateInstanceError) -> Self {
        let code = match &error {
            CreateInstanceError::InvalidName(_) | CreateInstanceError::InvalidVersion(_) => {
                ErrorCode::InvalidInput
            }
            CreateInstanceError::Storage(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

fn java_code(error: &JavaRuntimeError) -> ErrorCode {
    match error {
        JavaRuntimeError::Unavailable { .. } => ErrorCode::JavaUnavailable,
        JavaRuntimeError::Download(_) => ErrorCode::Network,
        JavaRuntimeError::Invalid(_) => ErrorCode::Internal,
        JavaRuntimeError::Cancelled => ErrorCode::Cancelled,
    }
}

/// The code a loader install failure is reported with.
fn loader_code(error: &LoaderInstallError) -> ErrorCode {
    match error {
        LoaderInstallError::UnsupportedGameVersion(..)
        | LoaderInstallError::UnknownLoaderVersion(..)
        | LoaderInstallError::UnsupportedLoader(_) => ErrorCode::LoaderUnavailable,
        LoaderInstallError::Network(_) => ErrorCode::Network,
        LoaderInstallError::Invalid(_)
        | LoaderInstallError::Storage(_)
        | LoaderInstallError::ProcessorFailed(_)
        | LoaderInstallError::OutputMismatch(_) => ErrorCode::Internal,
        LoaderInstallError::Cancelled => ErrorCode::Cancelled,
    }
}

impl From<ListLoaderVersionsError> for AppError {
    fn from(error: ListLoaderVersionsError) -> Self {
        let code = match &error {
            ListLoaderVersionsError::InvalidGameVersion(_) => ErrorCode::InvalidInput,
            ListLoaderVersionsError::Metadata(LoaderMetadataError::UnsupportedGameVersion(..)) => {
                ErrorCode::LoaderUnavailable
            }
            ListLoaderVersionsError::Metadata(LoaderMetadataError::Network(_)) => {
                ErrorCode::Network
            }
        };
        Self::new(code, error.to_string())
    }
}

impl From<InstallError> for AppError {
    fn from(error: InstallError) -> Self {
        let code = match &error {
            InstallError::Instance(InstanceRepositoryError::NotFound(_)) => ErrorCode::NotFound,
            InstallError::Instance(_) => ErrorCode::Internal,
            InstallError::Version(VersionSourceError::UnknownVersion(_)) => ErrorCode::NotFound,
            InstallError::Version(VersionSourceError::Network(_)) => ErrorCode::Network,
            InstallError::Version(_) => ErrorCode::Internal,
            InstallError::Download(DownloadError::Io { .. }) => ErrorCode::Internal,
            InstallError::Download(_) => ErrorCode::Network,
            InstallError::Java(java) => java_code(java),
            InstallError::Loader(loader) => loader_code(loader),
            InstallError::UnverifiedLibrary(_) => ErrorCode::UnverifiedFile,
            InstallError::Cancelled => ErrorCode::Cancelled,
        };
        Self::new(code, error.to_string())
    }
}

/// The code a Microsoft session failure is reported with.
pub fn session_code(error: &SessionError) -> ErrorCode {
    match error {
        SessionError::Expired => ErrorCode::SessionExpired,
        SessionError::Auth(auth) => auth_code(auth),
        SessionError::Keychain(_) => ErrorCode::KeychainUnavailable,
    }
}

/// The code a Microsoft sign-in failure is reported with.
pub fn auth_code(error: &AuthError) -> ErrorCode {
    match error {
        AuthError::SessionExpired | AuthError::CodeExpired => ErrorCode::SessionExpired,
        AuthError::Declined
        | AuthError::NoXboxAccount
        | AuthError::XboxUnavailableInCountry
        | AuthError::ChildAccount => ErrorCode::SignInRefused,
        AuthError::NotEntitled | AuthError::NoProfile => ErrorCode::NotEntitled,
        AuthError::AppNotApproved => ErrorCode::AppNotApproved,
        AuthError::Network(_) => ErrorCode::Network,
        AuthError::Unexpected(_) => ErrorCode::Internal,
    }
}

impl From<LaunchError> for AppError {
    fn from(error: LaunchError) -> Self {
        let code = match &error {
            LaunchError::Instance(InstanceRepositoryError::NotFound(_)) => ErrorCode::NotFound,
            LaunchError::NoAccount => ErrorCode::NoAccount,
            LaunchError::Session(session) => session_code(session),
            LaunchError::UnsupportedLoader(_) => ErrorCode::LoaderUnavailable,
            LaunchError::Version(VersionSourceError::Network(_)) => ErrorCode::Network,
            LaunchError::NotInstalled(_) => ErrorCode::NotInstalled,
            LaunchError::Java(java) => java_code(java),
            _ => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

impl From<launcher_core::domain::server::ServerStatusError> for AppError {
    fn from(error: launcher_core::domain::server::ServerStatusError) -> Self {
        Self::new(ErrorCode::ServerUnreachable, error.to_string())
    }
}

impl From<launcher_core::application::update_settings::UpdateSettingsError> for AppError {
    fn from(error: launcher_core::application::update_settings::UpdateSettingsError) -> Self {
        use launcher_core::application::update_settings::UpdateSettingsError;
        let code = match &error {
            UpdateSettingsError::InvalidUrl(_) => ErrorCode::InvalidInput,
            UpdateSettingsError::Storage(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

impl From<launcher_core::domain::settings::SettingsError> for AppError {
    fn from(error: launcher_core::domain::settings::SettingsError) -> Self {
        Self::new(ErrorCode::Internal, error.to_string())
    }
}

impl From<launcher_core::application::play_on_cinepol::PlayOnCinepolError> for AppError {
    fn from(error: launcher_core::application::play_on_cinepol::PlayOnCinepolError) -> Self {
        use launcher_core::application::play_on_cinepol::PlayOnCinepolError;
        match error {
            PlayOnCinepolError::Prepare(prepare) => Self::from(prepare),
            PlayOnCinepolError::Install(install) => Self::from(install),
            PlayOnCinepolError::Launch(launch) => Self::from(launch),
            PlayOnCinepolError::AlreadyRunning(_) => {
                Self::new(ErrorCode::AlreadyRunning, error.to_string())
            }
        }
    }
}

impl From<LoginError> for AppError {
    fn from(error: LoginError) -> Self {
        let code = match &error {
            LoginError::Auth(auth) => auth_code(auth),
            LoginError::Cancelled => ErrorCode::Cancelled,
            LoginError::Storage(_) => ErrorCode::Internal,
            LoginError::Keychain(_) => ErrorCode::KeychainUnavailable,
        };
        Self::new(code, error.to_string())
    }
}

impl From<RemoveAccountError> for AppError {
    fn from(error: RemoveAccountError) -> Self {
        let code = match &error {
            RemoveAccountError::NotFound => ErrorCode::NotFound,
            RemoveAccountError::Storage(_) | RemoveAccountError::Instances(_) => {
                ErrorCode::Internal
            }
            RemoveAccountError::Keychain(_) => ErrorCode::KeychainUnavailable,
        };
        Self::new(code, error.to_string())
    }
}

impl From<AccountSkinError> for AppError {
    fn from(error: AccountSkinError) -> Self {
        let code = match &error {
            AccountSkinError::NotFound => ErrorCode::NotFound,
            AccountSkinError::Storage(_) => ErrorCode::Internal,
            AccountSkinError::Auth(auth) => auth_code(auth),
        };
        Self::new(code, error.to_string())
    }
}

/// The code a modpack catalog failure is reported with.
fn source_code(error: &ModpackSourceError) -> ErrorCode {
    match error {
        ModpackSourceError::NotFound(_) => ErrorCode::NotFound,
        ModpackSourceError::RateLimited { .. } => ErrorCode::RateLimited,
        ModpackSourceError::Network(_) => ErrorCode::Network,
        ModpackSourceError::NotConfigured(_) | ModpackSourceError::InvalidResponse(_) => {
            ErrorCode::Internal
        }
    }
}

/// The code a modpack file failure is reported with.
fn pack_file_code(error: &PackFileError) -> ErrorCode {
    match error {
        PackFileError::Io { .. } => ErrorCode::Internal,
        PackFileError::Index(MrpackError::UnverifiedFile(_)) => ErrorCode::UnverifiedFile,
        PackFileError::InvalidArchive(_)
        | PackFileError::UnsafeEntry(_)
        | PackFileError::Index(_)
        | PackFileError::Packwiz(_)
        | PackFileError::TooLarge(_) => ErrorCode::InvalidModpack,
        PackFileError::Source(_) => ErrorCode::Network,
        PackFileError::HashMismatch(_) => ErrorCode::UnverifiedFile,
        PackFileError::InvalidLocation(_) => ErrorCode::InvalidInput,
    }
}

fn pack_sync_code(error: &PackSyncError) -> ErrorCode {
    match error {
        PackSyncError::Files(files) => pack_file_code(files),
        PackSyncError::Download(DownloadError::Io { .. }) | PackSyncError::Scratch(_) => {
            ErrorCode::Internal
        }
        PackSyncError::Download(_) => ErrorCode::Network,
        PackSyncError::Cancelled => ErrorCode::Cancelled,
    }
}

impl From<ModpackSourceError> for AppError {
    fn from(error: ModpackSourceError) -> Self {
        Self::new(source_code(&error), error.to_string())
    }
}

impl From<InstallModpackError> for AppError {
    fn from(error: InstallModpackError) -> Self {
        let code = match &error {
            InstallModpackError::Source(source) => source_code(source),
            InstallModpackError::UnsupportedSource(_) => ErrorCode::InvalidInput,
            InstallModpackError::VersionNotFound(_) => ErrorCode::NotFound,
            InstallModpackError::NoPackFile(_) => ErrorCode::InvalidModpack,
            InstallModpackError::Create(CreateInstanceError::Storage(_)) => ErrorCode::Internal,
            InstallModpackError::Create(_) => ErrorCode::InvalidModpack,
            InstallModpackError::Pack(pack) => pack_sync_code(pack),
            InstallModpackError::Cancelled => ErrorCode::Cancelled,
            InstallModpackError::Busy(_) => ErrorCode::AlreadyRunning,
        };
        Self::new(code, error.to_string())
    }
}

impl From<UpdateModpackError> for AppError {
    fn from(error: UpdateModpackError) -> Self {
        let code = match &error {
            UpdateModpackError::Instance(InstanceRepositoryError::NotFound(_)) => {
                ErrorCode::NotFound
            }
            UpdateModpackError::Instance(_) => ErrorCode::Internal,
            UpdateModpackError::NotFromCatalog(_) | UpdateModpackError::UnsupportedSource(_) => {
                ErrorCode::InvalidInput
            }
            UpdateModpackError::Source(source) => source_code(source),
            UpdateModpackError::VersionNotFound(_)
            | UpdateModpackError::NoInstallableVersion(_) => ErrorCode::NotFound,
            UpdateModpackError::NoPackFile(_) => ErrorCode::InvalidModpack,
            UpdateModpackError::Pack(pack) => pack_sync_code(pack),
            UpdateModpackError::Cancelled => ErrorCode::Cancelled,
        };
        Self::new(code, error.to_string())
    }
}

impl From<ExportMrpackError> for AppError {
    fn from(error: ExportMrpackError) -> Self {
        let code = match &error {
            ExportMrpackError::NotMrpack(_) => ErrorCode::InvalidInput,
            ExportMrpackError::Instance(InstanceRepositoryError::NotFound(_)) => {
                ErrorCode::NotFound
            }
            ExportMrpackError::Instance(_) => ErrorCode::Internal,
            ExportMrpackError::Catalog(source) => source_code(source),
            ExportMrpackError::Files(files) => pack_file_code(files),
        };
        Self::new(code, error.to_string())
    }
}

impl From<DuplicateInstanceError> for AppError {
    fn from(error: DuplicateInstanceError) -> Self {
        let code = match &error {
            DuplicateInstanceError::InvalidName(_) => ErrorCode::InvalidInput,
            DuplicateInstanceError::Instance(InstanceRepositoryError::NotFound(_)) => {
                ErrorCode::NotFound
            }
            DuplicateInstanceError::Instance(_) | DuplicateInstanceError::Copy(_) => {
                ErrorCode::Internal
            }
        };
        Self::new(code, error.to_string())
    }
}

impl From<UpdateInstanceSettingsError> for AppError {
    fn from(error: UpdateInstanceSettingsError) -> Self {
        let code = match &error {
            UpdateInstanceSettingsError::InvalidName(_)
            | UpdateInstanceSettingsError::InvalidSettings(_) => ErrorCode::InvalidInput,
            UpdateInstanceSettingsError::InvalidJava { .. } => ErrorCode::JavaUnavailable,
            UpdateInstanceSettingsError::UnknownAccount(_) => ErrorCode::NotFound,
            UpdateInstanceSettingsError::Instance(InstanceRepositoryError::NotFound(_)) => {
                ErrorCode::NotFound
            }
            UpdateInstanceSettingsError::Instance(_) | UpdateInstanceSettingsError::Accounts(_) => {
                ErrorCode::Internal
            }
        };
        Self::new(code, error.to_string())
    }
}

impl From<InstallPackwizError> for AppError {
    fn from(error: InstallPackwizError) -> Self {
        let code = match &error {
            InstallPackwizError::Create(CreateInstanceError::Storage(_)) => ErrorCode::Internal,
            InstallPackwizError::Create(_) => ErrorCode::InvalidModpack,
            InstallPackwizError::Pack(pack) => pack_sync_code(pack),
            InstallPackwizError::Cancelled => ErrorCode::Cancelled,
            InstallPackwizError::Busy(_) => ErrorCode::AlreadyRunning,
        };
        Self::new(code, error.to_string())
    }
}

impl From<UpdatePackwizError> for AppError {
    fn from(error: UpdatePackwizError) -> Self {
        let code = match &error {
            UpdatePackwizError::Instance(InstanceRepositoryError::NotFound(_)) => {
                ErrorCode::NotFound
            }
            UpdatePackwizError::Instance(_) => ErrorCode::Internal,
            UpdatePackwizError::NotPackwiz(_) => ErrorCode::InvalidInput,
            UpdatePackwizError::Pack(pack) => pack_sync_code(pack),
            UpdatePackwizError::Cancelled => ErrorCode::Cancelled,
        };
        Self::new(code, error.to_string())
    }
}

impl From<ResolveManualDownloadsError> for AppError {
    fn from(error: ResolveManualDownloadsError) -> Self {
        let code = match &error {
            ResolveManualDownloadsError::Files(files) => pack_file_code(files),
        };
        Self::new(code, error.to_string())
    }
}

impl From<EditServerListError> for AppError {
    fn from(error: EditServerListError) -> Self {
        let code = match &error {
            EditServerListError::InvalidName | EditServerListError::InvalidAddress(_) => {
                ErrorCode::InvalidInput
            }
            EditServerListError::OutOfRange(_) => ErrorCode::NotFound,
            EditServerListError::Storage(_) => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_instance_management_errors_to_actionable_codes() {
        assert_eq!(
            AppError::from(UpdateInstanceSettingsError::InvalidJava {
                path: "/x".into(),
                reason: "not java".into(),
            })
            .code,
            ErrorCode::JavaUnavailable
        );
        assert_eq!(
            AppError::from(DuplicateInstanceError::InvalidName(
                launcher_core::domain::instance::InstanceName::parse(" ").unwrap_err()
            ))
            .code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn maps_packwiz_errors_to_actionable_codes() {
        let unsafe_path = PackSyncError::Files(PackFileError::Packwiz(
            launcher_core::domain::modpack::packwiz::PackwizError::UnsafePath("../x".into()),
        ));
        assert_eq!(
            AppError::from(InstallPackwizError::Pack(unsafe_path)).code,
            ErrorCode::InvalidModpack
        );
        let unreachable = PackSyncError::Files(PackFileError::Source("offline".into()));
        assert_eq!(
            AppError::from(UpdatePackwizError::Pack(unreachable)).code,
            ErrorCode::Network
        );
        assert_eq!(
            AppError::from(InstallPackwizError::Busy(
                launcher_core::domain::instance::InstanceId::parse("fo").unwrap()
            ))
            .code,
            ErrorCode::AlreadyRunning
        );
    }

    #[test]
    fn maps_modpack_errors_to_actionable_codes() {
        let rate_limited = ModpackSourceError::RateLimited {
            retry_after_secs: 30,
        };
        assert_eq!(AppError::from(rate_limited).code, ErrorCode::RateLimited);
        let unsafe_entry = PackSyncError::Files(PackFileError::UnsafeEntry("../x".into()));
        assert_eq!(
            AppError::from(InstallModpackError::Pack(unsafe_entry)).code,
            ErrorCode::InvalidModpack
        );
        let unverified = PackFileError::Index(MrpackError::UnverifiedFile("mods/a.jar".into()));
        assert_eq!(
            AppError::from(ExportMrpackError::Files(unverified)).code,
            ErrorCode::UnverifiedFile
        );
        assert_eq!(
            AppError::from(UpdateModpackError::Cancelled).code,
            ErrorCode::Cancelled
        );
    }

    #[test]
    fn maps_demo_errors_to_codes() {
        assert_eq!(
            AppError::from(ProgressDemoError::Cancelled).code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            AppError::from(ProgressDemoError::InvalidSteps(0)).code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn maps_launch_and_install_errors_to_actionable_codes() {
        assert_eq!(
            AppError::from(LaunchError::NoAccount).code,
            ErrorCode::NoAccount
        );
        assert_eq!(
            AppError::from(LaunchError::NotInstalled("x.jar".into())).code,
            ErrorCode::NotInstalled
        );
        assert_eq!(
            AppError::from(InstallError::Cancelled).code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            AppError::from(InstallError::Java(JavaRuntimeError::Unavailable {
                major: 25,
                platform: "linux".into()
            }))
            .code,
            ErrorCode::JavaUnavailable
        );
        assert_eq!(
            AppError::from(InstallError::Download(DownloadError::Http {
                url: "u".into(),
                reason: "r".into()
            }))
            .code,
            ErrorCode::Network
        );
    }

    #[test]
    fn serializes_with_snake_case_code_and_without_empty_details() {
        let json = serde_json::to_value(AppError::from(ProgressDemoError::Cancelled)).unwrap();
        assert_eq!(json["code"], "cancelled");
        assert_eq!(json["message"], "the operation was cancelled");
        assert!(json.get("details").is_none());
    }
}
