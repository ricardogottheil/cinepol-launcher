//! Composition root: wires infrastructure adapters into use cases.

use launcher_core::infrastructure::time::TokioDelay;

use crate::operations::OperationRegistry;

/// Application state shared with every command through `tauri::State`.
#[derive(Debug, Default)]
pub struct AppState {
    pub operations: OperationRegistry,
    pub delay: TokioDelay,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}
