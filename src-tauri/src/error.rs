//! The serializable error every command returns. The frontend switches on
//! `code`; `message` is for logs and developers, never parsed.

use launcher_core::application::run_progress_demo::ProgressDemoError;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Cancelled,
    InvalidInput,
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
        assert_eq!(
            AppError::from(ProgressDemoError::InvalidInterval).code,
            ErrorCode::InvalidInput
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
