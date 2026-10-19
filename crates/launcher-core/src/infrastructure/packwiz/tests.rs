#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use sha2::Digest;

use super::wire::{parse_index, parse_metafile, parse_pack};
use super::*;
use crate::domain::integrity::curseforge_fingerprint;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::packwiz::{Metafile, MetafileSource, PackwizIndex, PackwizPack};
use crate::testing::FakeHttpClient;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packwiz")
}

fn fo() -> PathBuf {
    fixtures().join("fabulously-optimized")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn path(raw: &str) -> RelativePath {
    RelativePath::parse(raw).unwrap()
}

const FO_METAFILES: [&str; 4] = ["fabric-api", "sodium", "lithium", "modmenu"];

fn local() -> PackwizFiles<FakeHttpClient> {
    PackwizFiles::new(FakeHttpClient::default())
}

fn sha256(bytes: &[u8]) -> Checksum {
    Checksum::parse(
        HashAlgorithm::Sha256,
        &hex::encode(sha2::Sha256::digest(bytes)),
    )
    .unwrap()
}

#[test]
fn parses_the_fabulously_optimized_pack() {
    let raw = parse_pack("pack.toml", &read(&fo().join("pack.toml"))).unwrap();
    assert_eq!(raw.name, "Fabulously Optimized");
    assert_eq!(raw.version.as_deref(), Some("14.1.1"));
    assert_eq!(raw.pack_format.as_deref(), Some("packwiz:1.1.0"));
    assert_eq!(raw.index_file, "index.toml");
    let pack = PackwizPack::validate(raw).unwrap();
    assert_eq!(pack.game_version.as_str(), "26.2");
    assert_eq!(pack.loader.kind(), LoaderKind::Fabric);
    assert_eq!(pack.loader.version().unwrap().as_str(), "0.19.5");
    assert_eq!(
        pack.index_checksum.value(),
        "e08daa5b3e91b3fe3c98b3f9c279602f87e81c619c2741384939ac578671b025"
    );
}

#[test]
fn parses_the_real_index() {
    let raw = parse_index("index.toml", &read(&fo().join("index.toml"))).unwrap();
    assert_eq!(raw.hash_format, "sha256");
    assert_eq!(raw.files.len(), 108);
    let index = PackwizIndex::validate(raw).unwrap();
    assert_eq!(index.entries.iter().filter(|e| e.metafile).count(), 51);
    assert!(
        index
            .entries
            .iter()
            .all(|e| e.checksum.algorithm() == HashAlgorithm::Sha256)
    );
}

#[test]
fn parses_every_metafile_fixture() {
    for name in FO_METAFILES {
        let relative = path(&format!("mods/{name}.pw.toml"));
        let raw = parse_metafile(relative.as_str(), &read(&relative.under(&fo()))).unwrap();
        assert_eq!(raw.side.as_deref(), Some("both"), "{name}");
        assert_eq!(raw.hash_format, "sha512", "{name}");
        assert_eq!(raw.mode, None, "{name}");
        let file = Metafile::validate(raw, &relative).unwrap().unwrap();
        assert!(file.path.as_str().starts_with("mods/"), "{name}");
        assert!(file.path.as_str().ends_with(".jar"), "{name}");
        let MetafileSource::Download { url } = file.source else {
            panic!("{name} should download from a URL");
        };
        assert!(
            url.starts_with("https://cdn.modrinth.com/"),
            "{name}: {url}"
        );
    }

    let jei = parse_metafile(
        "jei.pw.toml",
        &read(&fixtures().join("curseforge/jei.pw.toml")),
    )
    .unwrap();
    assert_eq!(jei.mode.as_deref(), Some("metadata:curseforge"));
    assert_eq!(jei.curseforge, Some((238_222, 8_367_520)));
    assert_eq!(jei.url, None);
    let file = Metafile::validate(jei, &path("mods/jei.pw.toml"))
        .unwrap()
        .unwrap();
    let MetafileSource::CurseForge(manual) = file.source else {
        panic!("jei should be a manual download");
    };
    assert_eq!(
        manual.path.as_str(),
        "mods/jei-1.20.1-forge-15.20.0.133.jar"
    );
    assert_eq!(manual.checksum.algorithm(), HashAlgorithm::Sha1);
    assert_eq!(
        manual.checksum.value(),
        "c23406dbad87f9a7563c97ff3d5195311ab0fed6"
    );
    assert_eq!((manual.project_id, manual.file_id), (238_222, 8_367_520));
}

#[test]
fn server_only_metafiles_are_filtered_out() {
    let body = br#"
name = "Server Thing"
filename = "server-thing.jar"
side = "server"
future-key = 1

[download]
url = "https://cdn.modrinth.com/data/x/server-thing.jar"
hash-format = "sha1"
hash = "c23406dbad87f9a7563c97ff3d5195311ab0fed6"
"#;
    let raw = parse_metafile("server.pw.toml", body).unwrap();
    assert_eq!(
        Metafile::validate(raw, &path("mods/server.pw.toml")),
        Ok(None)
    );
}

#[test]
fn bad_toml_names_the_file() {
    for (name, result) in [
        ("pack.toml", parse_pack("pack.toml", b"name = ").map(|_| ())),
        (
            "index.toml",
            parse_index("index.toml", b"[[files]]\nfile = 1").map(|_| ()),
        ),
        (
            "a.pw.toml",
            parse_metafile("a.pw.toml", b"name = \"x\"").map(|_| ()),
        ),
        (
            "b.pw.toml",
            parse_metafile("b.pw.toml", &[0xff, 0xfe]).map(|_| ()),
        ),
    ] {
        assert!(
            matches!(&result, Err(PackFileError::Source(reason)) if reason.contains(name)),
            "{name}: {result:?}"
        );
    }
}

#[tokio::test]
async fn reads_a_local_pack_and_verifies_its_index_and_metafiles() {
    let source = local();
    let location = PackLocation::Local(fo().join("pack.toml"));
    let pack = PackwizPack::validate(source.read_pack(&location).await.unwrap()).unwrap();
    let raw = source
        .read_index(&location, &pack.index, &pack.index_checksum)
        .await
        .unwrap();
    let index = PackwizIndex::validate(raw).unwrap();
    for name in FO_METAFILES {
        let relative = path(&format!("mods/{name}.pw.toml"));
        let entry = index.entries.iter().find(|e| e.path == relative).unwrap();
        let raw = source
            .read_metafile(&location, &relative, &entry.checksum)
            .await
            .unwrap();
        assert!(Metafile::validate(raw, &relative).unwrap().is_some());
    }
    // A folder works as well as the pack.toml inside it.
    let folder = PackLocation::Local(fo());
    assert_eq!(
        source.read_pack(&folder).await.unwrap(),
        source.read_pack(&location).await.unwrap()
    );
}

#[tokio::test]
async fn reads_a_remote_pack_relative_to_its_folder() {
    let base = "https://example.com/packs/my%20pack";
    let pack_bytes = read(&fo().join("pack.toml"));
    let index_bytes = read(&fo().join("index.toml"));
    let meta_bytes = read(&fo().join("mods/sodium.pw.toml"));
    let http = FakeHttpClient::with([
        (format!("{base}/pack.toml?token=abc"), pack_bytes),
        (format!("{base}/index.toml"), index_bytes.clone()),
        (format!("{base}/mods/sodium.pw.toml"), meta_bytes.clone()),
    ]);
    let source = PackwizFiles::new(http);
    let location = PackLocation::Url(format!("{base}/pack.toml?token=abc"));
    let pack = PackwizPack::validate(source.read_pack(&location).await.unwrap()).unwrap();
    let index = source
        .read_index(&location, &pack.index, &pack.index_checksum)
        .await
        .unwrap();
    assert_eq!(index.files.len(), 108);
    let meta = source
        .read_metafile(
            &location,
            &path("mods/sodium.pw.toml"),
            &sha256(&meta_bytes),
        )
        .await
        .unwrap();
    assert_eq!(meta.name, "Sodium");
    assert_eq!(
        source.http.requests(),
        [
            format!("{base}/pack.toml?token=abc"),
            format!("{base}/index.toml"),
            format!("{base}/mods/sodium.pw.toml"),
        ]
    );
}

#[test]
fn urls_are_joined_segment_by_segment() {
    let source = local();
    let locate = |pack: &str, relative: &str| {
        source.locate(&PackLocation::Url(pack.into()), &path(relative))
    };
    assert_eq!(
        locate(
            "https://example.com/my pack/pack.toml?token=abc#top",
            "config/my file.txt"
        ),
        Ok(PackFileLocation::Url(
            "https://example.com/my%20pack/config/my%20file.txt".into()
        ))
    );
    assert_eq!(
        locate("https://example.com/pack.toml", "mods/a?b#c%20.jar"),
        Ok(PackFileLocation::Url(
            "https://example.com/mods/a%3Fb%23c%2520.jar".into()
        ))
    );
    assert_eq!(
        locate("https://example.com/a/b/pack.toml", "options.txt"),
        Ok(PackFileLocation::Url(
            "https://example.com/a/b/options.txt".into()
        ))
    );
    for bad in [
        "http://example.com/pack.toml",
        "https://",
        "https://exa mple.com/pack.toml",
    ] {
        assert!(
            matches!(locate(bad, "options.txt"), Err(PackFileError::Source(_))),
            "{bad}"
        );
    }
}

#[test]
fn local_files_are_located_inside_the_pack_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pack/config")).unwrap();
    std::fs::write(dir.path().join("pack/pack.toml"), b"").unwrap();
    std::fs::write(dir.path().join("pack/config/a.txt"), b"a").unwrap();
    let location = PackLocation::Local(dir.path().join("pack/pack.toml"));
    let canonical = dir.path().join("pack").canonicalize().unwrap();
    assert_eq!(
        local().locate(&location, &path("config/a.txt")),
        Ok(PackFileLocation::Path(canonical.join("config/a.txt")))
    );
    // A missing file still resolves inside the folder.
    assert_eq!(
        local().locate(&location, &path("config/new/b.txt")),
        Ok(PackFileLocation::Path(canonical.join("config/new/b.txt")))
    );
    assert!(RelativePath::parse("../outside.txt").is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn symbolic_links_cannot_escape_a_local_pack() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pack/config")).unwrap();
    std::fs::create_dir_all(dir.path().join("outside")).unwrap();
    std::fs::write(dir.path().join("outside/secret.txt"), b"secret").unwrap();
    std::fs::write(dir.path().join("pack/pack.toml"), b"").unwrap();
    std::fs::write(dir.path().join("pack/config/a.txt"), b"a").unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("pack/escape")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("outside/secret.txt"),
        dir.path().join("pack/secret.txt"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("pack/config/a.txt"),
        dir.path().join("pack/inside.txt"),
    )
    .unwrap();
    let location = PackLocation::Local(dir.path().join("pack/pack.toml"));
    let source = local();
    for escape in ["escape/secret.txt", "secret.txt", "escape/missing/x.txt"] {
        assert!(
            matches!(
                source.locate(&location, &path(escape)),
                Err(PackFileError::Packwiz(PackwizError::UnsafePath(_)))
            ),
            "{escape}"
        );
    }
    let secret = sha256(b"secret");
    assert!(matches!(
        source
            .read_metafile(&location, &path("escape/secret.txt"), &secret)
            .await,
        Err(PackFileError::Packwiz(PackwizError::UnsafePath(_)))
    ));
    let canonical = dir.path().join("pack").canonicalize().unwrap();
    assert_eq!(
        source.locate(&location, &path("inside.txt")),
        Ok(PackFileLocation::Path(canonical.join("config/a.txt")))
    );
}

#[tokio::test]
async fn checksum_mismatches_are_rejected_in_every_algorithm() {
    let body = read(&fo().join("mods/sodium.pw.toml"));
    let http = FakeHttpClient::with([(
        "https://example.com/mods/sodium.pw.toml".to_owned(),
        body.clone(),
    )]);
    let source = PackwizFiles::new(http);
    let location = PackLocation::Url("https://example.com/pack.toml".into());
    let relative = path("mods/sodium.pw.toml");
    let right = [
        Checksum::parse(HashAlgorithm::Sha1, &hex::encode(sha1::Sha1::digest(&body))).unwrap(),
        sha256(&body),
        Checksum::parse(
            HashAlgorithm::Sha512,
            &hex::encode(sha2::Sha512::digest(&body)),
        )
        .unwrap(),
        Checksum::parse(
            HashAlgorithm::Murmur2,
            &curseforge_fingerprint(&body).to_string(),
        )
        .unwrap(),
    ];
    for checksum in right {
        assert!(
            source
                .read_metafile(&location, &relative, &checksum)
                .await
                .is_ok(),
            "{checksum}"
        );
    }
    let wrong = [
        Checksum::parse(HashAlgorithm::Sha1, &"0".repeat(40)).unwrap(),
        sha256(b"something else"),
        Checksum::parse(HashAlgorithm::Sha512, &"0".repeat(128)).unwrap(),
        Checksum::parse(
            HashAlgorithm::Murmur2,
            &curseforge_fingerprint(b"x").to_string(),
        )
        .unwrap(),
    ];
    for checksum in wrong {
        let result = source.read_metafile(&location, &relative, &checksum).await;
        assert!(
            matches!(&result, Err(PackFileError::Source(reason)) if reason.contains("mods/sodium.pw.toml") && reason.contains("hash")),
            "{checksum}: {result:?}"
        );
    }
    let index = source
        .read_index(&location, &relative, &sha256(b"other"))
        .await;
    assert!(matches!(index, Err(PackFileError::Source(_))), "{index:?}");
}

#[tokio::test]
async fn oversized_files_are_refused() {
    let big_meta = vec![b' '; METAFILE_LIMIT + 1];
    let big_index = vec![b' '; INDEX_LIMIT + 1];
    let http = FakeHttpClient::with([
        ("https://example.com/pack.toml".to_owned(), big_meta.clone()),
        ("https://example.com/a.pw.toml".to_owned(), big_meta.clone()),
        (
            "https://example.com/index.toml".to_owned(),
            big_index.clone(),
        ),
    ]);
    let source = PackwizFiles::new(http);
    let location = PackLocation::Url("https://example.com/pack.toml".into());
    assert!(matches!(
        source.read_pack(&location).await,
        Err(PackFileError::TooLarge(_))
    ));
    assert!(matches!(
        source
            .read_metafile(&location, &path("a.pw.toml"), &sha256(&big_meta))
            .await,
        Err(PackFileError::TooLarge(_))
    ));
    assert!(matches!(
        source
            .read_index(&location, &path("index.toml"), &sha256(&big_index))
            .await,
        Err(PackFileError::TooLarge(_))
    ));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pack.toml"), &big_meta).unwrap();
    std::fs::write(dir.path().join("index.toml"), &big_index).unwrap();
    let local_pack = PackLocation::Local(dir.path().join("pack.toml"));
    assert!(matches!(
        local().read_pack(&local_pack).await,
        Err(PackFileError::TooLarge(_))
    ));
    assert!(matches!(
        local()
            .read_index(&local_pack, &path("index.toml"), &sha256(&big_index))
            .await,
        Err(PackFileError::TooLarge(_))
    ));
}

#[tokio::test]
async fn http_and_local_failures_are_errors() {
    let source = local();
    let remote = PackLocation::Url("https://example.com/pack.toml".into());
    assert!(matches!(
        source.read_pack(&remote).await,
        Err(PackFileError::Source(_))
    ));
    let dir = tempfile::tempdir().unwrap();
    let missing = PackLocation::Local(dir.path().join("pack.toml"));
    assert!(matches!(
        source.read_pack(&missing).await,
        Err(PackFileError::Io { .. })
    ));
}
