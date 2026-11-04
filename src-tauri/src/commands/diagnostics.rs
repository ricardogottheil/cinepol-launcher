//! Network diagnostics: an MTR-style test of the path to a server, and the
//! shareable report.

use std::time::Duration;

use launcher_core::application::diagnose_connection::{DiagnoseDeps, diagnose_connection};
use launcher_core::application::share_diagnostic_report::share_diagnostic_report as share;
use launcher_core::domain::diagnostics::{Diagnostic, DiagnosticSink};
use tauri::State;
use tauri::ipc::Channel;

use crate::dto::{DiagnosticDto, SharedReportDto};
use crate::error::AppError;
use crate::state::AppState;

/// Forwards every diagnosis update to the webview.
struct ChannelDiagnosticSink(Channel<DiagnosticDto>);

impl DiagnosticSink for ChannelDiagnosticSink {
    fn update(&self, diagnostic: &Diagnostic) {
        // A send only fails when the webview dropped the channel (page
        // reload); the diagnosis keeps going and still returns its result.
        let _ = self.0.send(DiagnosticDto::from(diagnostic));
    }
}

/// Diagnoses the connection to `address` for `duration_secs` (60, 300 or
/// 600), sending the diagnosis through `on_update` about once per second.
/// `cancel_operation(operation_id)` stops it early; the result then holds
/// what was measured.
#[tauri::command]
pub async fn diagnose_server(
    state: State<'_, AppState>,
    operation_id: String,
    address: String,
    duration_secs: u32,
    on_update: Channel<DiagnosticDto>,
) -> Result<DiagnosticDto, AppError> {
    let token = state.operations.start(&operation_id)?;
    let deps = DiagnoseDeps {
        hops: &state.hop_prober,
        connections: &state.connection_timer,
        status: &state.diagnostic_pinger,
        delay: &state.delay,
        clock: &state.clock,
    };
    let result = diagnose_connection(
        &address,
        Duration::from_secs(u64::from(duration_secs)),
        &deps,
        &ChannelDiagnosticSink(on_update),
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    Ok(DiagnosticDto::from(&result?))
}

/// Uploads a diagnosis report (plain text written by the frontend) to
/// mclo.gs and returns its link.
#[tauri::command]
pub async fn share_diagnostic_report(
    state: State<'_, AppState>,
    content: String,
) -> Result<SharedReportDto, AppError> {
    let shared = share(&content, &state.report_host).await?;
    Ok(SharedReportDto::from(shared))
}
