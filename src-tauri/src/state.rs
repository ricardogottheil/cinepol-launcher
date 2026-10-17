//! Composition root: wires infrastructure adapters into use cases.
//!
//! Use cases and adapters are registered here as milestones add them.

/// Application state shared with every command through `tauri::State`.
#[derive(Debug, Default)]
pub struct AppState {}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}
