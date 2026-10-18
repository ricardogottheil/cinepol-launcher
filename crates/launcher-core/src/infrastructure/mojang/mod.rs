//! Mojang version manifest, version JSONs and asset indexes.
//!
//! [`MojangVersionSource`] implements the `VersionSource` port: it reads
//! Mojang's metadata over HTTP, verifies every SHA-1 it declares and caches the
//! version JSONs and asset indexes on disk so installed versions keep launching
//! offline. [`MojangPremiumNames`] asks the public profile API whether a
//! premium account owns a name.

mod cache;
mod json;
mod profiles;
mod source;

pub use profiles::{MojangPremiumNames, PROFILES_URL};

pub use source::{
    ASSET_OBJECTS_BASE_URL, LIBRARIES_BASE_URL, MojangVersionSource, VERSION_MANIFEST_URL,
};
