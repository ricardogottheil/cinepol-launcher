//! File-system adapters: the `FileSystem` port, atomic writes and the JSON
//! repositories for instances and account metadata.

mod accounts;
mod atomic;
mod filesystem;
mod instances;
mod mod_registry;
mod pack_files;
mod pack_manifest;
mod saved_servers;
mod settings;

pub use accounts::JsonAccountRepository;
pub use atomic::write_atomic;
pub use filesystem::TokioFileSystem;
pub use instances::JsonInstanceRepository;
pub use mod_registry::JsonModRegistryStore;
pub use pack_files::StdPackFiles;
pub use pack_manifest::JsonPackManifestStore;
pub use saved_servers::JsonSavedServerRepository;
pub use settings::JsonSettingsRepository;
