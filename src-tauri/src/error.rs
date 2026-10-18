//! The serializable error every command returns. The frontend switches on
//! `code`; `message` is for logs and developers, never parsed.

use launcher_core::application::create_instance::CreateInstanceError;
use launcher_core::application::create_offline_profile::CreateOfflineProfileError;
use launcher_core::application::install_instance::InstallError;
use launcher_core::application::launch_instance::LaunchError;
use launcher_core::application::refresh_session::SessionError;
use launcher_core::application::run_progress_demo::ProgressDemoError;
use launcher_core::domain::account::AccountRepositoryError;
use launcher_core::domain::account::AuthError;
use launcher_core::domain::instance::InstanceRepositoryError;
use launcher_core::domain::java::JavaRuntimeError;
use launcher_core::domain::launch::VersionSourceError;
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
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
