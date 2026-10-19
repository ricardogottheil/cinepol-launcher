//! Managed Java runtimes (Mojang runtime manifest, Adoptium fallback) and `java -version` probing.

mod adoptium;
mod marker;
mod mojang;
mod probe;
mod provider;

pub use adoptium::adoptium_assets_url;
pub use mojang::MOJANG_RUNTIME_INDEX_URL;
pub use probe::ProcessJavaProbe;
pub use provider::ManagedJavaRuntimes;

#[cfg(test)]
mod tests;

/// Adversarial regression probes from the independent security verification.
/// Unix only: they attack symlinks and check exec bits, and on Windows links
/// are extracted as copies (`archive::extract::link_or_copy`).
#[cfg(test)]
#[cfg(unix)]
mod probes;
