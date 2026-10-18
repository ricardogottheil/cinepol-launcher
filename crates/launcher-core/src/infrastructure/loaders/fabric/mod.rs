//! Fabric: `meta.fabricmc.net` v2. Its releases carry a `stable` flag, and its
//! profiles declare a sha1 for most libraries but not for the loader itself.

use super::meta::MetaApi;
use crate::domain::loader::LoaderKind;

/// Loader releases per game version and their profiles live under this path.
pub const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2/versions/loader";

pub(super) const API: MetaApi = MetaApi {
    kind: LoaderKind::Fabric,
    base_url: FABRIC_META_URL,
};
