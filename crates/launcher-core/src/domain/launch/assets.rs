//! Asset indexes: the content-addressed game resources shared by all instances.

use super::{Artifact, RelativePath};
use crate::domain::integrity::Checksum;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetObject {
    /// Logical name, e.g. `minecraft/sounds/ambient/cave/cave1.ogg`.
    pub name: String,
    /// SHA-1 of the content; also its address.
    pub hash: Checksum,
    pub size: u64,
    pub url: String,
}

impl AssetObject {
    /// `objects/<first two hex chars>/<hash>` relative to the assets root.
    pub fn object_path(&self) -> RelativePath {
        let hash = self.hash.value();
        let prefix = hash.get(..2).unwrap_or(hash);
        // A validated digest is hex only, so this path cannot escape the root.
        RelativePath::from_hex_digest_path(format!("objects/{prefix}/{hash}"))
    }

    /// The object as a download, relative to the assets root.
    pub fn artifact(&self) -> Artifact {
        Artifact {
            path: self.object_path(),
            url: self.url.clone(),
            checksum: Some(self.hash.clone()),
            size: Some(self.size),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetIndex {
    pub id: String,
    pub objects: Vec<AssetObject>,
}

impl AssetIndex {
    /// `indexes/<id>.json` relative to the assets root.
    pub fn index_path(id: &str) -> Option<RelativePath> {
        RelativePath::parse(&format!("indexes/{id}.json")).ok()
    }

    pub fn total_size(&self) -> u64 {
        self.objects.iter().map(|object| object.size).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::integrity::HashAlgorithm;

    #[test]
    fn objects_are_stored_by_hash_prefix() {
        let object = AssetObject {
            name: "minecraft/sounds/ambient/cave/cave1.ogg".into(),
            hash: Checksum::parse(
                HashAlgorithm::Sha1,
                "bdf48ef6b5d0d23bbb02e17d04865216179f510a",
            )
            .unwrap(),
            size: 16_854,
            url: "https://resources.example/bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a".into(),
        };
        assert_eq!(
            object.object_path().as_str(),
            "objects/bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a"
        );
        assert_eq!(object.artifact().size, Some(16_854));
        assert_eq!(
            AssetIndex::index_path("32").map(|p| p.to_string()),
            Some("indexes/32.json".into())
        );
        assert_eq!(AssetIndex::index_path("../x"), None);
    }
}
