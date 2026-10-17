//! File-system adapters: the `FileSystem` port, atomic writes and the JSON
//! repositories for instances and account metadata.

mod accounts;
mod atomic;
mod filesystem;
mod instances;
mod settings;

pub use accounts::JsonAccountRepository;
pub use atomic::write_atomic;
pub use filesystem::TokioFileSystem;
pub use instances::JsonInstanceRepository;
pub use settings::JsonSettingsRepository;
