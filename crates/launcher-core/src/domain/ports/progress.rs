/// One progress step of a long-running operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressUpdate {
    /// Units completed so far.
    pub done: u64,
    /// Total units, when known.
    pub total: Option<u64>,
}

/// Receives progress from a use case. The desktop shell implements it with a
/// `tauri::ipc::Channel`; tests use an in-memory recorder.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, update: ProgressUpdate);
}
