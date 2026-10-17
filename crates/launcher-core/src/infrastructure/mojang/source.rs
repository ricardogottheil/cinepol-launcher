//! The `VersionSource` adapter backed by Mojang's metadata servers.

use std::collections::HashMap;
use std::path::Path;

use super::cache::{read_if_present, sha1_hex, write_atomic};
use super::json::{self, ManifestEntry};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::{
    AssetIndex, AssetIndexRef, LauncherPaths, VersionManifest, VersionSource, VersionSourceError,
    VersionSpec,
};
use crate::domain::ports::{HttpClient, HttpError};

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const ASSET_OBJECTS_BASE_URL: &str = "https://resources.download.minecraft.net";
pub const LIBRARIES_BASE_URL: &str = "https://libraries.minecraft.net/";

/// Loader profiles may inherit from profiles; deeper chains are rejected.
const MAX_INHERITANCE_DEPTH: usize = 5;

/// Why the manifest could not tell where a version lives.
#[derive(Debug, Clone)]
enum ManifestLookup {
    Entries(HashMap<String, ManifestEntry>),
    NotFound,
    Failed(String),
}

/// Mojang's version catalog with an on-disk cache under [`LauncherPaths`].
#[derive(Debug)]
pub struct MojangVersionSource<H> {
    http: H,
    paths: LauncherPaths,
}

impl<H: HttpClient> MojangVersionSource<H> {
    pub fn new(http: H, paths: LauncherPaths) -> Self {
        Self { http, paths }
    }

    async fn manifest_lookup(&self) -> ManifestLookup {
        let bytes = match self.http.get_bytes(VERSION_MANIFEST_URL).await {
            Ok(bytes) => bytes,
            Err(HttpError::NotFound(_)) => return ManifestLookup::NotFound,
            Err(error) => return ManifestLookup::Failed(error.to_string()),
        };
        match json::manifest_entries(&bytes) {
            Ok(entries) => ManifestLookup::Entries(
                entries
                    .into_iter()
                    .map(|entry| (entry.id.clone(), entry))
                    .collect(),
            ),
            Err(error) => ManifestLookup::Failed(error.to_string()),
        }
    }

    /// Returns the cached file when its SHA-1 matches; otherwise downloads,
    /// verifies and caches it. Cache write failures only cost a re-download.
    async fn fetch_verified(
        &self,
        cache: &Path,
        url: &str,
        checksum: Option<&Checksum>,
        on_http_error: impl FnOnce(HttpError) -> VersionSourceError,
    ) -> Result<Vec<u8>, VersionSourceError> {
        if let Some(checksum) = checksum
            && let Some(bytes) = read_if_present(cache).await
            && checksum.matches(&sha1_hex(&bytes))
        {
            return Ok(bytes);
        }
        let bytes = self.http.get_bytes(url).await.map_err(on_http_error)?;
        if let Some(checksum) = checksum
            && !checksum.matches(&sha1_hex(&bytes))
        {
            return Err(VersionSourceError::Invalid(format!(
                "sha1 mismatch for {url}"
            )));
        }
        if let Err(error) = write_atomic(cache, &bytes).await {
            tracing::warn!(path = %cache.display(), %error, "could not cache Mojang metadata");
        }
        Ok(bytes)
    }

    /// One version JSON, without resolving `inheritsFrom`.
    async fn load_version(
        &self,
        id: &str,
        manifest: &mut Option<ManifestLookup>,
    ) -> Result<VersionSpec, VersionSourceError> {
        GameVersion::parse(id)
            .map_err(|_| VersionSourceError::Invalid(format!("unsafe version id {id:?}")))?;
        let cache = self
            .paths
            .version_json(id)
            .ok_or_else(|| VersionSourceError::Invalid(format!("unsafe version id {id:?}")))?;

        let lookup = match manifest {
            Some(lookup) => lookup,
            None => manifest.insert(self.manifest_lookup().await),
        };
        let unavailable = match lookup {
            ManifestLookup::Entries(entries) => match entries.get(id) {
                Some(entry) => {
                    // A declared but malformed digest must fail: silently dropping it
                    // would download and cache the file without any verification.
                    let checksum = entry
                        .sha1
                        .as_deref()
                        .map(|raw| Checksum::parse(HashAlgorithm::Sha1, raw))
                        .transpose()
                        .map_err(|error| {
                            VersionSourceError::Invalid(format!("manifest sha1 for {id}: {error}"))
                        })?;
                    let bytes = self
                        .fetch_verified(&cache, &entry.url, checksum.as_ref(), |error| {
                            version_http_error(id, error)
                        })
                        .await?;
                    return parse_cached_version(id, &bytes);
                }
                None => VersionSourceError::UnknownVersion(id.to_owned()),
            },
            ManifestLookup::NotFound => VersionSourceError::UnknownVersion(id.to_owned()),
            ManifestLookup::Failed(reason) => VersionSourceError::Network(reason.clone()),
        };

        match read_if_present(&cache).await {
            Some(bytes) => parse_cached_version(id, &bytes),
            None => Err(unavailable),
        }
    }
}

fn version_http_error(id: &str, error: HttpError) -> VersionSourceError {
    match error {
        HttpError::NotFound(_) => VersionSourceError::UnknownVersion(id.to_owned()),
        other => VersionSourceError::Network(other.to_string()),
    }
}

fn parse_cached_version(id: &str, bytes: &[u8]) -> Result<VersionSpec, VersionSourceError> {
    let spec = json::parse_version(bytes)?;
    if spec.id != id {
        return Err(VersionSourceError::Invalid(format!(
            "version JSON for {id} declares id {:?}",
            spec.id
        )));
    }
    Ok(spec)
}

impl<H: HttpClient> VersionSource for MojangVersionSource<H> {
    async fn manifest(&self) -> Result<VersionManifest, VersionSourceError> {
        let bytes = self
            .http
            .get_bytes(VERSION_MANIFEST_URL)
            .await
            .map_err(|error| VersionSourceError::Network(error.to_string()))?;
        json::parse_manifest(&bytes)
    }

    async fn version(&self, id: &str) -> Result<VersionSpec, VersionSourceError> {
        let mut manifest = None;
        let mut chain = vec![self.load_version(id, &mut manifest).await?];
        while let Some(parent) = chain.last().and_then(|spec| spec.inherits_from.clone()) {
            if chain.len() > MAX_INHERITANCE_DEPTH {
                return Err(VersionSourceError::Invalid(format!(
                    "{id} inherits through more than {MAX_INHERITANCE_DEPTH} versions"
                )));
            }
            chain.push(self.load_version(&parent, &mut manifest).await?);
        }

        let mut merged = chain.pop();
        while let Some(child) = chain.pop() {
            merged = merged.map(|parent| child.merged_over(parent));
        }
        merged.ok_or_else(|| VersionSourceError::UnknownVersion(id.to_owned()))
    }

    async fn asset_index(&self, index: &AssetIndexRef) -> Result<AssetIndex, VersionSourceError> {
        let cache = index.artifact.path.under(&self.paths.assets_dir());
        let bytes = self
            .fetch_verified(
                &cache,
                &index.artifact.url,
                index.artifact.checksum.as_ref(),
                |error| VersionSourceError::Network(error.to_string()),
            )
            .await?;
        json::parse_asset_index(&index.id, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::launch::{Artifact, RelativePath};
    use crate::testing::FakeHttpClient;

    const MANIFEST: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/version_manifest_v2.json"
    ));
    const V26_2: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/version-26.2.json"
    ));
    const ASSETS_32: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/asset-index-32-trimmed.json"
    ));
    const V26_2_URL: &str = "https://piston-meta.mojang.com/v1/packages/91ef51b7d36ca092c5c02aba42f0f40c7cc1db84/26.2.json";
    const ASSETS_URL: &str = "https://piston-meta.example/indexes/32.json";
    const ASSETS_SHA1: &str = "294596ac14bddacef20a2aba749b73533499ac6d";

    fn online() -> FakeHttpClient {
        FakeHttpClient::with([
            (VERSION_MANIFEST_URL.to_owned(), MANIFEST.to_vec()),
            (V26_2_URL.to_owned(), V26_2.to_vec()),
            (ASSETS_URL.to_owned(), ASSETS_32.to_vec()),
        ])
    }

    fn offline() -> FakeHttpClient {
        FakeHttpClient::default()
    }

    fn source(http: FakeHttpClient, root: &Path) -> MojangVersionSource<FakeHttpClient> {
        MojangVersionSource::new(http, LauncherPaths::new(root))
    }

    fn write_cached(root: &Path, id: &str, json: &str) {
        let path = LauncherPaths::new(root).version_json(id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
    }

    fn asset_index_ref() -> AssetIndexRef {
        AssetIndexRef {
            id: "32".into(),
            artifact: Artifact {
                path: RelativePath::parse("indexes/32.json").unwrap(),
                url: ASSETS_URL.into(),
                checksum: Some(Checksum::parse(HashAlgorithm::Sha1, ASSETS_SHA1).unwrap()),
                size: None,
            },
        }
    }

    #[tokio::test]
    async fn fetches_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = source(online(), dir.path()).manifest().await.unwrap();
        assert_eq!(manifest.latest_release.as_str(), "26.3");
        assert!(matches!(
            source(offline(), dir.path()).manifest().await,
            Err(VersionSourceError::Network(_))
        ));
    }

    #[tokio::test]
    async fn downloads_and_caches_a_version_then_serves_it_offline() {
        let dir = tempfile::tempdir().unwrap();
        let first = source(online(), dir.path());
        let spec = first.version("26.2").await.unwrap();
        assert_eq!(spec.libraries.len(), 131);
        assert_eq!(first.http.requests(), [VERSION_MANIFEST_URL, V26_2_URL]);
        let cached = LauncherPaths::new(dir.path()).version_json("26.2").unwrap();
        assert_eq!(std::fs::read(&cached).unwrap(), V26_2);

        let second = source(offline(), dir.path());
        assert_eq!(second.version("26.2").await.unwrap(), spec);
        assert_eq!(second.http.requests(), [VERSION_MANIFEST_URL]);

        let third = source(online(), dir.path());
        assert_eq!(third.version("26.2").await.unwrap(), spec);
        assert_eq!(
            third.http.requests(),
            [VERSION_MANIFEST_URL],
            "a cache whose sha1 matches is reused"
        );
    }

    #[tokio::test]
    async fn re_downloads_a_corrupted_cache_when_online() {
        let dir = tempfile::tempdir().unwrap();
        write_cached(dir.path(), "26.2", "{\"id\": \"26.2\", \"tampered\": true}");
        let online = source(online(), dir.path());
        let spec = online.version("26.2").await.unwrap();
        assert_eq!(spec.main_class, "net.minecraft.client.main.Main");
        assert_eq!(online.http.requests(), [VERSION_MANIFEST_URL, V26_2_URL]);
        let cached = LauncherPaths::new(dir.path()).version_json("26.2").unwrap();
        assert_eq!(std::fs::read(cached).unwrap(), V26_2);
    }

    #[tokio::test]
    async fn rejects_a_download_whose_sha1_does_not_match() {
        let dir = tempfile::tempdir().unwrap();
        let http = FakeHttpClient::with([
            (VERSION_MANIFEST_URL.to_owned(), MANIFEST.to_vec()),
            (V26_2_URL.to_owned(), b"{\"id\": \"26.2\"}".to_vec()),
        ]);
        assert!(matches!(
            source(http, dir.path()).version("26.2").await,
            Err(VersionSourceError::Invalid(_))
        ));
        assert!(
            !LauncherPaths::new(dir.path())
                .version_json("26.2")
                .unwrap()
                .exists()
        );
    }

    #[tokio::test]
    async fn rejects_unsafe_ids_without_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let source = source(online(), dir.path());
        for id in ["../evil", "a/b", ""] {
            assert!(
                matches!(
                    source.version(id).await,
                    Err(VersionSourceError::Invalid(_))
                ),
                "{id}"
            );
        }
        assert!(source.http.requests().is_empty());
    }

    #[tokio::test]
    async fn reports_unknown_versions() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            source(online(), dir.path()).version("9.9").await,
            Err(VersionSourceError::UnknownVersion("9.9".into()))
        );
        assert_eq!(
            source(offline(), dir.path()).version("26.2").await,
            Err(VersionSourceError::UnknownVersion("26.2".into()))
        );
    }

    #[tokio::test]
    async fn merges_a_cached_loader_profile_over_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        write_cached(
            dir.path(),
            "fabric-test-26.2",
            r#"{
                "id": "fabric-test-26.2",
                "inheritsFrom": "26.2",
                "type": "release",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "arguments": {"game": [], "jvm": ["-Dfabric.test=1"]},
                "libraries": [{"name": "net.fabricmc:fabric-loader:0.17.2", "url": "https://maven.fabricmc.net/"}]
            }"#,
        );
        let spec = source(online(), dir.path())
            .version("fabric-test-26.2")
            .await
            .unwrap();
        assert_eq!(spec.id, "fabric-test-26.2");
        assert_eq!(
            spec.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(spec.libraries.len(), 132);
        assert_eq!(spec.libraries[0].name.artifact, "fabric-loader");
        assert_eq!(spec.java.map(|java| java.major), Some(25));
        assert_eq!(spec.inherits_from, None);
        assert!(spec.client_jar.is_some());
    }

    #[tokio::test]
    async fn rejects_inheritance_loops() {
        let dir = tempfile::tempdir().unwrap();
        write_cached(
            dir.path(),
            "loop-a",
            r#"{"id": "loop-a", "inheritsFrom": "loop-b"}"#,
        );
        write_cached(
            dir.path(),
            "loop-b",
            r#"{"id": "loop-b", "inheritsFrom": "loop-a"}"#,
        );
        assert!(matches!(
            source(offline(), dir.path()).version("loop-a").await,
            Err(VersionSourceError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn caches_asset_indexes_and_reuses_them_offline() {
        let dir = tempfile::tempdir().unwrap();
        let first = source(online(), dir.path());
        let index = first.asset_index(&asset_index_ref()).await.unwrap();
        assert_eq!(index.id, "32");
        assert_eq!(index.objects.len(), 6);
        assert_eq!(first.http.requests(), [ASSETS_URL]);
        let cached = LauncherPaths::new(dir.path())
            .assets_dir()
            .join("indexes")
            .join("32.json");
        assert_eq!(std::fs::read(&cached).unwrap(), ASSETS_32);

        let second = source(offline(), dir.path());
        assert_eq!(second.asset_index(&asset_index_ref()).await.unwrap(), index);
        assert!(second.http.requests().is_empty());

        std::fs::write(&cached, b"corrupt").unwrap();
        assert!(matches!(
            second.asset_index(&asset_index_ref()).await,
            Err(VersionSourceError::Network(_))
        ));
        let third = source(online(), dir.path());
        assert_eq!(third.asset_index(&asset_index_ref()).await.unwrap(), index);
    }

    #[tokio::test]
    async fn a_malformed_manifest_sha1_fails_instead_of_skipping_verification() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = String::from_utf8(MANIFEST.to_vec()).unwrap().replace(
            "91ef51b7d36ca092c5c02aba42f0f40c7cc1db84\",",
            "not-a-sha1\",",
        );
        assert!(
            manifest.contains("not-a-sha1"),
            "the fixture entry for 26.2 was patched"
        );
        let http = FakeHttpClient::with([
            (VERSION_MANIFEST_URL.to_owned(), manifest.into_bytes()),
            (V26_2_URL.to_owned(), V26_2.to_vec()),
        ]);
        let source = source(http, dir.path());

        assert!(matches!(
            source.version("26.2").await,
            Err(VersionSourceError::Invalid(_))
        ));
        let cached = LauncherPaths::new(dir.path()).version_json("26.2").unwrap();
        assert!(!cached.exists(), "nothing unverified is cached");
    }
}
