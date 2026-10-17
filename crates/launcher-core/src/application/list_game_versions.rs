//! Lists the Minecraft versions a player can create an instance with.

use crate::domain::launch::{
    VersionKind, VersionManifest, VersionSource, VersionSourceError, VersionSummary,
};

/// Releases only by default; snapshots on request. Old alphas and betas are hidden.
pub async fn list_game_versions<V: VersionSource>(
    versions: &V,
    include_snapshots: bool,
) -> Result<(VersionManifest, Vec<VersionSummary>), VersionSourceError> {
    let manifest = versions.manifest().await?;
    let visible = manifest
        .versions
        .iter()
        .filter(|version| match version.kind {
            VersionKind::Release => true,
            VersionKind::Snapshot => include_snapshots,
            VersionKind::OldAlpha | VersionKind::OldBeta => false,
        })
        .cloned()
        .collect();
    Ok((manifest, visible))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::domain::instance::GameVersion;
    use crate::testing::FakeVersionSource;

    fn summary(id: &str, kind: VersionKind) -> VersionSummary {
        VersionSummary {
            id: GameVersion::parse(id).unwrap(),
            kind,
            release_time: String::new(),
        }
    }

    #[tokio::test]
    async fn hides_snapshots_unless_requested_and_always_hides_old_versions() {
        let source = FakeVersionSource {
            manifest: VersionManifest {
                latest_release: GameVersion::parse("26.2").unwrap(),
                latest_snapshot: GameVersion::parse("26.3-snapshot-1").unwrap(),
                versions: vec![
                    summary("26.3-snapshot-1", VersionKind::Snapshot),
                    summary("26.2", VersionKind::Release),
                    summary("b1.7.3", VersionKind::OldBeta),
                ],
            },
            versions: HashMap::new(),
            asset_indexes: HashMap::new(),
        };
        let ids = |list: Vec<VersionSummary>| {
            list.into_iter()
                .map(|v| v.id.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(list_game_versions(&source, false).await.unwrap().1),
            ["26.2"]
        );
        assert_eq!(
            ids(list_game_versions(&source, true).await.unwrap().1),
            ["26.3-snapshot-1", "26.2"]
        );
    }
}
