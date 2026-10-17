use std::time::Duration;

use launcher_core::application::run_progress_demo::{ProgressDemoInput, run_progress_demo as run};
use tauri::State;
use tauri::ipc::Channel;

use crate::dto::ProgressEvent;
use crate::error::AppError;
use crate::progress::ChannelReporter;
use crate::state::AppState;

/// Demo of a cancellable long operation reporting progress through a Channel.
#[tauri::command]
pub async fn run_progress_demo(
    state: State<'_, AppState>,
    operation_id: String,
    steps: u32,
    interval_ms: u32,
    on_progress: Channel<ProgressEvent>,
) -> Result<(), AppError> {
    let token = state.operations.start(&operation_id)?;
    let input = ProgressDemoInput {
        steps,
        interval: Duration::from_millis(u64::from(interval_ms)),
    };
    let result = run(
        input,
        &ChannelReporter::new(on_progress),
        &state.delay,
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    result.map_err(AppError::from)
}

/// Cancels a running operation. Returns `false` when it already finished.
#[tauri::command]
pub fn cancel_operation(state: State<'_, AppState>, operation_id: String) -> bool {
    state.operations.cancel(&operation_id)
}
