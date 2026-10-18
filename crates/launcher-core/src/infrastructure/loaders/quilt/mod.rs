//! Quilt: `meta.quiltmc.org` v3. Its releases have no `stable` flag (betas
//! carry a `-beta.N` suffix), and its profiles declare no library sha1 at all.

use super::meta::MetaApi;
use crate::domain::loader::LoaderKind;

/// Loader releases per game version and their profiles live under this path.
pub const QUILT_META_URL: &str = "https://meta.quiltmc.org/v3/versions/loader";

pub(super) const API: MetaApi = MetaApi {
    kind: LoaderKind::Quilt,
    base_url: QUILT_META_URL,
};
