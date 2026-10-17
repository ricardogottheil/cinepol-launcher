#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::domain::integrity::HashAlgorithm;
use crate::domain::java::{JavaRequirement, JavaRuntimeError, JavaRuntimeProvider, JavaVersion};
use crate::domain::launch::{Arch, OsName, Platform};
use crate::testing::{FakeDownloader, FakeHttpClient, FakeJavaProbe, RecordingReporter};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/v1/packages/5643d63e75d54da43c9501cdb1988530a9bc5d7f/manifest.json";
const ADOPT_URL: &str = "https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=aarch64&image_type=jre&os=linux&vendor=eclipse";
const ADOPT_JDK_URL: &str = "https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=aarch64&image_type=jdk&os=linux&vendor=eclipse";
const LINK: &str = "https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.4.1%2B1/OpenJDK25U-jre_aarch64_linux_hotspot_25.0.4.1_1.tar.gz";

fn fixture(relative: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(relative),
    )
    .unwrap()
}

fn http_for(manifest: &[u8]) -> FakeHttpClient {
    let mut index: serde_json::Value =
        serde_json::from_slice(&fixture("mojang/java-runtime-all-trimmed.json")).unwrap();
    index["mac-os-arm64"]["java-runtime-epsilon"][0]["manifest"]["sha1"] =
        hex::encode(Sha1::digest(manifest)).into();
    FakeHttpClient::with([
        (
            MOJANG_RUNTIME_INDEX_URL.to_owned(),
            serde_json::to_vec(&index).unwrap(),
        ),
        (MANIFEST_URL.to_owned(), manifest.to_vec()),
    ])
}

fn req() -> JavaRequirement {
    JavaRequirement {
        component: "java-runtime-epsilon".into(),
        major: 25,
    }
}

fn java25() -> JavaVersion {
    JavaVersion {
        major: 25,
        full: "25.0.1".into(),
        is_64_bit: true,
    }
}

async fn run<
    H: crate::domain::ports::HttpClient,
    D: crate::domain::ports::Downloader,
    P: crate::domain::java::JavaProbe,
>(
    p: &ManagedJavaRuntimes<H, D, P>,
    platform: &Platform,
) -> Result<crate::domain::java::JavaRuntime, JavaRuntimeError> {
    p.ensure(
        &req(),
        platform,
        &RecordingReporter::default(),
        &CancellationToken::new(),
    )
    .await
}

/// sandbox/runtimes; anything outside runtimes/ in sandbox is an escape.
fn escapes(sandbox: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(sandbox).unwrap().flatten() {
        if e.file_name() != "runtimes" {
            out.push(e.path());
        }
    }
    out
}

fn file(key: &str, sha1: &str, exe: bool) -> String {
    format!(
        r#""{key}": {{"type":"file","executable":{exe},"downloads":{{"raw":{{"sha1":"{sha1}","size":2,"url":"https://x/{key}"}}}}}}"#
    )
}

const JAVA: &str = "jre.bundle/Contents/Home/bin/java";

#[tokio::test]
async fn mojang_key_probes() {
    for key in [
        "a/../../x",
        "/abs",
        "bin/./x",
        "a//b",
        "C:/x",
        "a\\\\..\\\\x",
        "..",
    ] {
        let sb = tempfile::tempdir().unwrap();
        let rt = sb.path().join("runtimes");
        let m = format!("{{\"files\":{{{}}}}}", file(key, &"a".repeat(40), false));
        let p = ManagedJavaRuntimes::new(
            http_for(m.as_bytes()),
            FakeDownloader::with([]),
            FakeJavaProbe::default(),
            rt,
        );
        let r = run(&p, &Platform::new(OsName::MacOs, Arch::Arm64)).await;
        eprintln!(
            "PROBE mojang key {key:?}: {:?} downloads={} escapes={:?}",
            r.as_ref().err(),
            p.downloader.requests().len(),
            escapes(sb.path())
        );
        assert!(matches!(r, Err(JavaRuntimeError::Invalid(_))));
        assert!(p.downloader.requests().is_empty());
        assert!(escapes(sb.path()).is_empty());
    }
}

#[tokio::test]
async fn mojang_link_and_sha_probes() {
    let platform = Platform::new(OsName::MacOs, Arch::Arm64);
    let sha = "a".repeat(40);
    let cases = [
        (
            "link escaping",
            format!(
                r#"{{"files":{{{}, "esc": {{"type":"link","target":"../../../../../../outside"}}}}}}"#,
                file(JAVA, &sha, true)
            ),
        ),
        (
            "link absolute",
            format!(
                r#"{{"files":{{{}, "esc": {{"type":"link","target":"/etc"}}}}}}"#,
                file(JAVA, &sha, true)
            ),
        ),
        (
            "link mid ..",
            format!(
                r#"{{"files":{{{}, "jre.bundle/esc": {{"type":"link","target":"Contents/../../.."}}}}}}"#,
                file(JAVA, &sha, true)
            ),
        ),
        (
            "missing sha1",
            format!(
                r#"{{"files":{{"{JAVA}": {{"type":"file","executable":true,"downloads":{{"raw":{{"size":2,"url":"https://x/j"}}}}}}}}}}"#
            ),
        ),
        (
            "empty sha1",
            format!(r#"{{"files":{{{}}}}}"#, file(JAVA, "", true)),
        ),
        (
            "bad sha1",
            format!(r#"{{"files":{{{}}}}}"#, file(JAVA, "zz", true)),
        ),
    ];
    for (label, m) in cases {
        let sb = tempfile::tempdir().unwrap();
        let rt = sb.path().join("runtimes");
        let p = ManagedJavaRuntimes::new(
            http_for(m.as_bytes()),
            FakeDownloader::with([]),
            FakeJavaProbe::with([(
                rt.join("mojang/java-runtime-epsilon/mac-os-arm64")
                    .join(JAVA),
                java25(),
            )]),
            rt,
        );
        let r = run(&p, &platform).await;
        eprintln!(
            "PROBE mojang {label}: {:?} escapes={:?}",
            r.as_ref().err(),
            escapes(sb.path())
        );
        assert!(r.is_err(), "{label}");
        assert!(escapes(sb.path()).is_empty());
    }
}

#[tokio::test]
async fn mojang_file_through_previous_link_and_exec_and_checksum() {
    use std::os::unix::fs::PermissionsExt;
    let platform = Platform::new(OsName::MacOs, Arch::Arm64);
    let sb = tempfile::tempdir().unwrap();
    let rt = sb.path().join("runtimes");
    let dir = rt.join("mojang/java-runtime-epsilon/mac-os-arm64");
    let sha = hex::encode(Sha1::digest(b"ok"));
    let m1 = format!(
        r#"{{"files":{{{}, "up": {{"type":"link","target":"jre.bundle"}}, "jre.bundle/Contents/MacOS/lib": {{"type":"link","target":"../Home/bin/java"}}}}}}"#,
        file(JAVA, &sha, true)
    );
    let p1 = ManagedJavaRuntimes::new(
        http_for(m1.as_bytes()),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(dir.join(JAVA), java25())]),
        rt.clone(),
    );
    let r = run(&p1, &platform).await;
    eprintln!(
        "PROBE mojang install1: {:?}",
        r.as_ref().map(|r| r.executable.clone())
    );
    assert!(r.is_ok());
    let reqs = p1.downloader.requests();
    eprintln!("  checksum declared: {:?}", reqs[0].checksum);
    assert_eq!(
        reqs[0].checksum.as_ref().unwrap().algorithm(),
        HashAlgorithm::Sha1
    );
    assert_eq!(reqs[0].checksum.as_ref().unwrap().value(), sha);
    let mode = std::fs::metadata(dir.join(JAVA))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    eprintln!("  java mode {mode:o}");
    assert_eq!(mode, 0o755);
    // Second manifest writes a file through the link of the first install.
    let m2 = format!(
        r#"{{"files":{{{}, {}}}}}"#,
        file(JAVA, &sha, true),
        file("up/evil", &sha, false)
    );
    let p2 = ManagedJavaRuntimes::new(
        http_for(m2.as_bytes()),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(dir.join(JAVA), java25())]),
        rt.clone(),
    );
    let r = run(&p2, &platform).await;
    eprintln!(
        "PROBE mojang file through previous link: {:?} evil_at={} escapes={:?}",
        r.as_ref().err(),
        dir.join("jre.bundle/evil").exists(),
        escapes(sb.path())
    );
    assert!(escapes(sb.path()).is_empty());
}

fn adopt_json(checksum: Option<&str>) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(&fixture(
        "adoptium/adoptium-latest-25-jre-linux-aarch64.json",
    ))
    .unwrap();
    match checksum {
        Some(c) => v[0]["binary"]["package"]["checksum"] = c.into(),
        None => {
            v[0]["binary"]["package"]
                .as_object_mut()
                .unwrap()
                .remove("checksum");
        }
    }
    serde_json::to_vec(&v).unwrap()
}

fn tar(entries: &[(&str, Option<&str>, &[u8], u32)]) -> Vec<u8> {
    let mut b = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (name, link, body, mode) in entries {
        let mut h = tar::Header::new_old();
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        if let Some(l) = link {
            h.as_old_mut().linkname[..l.len()].copy_from_slice(l.as_bytes());
            h.set_entry_type(tar::EntryType::Symlink);
        } else {
            h.set_entry_type(tar::EntryType::Regular);
        }
        h.set_mode(*mode);
        h.set_size(body.len() as u64);
        h.set_cksum();
        b.append(&h, *body).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

#[tokio::test]
async fn adoptium_probes() {
    let platform = Platform::new(OsName::Linux, Arch::Arm64);
    let good_sha = "34828cbb93ed31c281c84ecb31ddab655d11a802f263c1fc019d42e9e0230fed";
    let cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        (
            "top folder symlink outside",
            adopt_json(Some(good_sha)),
            tar(&[
                ("jdk", Some("../../../../../../outside"), b"", 0o777),
                ("jdk/bin/java", None, b"#!", 0o755),
            ]),
        ),
        (
            "top folder symlink absolute",
            adopt_json(Some(good_sha)),
            tar(&[
                ("jdk", Some("/usr"), b"", 0o777),
                ("jdk/bin/java", None, b"#!", 0o755),
            ]),
        ),
        (
            "top folder symlink inside",
            adopt_json(Some(good_sha)),
            tar(&[
                ("real/bin/java", None, b"#!", 0o755),
                ("jdk", Some("real"), b"", 0o777),
            ]),
        ),
        (
            "bin/java symlink to /bin/sh",
            adopt_json(Some(good_sha)),
            tar(&[("jdk/bin/java", Some("/bin/sh"), b"", 0o777)]),
        ),
        (
            "no checksum field",
            adopt_json(None),
            tar(&[("jdk/bin/java", None, b"#!", 0o755)]),
        ),
        (
            "empty checksum",
            adopt_json(Some("")),
            tar(&[("jdk/bin/java", None, b"#!", 0o755)]),
        ),
        (
            "sha1-length checksum",
            adopt_json(Some(&"a".repeat(40))),
            tar(&[("jdk/bin/java", None, b"#!", 0o755)]),
        ),
    ];
    for (label, json, archive) in cases {
        let sb = tempfile::tempdir().unwrap();
        let rt = sb.path().join("runtimes");
        let base = rt.join("adoptium/25/linux-aarch64");
        let p = ManagedJavaRuntimes::new(
            FakeHttpClient::with([
                (ADOPT_URL.to_owned(), json),
                (ADOPT_JDK_URL.to_owned(), b"[]".to_vec()),
            ]),
            FakeDownloader::with([(LINK.to_owned(), archive)]),
            FakeJavaProbe::with([
                (base.join("jdk/bin/java"), java25()),
                (base.join("real/bin/java"), java25()),
            ]),
            rt,
        );
        let r = run(&p, &platform).await;
        let reqs = p.downloader.requests();
        eprintln!(
            "PROBE adoptium {label}: {} downloads={} checksum={:?} escapes={:?}",
            match &r {
                Ok(r) => format!("Ok({})", r.executable.display()),
                Err(e) => format!("Err({e})"),
            },
            reqs.len(),
            reqs.first().map(|q| q.checksum.clone()),
            escapes(sb.path())
        );
        assert!(escapes(sb.path()).is_empty());
        if label.contains("checksum") {
            assert!(r.is_err());
            assert!(reqs.is_empty());
        }
        if label.contains("outside") || label.contains("absolute") || label.contains("/bin/sh") {
            assert!(r.is_err());
        }
        if label == "top folder symlink inside" {
            use std::os::unix::fs::PermissionsExt;
            let exe = r.as_ref().unwrap().executable.clone();
            assert!(exe.starts_with(&base));
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
            eprintln!("  exe {} mode {mode:o}", exe.display());
            assert_eq!(mode, 0o755);
        }
    }
}
