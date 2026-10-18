/// Which part of a long operation is running, so the UI can label it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgressStage {
    Metadata,
    Libraries,
    Assets,
    Java,
    /// Loader installer processors (Forge, NeoForge).
    Loader,
    Natives,
    Launching,
}

/// What `done` and `total` count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressUnit {
    /// Steps or files.
    #[default]
    Steps,
    /// Downloaded bytes.
    Bytes,
}

/// One progress step of a long-running operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressUpdate {
    /// Units completed so far, counted in `unit`.
    pub done: u64,
    /// Total units, when known.
    pub total: Option<u64>,
    pub stage: Option<ProgressStage>,
    pub unit: ProgressUnit,
}

impl ProgressUpdate {
    /// Progress counted in steps.
    pub fn new(done: u64, total: Option<u64>) -> Self {
        Self {
            done,
            total,
            stage: None,
            unit: ProgressUnit::Steps,
        }
    }

    /// Progress counted in downloaded bytes.
    pub fn bytes(done: u64, total: Option<u64>) -> Self {
        Self {
            unit: ProgressUnit::Bytes,
            ..Self::new(done, total)
        }
    }
}

/// Receives progress from a use case. The desktop shell implements it with a
/// `tauri::ipc::Channel`; tests use an in-memory recorder.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, update: ProgressUpdate);
}

/// Tags every update with a stage before forwarding it.
pub struct StagedReporter<'a, R: ProgressReporter> {
    inner: &'a R,
    stage: ProgressStage,
}

impl<'a, R: ProgressReporter> StagedReporter<'a, R> {
    pub fn new(inner: &'a R, stage: ProgressStage) -> Self {
        Self { inner, stage }
    }
}

impl<R: ProgressReporter> ProgressReporter for StagedReporter<'_, R> {
    fn report(&self, update: ProgressUpdate) {
        self.inner.report(ProgressUpdate {
            stage: Some(self.stage),
            ..update
        });
    }
}
