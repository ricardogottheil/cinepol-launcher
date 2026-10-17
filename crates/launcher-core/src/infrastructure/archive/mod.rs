//! Archive extraction (zip, tar.gz) with zip-slip protection.
//!
//! Every entry name is validated before anything is written: absolute paths,
//! `..` components and Windows drive or UNC prefixes are rejected, and every
//! write target is checked against the canonicalized destination root.
//! Symbolic and hard links are only created when they resolve inside it.

mod extract;
mod natives;

pub(crate) use extract::create_link_inside;
pub use extract::{extract_tar_gz, extract_tar_gz_async, extract_zip, extract_zip_async};
pub use natives::ZipNativesExtractor;

#[cfg(test)]
mod tests;

/// Adversarial regression probes from the independent security verification.
#[cfg(test)]
mod probes;
