//! Cross-cutting ports: traits the application layer depends on and
//! infrastructure implements. Feature-specific ports (repositories, sources)
//! live next to their entities, e.g. `domain::account::AccountRepository`.

mod clock;
mod delay;
mod download;
mod filesystem;
mod http;
mod progress;
mod secrets;

pub use clock::{Clock, unix_seconds};
pub use delay::Delay;
pub use download::{DownloadError, DownloadRequest, DownloadSummary, Downloader};
pub use filesystem::{FileSystem, FileSystemError};
pub use http::{
    ApiKeyHeader, HttpBody, HttpClient, HttpError, HttpMethod, HttpRequest, HttpRequester,
    HttpResponse, MultipartPart,
};
pub use progress::{ProgressReporter, ProgressStage, ProgressUnit, ProgressUpdate, StagedReporter};
pub use secrets::{Secret, SecretKey, SecretStore, SecretStoreError};
