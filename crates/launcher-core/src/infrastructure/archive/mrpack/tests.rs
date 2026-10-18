#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

use super::*;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::mrpack::{EnvSupport, MrpackError};
use crate::testing::sha1_hex;

const INDEX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/modrinth/modrinth.index.json"
));

enum Entry<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Link(&'a str, &'a str),
}

fn zip_bytes(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for entry in entries {
        match entry {
            Entry::File(name, body) => {
                writer.start_file(*name, options).unwrap();
                writer.write_all(body).unwrap();
            }
            Entry::Dir(name) => writer.add_directory(*name, options).unwrap(),
            Entry::Link(name, target) => writer.add_symlink(*name, *target, options).unwrap(),
        }
    }
    writer.finish().unwrap().into_inner()
}

/// A temp root holding the archive at `pack.mrpack` and a `staging/` folder;
/// an escaping entry would land in the root.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn with(entries: &[Entry<'_>]) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("pack.mrpack"), zip_bytes(entries)).unwrap();
        Self { root }
    }

    fn archive(&self) -> PathBuf {
        self.root.path().join("pack.mrpack")
    }

    fn staging(&self) -> PathBuf {
        self.root.path().join("staging")
    }

    /// Every file under the root except the archive, relative, sorted.
    fn files(&self) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                    walk(&path, root, out);
                } else {
                    let relative = path.strip_prefix(root).unwrap();
                    out.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(self.root.path(), self.root.path(), &mut out);
        out.retain(|path| path != "pack.mrpack");
        out.sort();
        out
    }
}

fn archive() -> ZipMrpackArchive {
    ZipMrpackArchive::new()
}

#[tokio::test]
async fn reads_the_fabulously_optimized_index() {
    let sandbox = Sandbox::with(&[
        Entry::File("modrinth.index.json", INDEX),
        Entry::File("overrides/options.txt", b"x"),
    ]);
    let index = archive().read_index(&sandbox.archive()).await.unwrap();
    assert_eq!(index.name, "Fabulously Optimized");
    assert_eq!(index.version_id, "14.1.1");
    assert_eq!(index.summary, None);
    assert_eq!(index.game_version.as_str(), "26.2");
    assert_eq!(index.loader.kind(), LoaderKind::Fabric);
    assert_eq!(index.loader.version().unwrap().as_str(), "0.19.5");
    assert_eq!(index.files.len(), 51);
    assert_eq!(index.client_files().count(), 51);
    let first = &index.files[0];
    assert_eq!(
        first.path.as_str(),
        "mods/BetterGrassify-1.8.7+fabric.26.2.jar"
    );
    assert_eq!(
        first.sha1.value(),
        "0f4a890d07402280686a518579fa9fa02309e315"
    );
    assert!(first.sha512.value().starts_with("7b70e796cea2ee57"));
    assert_eq!(first.size, 88_858);
    assert_eq!(
        first.downloads,
        [
            "https://cdn.modrinth.com/data/m5T5xmUy/versions/r4yqxYQl/BetterGrassify-1.8.7%2Bfabric.26.2.jar"
        ]
    );
    assert_eq!(
        (first.client, first.server),
        (EnvSupport::Required, EnvSupport::Required)
    );
}

fn synthetic_file(path: &str, env: Value) -> Value {
    let mut file = json!({
        "path": path,
        "hashes": {
            "sha1": "0f4a890d07402280686a518579fa9fa02309e315",
            "sha512": "a".repeat(128),
        },
        "downloads": [format!("https://cdn.modrinth.com/data/x/versions/y/{path}")],
        "fileSize": 3,
        "unknownField": true,
    });
    if !env.is_null() {
        file["env"] = env;
    }
    file
}

fn synthetic_index(files: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "1.0.0",
        "name": "Sides",
        "summary": "A pack",
        "files": files,
        "dependencies": { "minecraft": "26.2", "quilt-loader": "0.30.0" },
    }))
    .unwrap()
}

#[tokio::test]
async fn reads_env_sides_and_filters_server_only_files() {
    let index = synthetic_index(vec![
        synthetic_file(
            "mods/both.jar",
            json!({ "client": "required", "server": "required" }),
        ),
        synthetic_file(
            "mods/server.jar",
            json!({ "client": "unsupported", "server": "required" }),
        ),
        synthetic_file(
            "mods/optional.jar",
            json!({ "client": "optional", "server": "unsupported" }),
        ),
        synthetic_file("mods/no-env.jar", Value::Null),
    ]);
    let sandbox = Sandbox::with(&[Entry::File("modrinth.index.json", &index)]);
    let index = archive().read_index(&sandbox.archive()).await.unwrap();
    assert_eq!(index.summary.as_deref(), Some("A pack"));
    assert_eq!(index.loader.kind(), LoaderKind::Quilt);
    let client: Vec<&str> = index
        .client_files()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        client,
        ["mods/both.jar", "mods/optional.jar", "mods/no-env.jar"]
    );
    assert_eq!(index.files[2].server, EnvSupport::Unsupported);
}

#[tokio::test]
async fn archives_without_a_readable_index_are_invalid() {
    let missing = Sandbox::with(&[Entry::File("overrides/a.txt", b"a")]);
    let not_json = Sandbox::with(&[Entry::File("modrinth.index.json", b"{ nope")]);
    for sandbox in [&missing, &not_json] {
        let result = archive().read_index(&sandbox.archive()).await;
        assert!(
            matches!(result, Err(PackFileError::InvalidArchive(_))),
            "{result:?}"
        );
    }
    let not_zip = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(not_zip.path(), b"plain text").unwrap();
    let result = archive().read_index(not_zip.path()).await;
    assert!(
        matches!(result, Err(PackFileError::InvalidArchive(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn an_oversized_index_is_refused() {
    let mut big = synthetic_index(vec![]);
    big.pop();
    big.extend_from_slice(b",\"padding\":\"");
    big.resize(MAX_INDEX_BYTES as usize + 16, b'x');
    big.extend_from_slice(b"\"}");
    let sandbox = Sandbox::with(&[Entry::File("modrinth.index.json", &big)]);
    let result = archive().read_index(&sandbox.archive()).await;
    assert!(
        matches!(result, Err(PackFileError::InvalidArchive(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn index_validation_errors_are_reported_as_such() {
    let index = synthetic_index(vec![synthetic_file("../evil.jar", Value::Null)]);
    let sandbox = Sandbox::with(&[Entry::File("modrinth.index.json", &index)]);
    assert_eq!(
        archive().read_index(&sandbox.archive()).await,
        Err(PackFileError::Index(MrpackError::UnsafePath(
            "../evil.jar".into()
        )))
    );
}

fn staged_paths(staged: &[StagedFile]) -> Vec<&str> {
    staged.iter().map(|file| file.path.as_str()).collect()
}

#[tokio::test]
async fn extracts_overrides_then_client_overrides_only() {
    let sandbox = Sandbox::with(&[
        // The client override comes first in the archive and still wins.
        Entry::File("client-overrides/config/shared.toml", b"client"),
        Entry::Dir("overrides/"),
        Entry::Dir("overrides/config/"),
        Entry::File("overrides/config/shared.toml", b"common"),
        Entry::File("overrides/options.txt", b"fov:90"),
        Entry::File("server-overrides/server.properties", b"motd=x"),
        Entry::File("modrinth.index.json", INDEX),
        Entry::File("README.md", b"readme"),
    ]);
    let staged = archive()
        .extract_overrides(&sandbox.archive(), &sandbox.staging())
        .await
        .unwrap();
    assert_eq!(staged_paths(&staged), ["config/shared.toml", "options.txt"]);
    let shared = &staged[0];
    assert_eq!(shared.sha1, sha1_hex(b"client"));
    assert_eq!(std::fs::read(&shared.staged).unwrap(), b"client");
    assert!(
        shared
            .staged
            .starts_with(sandbox.staging().canonicalize().unwrap())
    );
    assert_eq!(staged[1].sha1, sha1_hex(b"fov:90"));
    assert_eq!(
        sandbox.files(),
        ["staging/config/shared.toml", "staging/options.txt"]
    );
}

#[tokio::test]
async fn entries_that_escape_fail_the_whole_extraction() {
    for name in [
        "overrides/../evil",
        "overrides//abs",
        "/overrides/abs",
        "overrides\\..\\evil",
        "overrides\\evil",
        "C:/overrides/evil",
        "client-overrides/../../evil",
        "server-overrides/../evil",
        "../evil",
    ] {
        let sandbox = Sandbox::with(&[
            Entry::File("overrides/fine.txt", b"fine"),
            Entry::File(name, b"evil"),
        ]);
        let result = archive()
            .extract_overrides(&sandbox.archive(), &sandbox.staging())
            .await;
        assert!(
            matches!(result, Err(PackFileError::UnsafeEntry(_))),
            "{name}: {result:?}"
        );
        assert!(
            sandbox.files().is_empty(),
            "{name}: nothing is written, got {:?}",
            sandbox.files()
        );
    }
}

#[tokio::test]
async fn symlink_entries_are_skipped() {
    let sandbox = Sandbox::with(&[
        Entry::Link("overrides/link", "../../outside"),
        Entry::File("overrides/a.txt", b"a"),
    ]);
    let staged = archive()
        .extract_overrides(&sandbox.archive(), &sandbox.staging())
        .await
        .unwrap();
    assert_eq!(staged_paths(&staged), ["a.txt"]);
    assert_eq!(sandbox.files(), ["staging/a.txt"]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_already_in_staging_cannot_redirect_writes() {
    let sandbox = Sandbox::with(&[Entry::File("overrides/config/a.toml", b"evil")]);
    let outside = sandbox.root.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(sandbox.staging()).unwrap();
    std::os::unix::fs::symlink(&outside, sandbox.staging().join("config")).unwrap();
    let result = archive()
        .extract_overrides(&sandbox.archive(), &sandbox.staging())
        .await;
    assert!(
        matches!(result, Err(PackFileError::UnsafeEntry(_))),
        "{result:?}"
    );
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

#[tokio::test]
async fn entries_and_packs_over_the_size_caps_are_refused() {
    let limited = ZipMrpackArchive::with_limits(1_000, 2_500);
    let big = vec![b'z'; 1_001];
    let one_big = Sandbox::with(&[Entry::File("overrides/big.bin", &big)]);
    let result = limited
        .extract_overrides(&one_big.archive(), &one_big.staging())
        .await;
    assert!(
        matches!(result, Err(PackFileError::InvalidArchive(_))),
        "{result:?}"
    );

    let part = vec![b'z'; 900];
    let many = Sandbox::with(&[
        Entry::File("overrides/a.bin", &part),
        Entry::File("overrides/b.bin", &part),
        Entry::File("client-overrides/c.bin", &part),
    ]);
    let result = limited
        .extract_overrides(&many.archive(), &many.staging())
        .await;
    assert!(
        matches!(result, Err(PackFileError::InvalidArchive(_))),
        "{result:?}"
    );

    let fits = Sandbox::with(&[
        Entry::File("overrides/a.bin", &part),
        Entry::File("overrides/b.bin", &part),
    ]);
    let staged = limited
        .extract_overrides(&fits.archive(), &fits.staging())
        .await
        .unwrap();
    assert_eq!(staged.len(), 2);
}

#[tokio::test]
async fn written_packs_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let source = Sandbox::with(&[Entry::File("modrinth.index.json", INDEX)]);
    let index = archive().read_index(&source.archive()).await.unwrap();
    let options = dir.path().join("options.txt");
    let config = dir.path().join("custom.toml");
    std::fs::write(&options, b"fov:90").unwrap();
    std::fs::write(&config, b"key = 1").unwrap();
    let overrides = vec![
        (RelativePath::parse("options.txt").unwrap(), options),
        (RelativePath::parse("config/custom.toml").unwrap(), config),
    ];
    let out = dir.path().join("exports");
    let target = out.join("pack.mrpack");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(&target, b"an older export").unwrap();

    archive().write(&target, &index, &overrides).await.unwrap();

    assert_eq!(archive().read_index(&target).await.unwrap(), index);
    let staging = dir.path().join("staging");
    let staged = archive()
        .extract_overrides(&target, &staging)
        .await
        .unwrap();
    assert_eq!(staged_paths(&staged), ["config/custom.toml", "options.txt"]);
    assert_eq!(staged[1].sha1, sha1_hex(b"fov:90"));
    let names: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["pack.mrpack"], "no temporary file is left behind");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
    let mut raw = String::new();
    zip.by_name("modrinth.index.json")
        .unwrap()
        .read_to_string(&mut raw)
        .unwrap();
    assert!(
        raw.contains("\n  \"formatVersion\": 1"),
        "pretty camelCase: {raw}"
    );
    assert!(raw.contains("\"fileSize\": 88858"));
    assert_eq!(
        zip.by_name("overrides/options.txt").unwrap().compression(),
        zip::CompressionMethod::Deflated
    );
}

#[tokio::test]
async fn a_missing_override_source_fails_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let source = Sandbox::with(&[Entry::File("modrinth.index.json", INDEX)]);
    let index = archive().read_index(&source.archive()).await.unwrap();
    let target = dir.path().join("pack.mrpack");
    let result = archive()
        .write(
            &target,
            &index,
            &[(
                RelativePath::parse("options.txt").unwrap(),
                dir.path().join("missing"),
            )],
        )
        .await;
    assert!(
        matches!(result, Err(PackFileError::Io { .. })),
        "{result:?}"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}
