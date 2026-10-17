#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use super::mojang::RuntimeIndex;
use super::*;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::java::{
    JavaRequirement, JavaRuntimeError, JavaRuntimeProvider, JavaSource, JavaVersion,
};
use crate::domain::launch::{Arch, OsName, Platform};
use crate::testing::{FakeDownloader, FakeHttpClient, FakeJavaProbe, RecordingReporter};

const EPSILON_MANIFEST_URL: &str = "https://piston-meta.mojang.com/v1/packages/5643d63e75d54da43c9501cdb1988530a9bc5d7f/manifest.json";
const ADOPTIUM_JRE_25_LINUX_ARM64: &str = "https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=aarch64&image_type=jre&os=linux&vendor=eclipse";
const TEMURIN_LINK: &str = "https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.4.1%2B1/OpenJDK25U-jre_aarch64_linux_hotspot_25.0.4.1_1.tar.gz";
const TEMURIN_SHA256: &str = "34828cbb93ed31c281c84ecb31ddab655d11a802f263c1fc019d42e9e0230fed";

fn fixture(relative: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(relative),
    )
    .unwrap()
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

/// The trimmed fixtures do not hash to the digests the real index declares,
/// so the index is patched in memory to declare the served manifest's sha1.
fn index_declaring(platform: &str, component: &str, manifest: &[u8]) -> Vec<u8> {
    let mut index: serde_json::Value =
        serde_json::from_slice(&fixture("mojang/java-runtime-all-trimmed.json")).unwrap();
    index[platform][component][0]["manifest"]["sha1"] = sha1_hex(manifest).into();
    serde_json::to_vec(&index).unwrap()
}

fn java(major: u32) -> JavaVersion {
    JavaVersion {
        major,
        full: format!("{major}.0.1"),
        is_64_bit: true,
    }
}

fn epsilon() -> JavaRequirement {
    JavaRequirement {
        component: "java-runtime-epsilon".into(),
        major: 25,
    }
}

fn mac_arm64() -> Platform {
    Platform::new(OsName::MacOs, Arch::Arm64)
}

fn epsilon_install_dir(runtimes: &Path) -> PathBuf {
    runtimes.join("mojang/java-runtime-epsilon/mac-os-arm64")
}

fn epsilon_java(runtimes: &Path) -> PathBuf {
    epsilon_install_dir(runtimes).join("jre.bundle/Contents/Home/bin/java")
}

fn mojang_http(manifest: &[u8]) -> FakeHttpClient {
    FakeHttpClient::with([
        (
            MOJANG_RUNTIME_INDEX_URL.to_owned(),
            index_declaring("mac-os-arm64", "java-runtime-epsilon", manifest),
        ),
        (EPSILON_MANIFEST_URL.to_owned(), manifest.to_vec()),
    ])
}

async fn ensure<H, D, P>(
    provider: &ManagedJavaRuntimes<H, D, P>,
    requirement: &JavaRequirement,
    platform: &Platform,
) -> Result<crate::domain::java::JavaRuntime, JavaRuntimeError>
where
    H: crate::domain::ports::HttpClient,
    D: crate::domain::ports::Downloader,
    P: crate::domain::java::JavaProbe,
{
    provider
        .ensure(
            requirement,
            platform,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
}

#[test]
fn runtime_index_offers_epsilon_but_not_jre_legacy_on_apple_silicon() {
    let index = RuntimeIndex::parse(&fixture("mojang/java-runtime-all-trimmed.json")).unwrap();

    let epsilon = index.entry("mac-os-arm64", "java-runtime-epsilon").unwrap();
    assert_eq!(epsilon.version.name, "25.0.1");
    assert_eq!(epsilon.manifest.url, EPSILON_MANIFEST_URL);
    assert_eq!(
        epsilon.manifest.sha1,
        "5643d63e75d54da43c9501cdb1988530a9bc5d7f"
    );
    assert!(index.entry("mac-os-arm64", "jre-legacy").is_none());
    assert!(index.entry("linux-arm64", "java-runtime-epsilon").is_none());
}

#[tokio::test]
async fn installs_a_mojang_runtime_and_reuses_it_afterwards() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = fixture("mojang/java-runtime-epsilon-mac-os-arm64-trimmed.json");
    let java_path = epsilon_java(runtimes.path());
    let provider = ManagedJavaRuntimes::new(
        mojang_http(&manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(java_path.clone(), java(25))]),
        runtimes.path().to_path_buf(),
    );

    let runtime = ensure(&provider, &epsilon(), &mac_arm64()).await.unwrap();

    assert_eq!(runtime.executable, java_path);
    assert_eq!(runtime.version, java(25));
    assert_eq!(runtime.source, JavaSource::Managed);
    let install = epsilon_install_dir(runtimes.path());
    assert!(install.join("jre.bundle/Contents").is_dir());

    let requests = provider.downloader.requests();
    assert_eq!(requests.len(), 5);
    let java_request = requests
        .iter()
        .find(|request| request.destination == java_path)
        .unwrap();
    assert_eq!(
        java_request.url,
        "https://piston-data.mojang.com/v1/objects/5af4a90be54ddeec03db3700e6989594c9438952/java"
    );
    assert_eq!(
        java_request.checksum,
        Some(
            Checksum::parse(
                HashAlgorithm::Sha1,
                "5af4a90be54ddeec03db3700e6989594c9438952"
            )
            .unwrap()
        )
    );
    assert_eq!(java_request.size, Some(84720));
    assert!(requests.iter().all(|request| {
        request.checksum.as_ref().map(Checksum::algorithm) == Some(HashAlgorithm::Sha1)
    }));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&java_path), 0o755);
        assert_eq!(
            mode(&install.join("jre.bundle/Contents/Home/bin/jar")),
            0o755
        );
        assert_eq!(
            mode(&install.join("jre.bundle/Contents/Home/conf/jaxp.properties")) & 0o111,
            0
        );
        let link = install.join("jre.bundle/Contents/Home/legal/java.compiler/ASSEMBLY_EXCEPTION");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("../java.base/ASSEMBLY_EXCEPTION")
        );
    }

    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(install.join(".cinepol-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(marker["source"], "mojang");
    assert_eq!(marker["id"], "java-runtime-epsilon");
    assert_eq!(marker["version"], "25.0.1");
    assert_eq!(marker["revision"], sha1_hex(&manifest));

    // Second run: the marker matches the index, nothing is downloaded again.
    let again = ensure(&provider, &epsilon(), &mac_arm64()).await.unwrap();
    assert_eq!(again.executable, java_path);
    assert_eq!(provider.downloader.requests().len(), 5);
    assert_eq!(provider.http.requests().len(), 3);

    // Offline: the index cannot be fetched, the installed runtime is reused.
    let offline = ManagedJavaRuntimes::new(
        FakeHttpClient::with([]),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(java_path.clone(), java(25))]),
        runtimes.path().to_path_buf(),
    );
    assert_eq!(
        ensure(&offline, &epsilon(), &mac_arm64())
            .await
            .unwrap()
            .executable,
        java_path
    );
    assert!(offline.downloader.requests().is_empty());
}

#[tokio::test]
async fn offline_without_an_installed_runtime_is_a_download_error() {
    let runtimes = tempfile::tempdir().unwrap();
    let provider = ManagedJavaRuntimes::new(
        FakeHttpClient::with([]),
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );

    assert!(matches!(
        ensure(&provider, &epsilon(), &mac_arm64()).await,
        Err(JavaRuntimeError::Download(_))
    ));
}

#[tokio::test]
async fn a_runtime_manifest_with_the_wrong_sha1_is_rejected() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = fixture("mojang/java-runtime-epsilon-mac-os-arm64-trimmed.json");
    let http = FakeHttpClient::with([
        (
            MOJANG_RUNTIME_INDEX_URL.to_owned(),
            fixture("mojang/java-runtime-all-trimmed.json"),
        ),
        (EPSILON_MANIFEST_URL.to_owned(), manifest),
    ]);
    let provider = ManagedJavaRuntimes::new(
        http,
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );

    assert!(matches!(
        ensure(&provider, &epsilon(), &mac_arm64()).await,
        Err(JavaRuntimeError::Download(reason)) if reason.contains("sha1")
    ));
    assert!(provider.downloader.requests().is_empty());
}

#[tokio::test]
async fn an_unsafe_runtime_manifest_key_is_rejected() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = br#"{"files":{
        "../x": {"type": "file", "executable": true, "downloads": {"raw": {
            "sha1": "5af4a90be54ddeec03db3700e6989594c9438952", "size": 1,
            "url": "https://piston-data.mojang.com/v1/objects/x"}}}
    }}"#;
    let provider = ManagedJavaRuntimes::new(
        mojang_http(manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );

    assert!(matches!(
        ensure(&provider, &epsilon(), &mac_arm64()).await,
        Err(JavaRuntimeError::Invalid(reason)) if reason.contains("../x")
    ));
    assert!(provider.downloader.requests().is_empty());
    assert!(
        !runtimes
            .path()
            .join("mojang/java-runtime-epsilon/x")
            .exists()
    );
}

#[tokio::test]
async fn a_runtime_manifest_link_escaping_the_install_dir_is_rejected() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = br#"{"files":{
        "bin": {"type": "directory"},
        "bin/escape": {"type": "link", "target": "../../../../secret"}
    }}"#;
    let provider = ManagedJavaRuntimes::new(
        mojang_http(manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );

    assert!(matches!(
        ensure(&provider, &epsilon(), &mac_arm64()).await,
        Err(JavaRuntimeError::Invalid(_))
    ));
    let link = epsilon_install_dir(runtimes.path()).join("bin/escape");
    assert!(std::fs::symlink_metadata(link).is_err());
}

#[tokio::test]
async fn a_probe_reporting_another_major_version_is_invalid() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = fixture("mojang/java-runtime-epsilon-mac-os-arm64-trimmed.json");
    let provider = ManagedJavaRuntimes::new(
        mojang_http(&manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(epsilon_java(runtimes.path()), java(21))]),
        runtimes.path().to_path_buf(),
    );

    assert!(matches!(
        ensure(&provider, &epsilon(), &mac_arm64()).await,
        Err(JavaRuntimeError::Invalid(_))
    ));
}

#[tokio::test]
async fn cancellation_before_the_download_is_reported() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = fixture("mojang/java-runtime-epsilon-mac-os-arm64-trimmed.json");
    let provider = ManagedJavaRuntimes::new(
        mojang_http(&manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = provider
        .ensure(
            &epsilon(),
            &mac_arm64(),
            &RecordingReporter::default(),
            &cancel,
        )
        .await;

    assert_eq!(result, Err(JavaRuntimeError::Cancelled));
}

fn temurin_tar_gz() -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (name, body, mode) in [
        (
            "jdk-25.0.4.1+1-jre/release",
            &b"JAVA_VERSION=\"25.0.4.1\"\n"[..],
            0o644,
        ),
        ("jdk-25.0.4.1+1-jre/bin/java", &b"#!/bin/sh\n"[..], 0o755),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(mode);
        header.set_entry_type(tar::EntryType::Regular);
        builder.append_data(&mut header, name, body).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

#[tokio::test]
async fn linux_arm64_falls_back_to_adoptium() {
    let runtimes = tempfile::tempdir().unwrap();
    let java_path = runtimes
        .path()
        .join("adoptium/25/linux-aarch64/jdk-25.0.4.1+1-jre/bin/java");
    let provider = ManagedJavaRuntimes::new(
        FakeHttpClient::with([(
            ADOPTIUM_JRE_25_LINUX_ARM64.to_owned(),
            fixture("adoptium/adoptium-latest-25-jre-linux-aarch64.json"),
        )]),
        FakeDownloader::with([(TEMURIN_LINK.to_owned(), temurin_tar_gz())]),
        FakeJavaProbe::with([(java_path.clone(), java(25))]),
        runtimes.path().to_path_buf(),
    );
    let linux_arm64 = Platform::new(OsName::Linux, Arch::Arm64);

    let runtime = ensure(&provider, &epsilon(), &linux_arm64).await.unwrap();

    assert_eq!(runtime.executable, java_path);
    assert_eq!(runtime.source, JavaSource::Managed);
    assert!(java_path.is_file());
    let requests = provider.downloader.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, TEMURIN_LINK);
    assert_eq!(
        requests[0].checksum,
        Some(Checksum::parse(HashAlgorithm::Sha256, TEMURIN_SHA256).unwrap())
    );
    assert!(
        requests[0]
            .destination
            .starts_with(runtimes.path().join("tmp"))
    );
    assert_eq!(provider.http.requests(), vec![ADOPTIUM_JRE_25_LINUX_ARM64]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&java_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
    let marker: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            runtimes
                .path()
                .join("adoptium/25/linux-aarch64/.cinepol-runtime.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(marker["source"], "adoptium");
    assert_eq!(marker["version"], "jdk-25.0.4.1+1");

    // Reused on the next run: no second download.
    ensure(&provider, &epsilon(), &linux_arm64).await.unwrap();
    assert_eq!(provider.downloader.requests().len(), 1);
}

#[tokio::test]
async fn adoptium_without_a_jre_retries_with_a_jdk() {
    let runtimes = tempfile::tempdir().unwrap();
    let jdk_url = adoptium_assets_url(25, "linux", "aarch64", "jdk");
    let java_path = runtimes
        .path()
        .join("adoptium/25/linux-aarch64/jdk-25.0.4.1+1-jre/bin/java");
    let provider = ManagedJavaRuntimes::new(
        FakeHttpClient::with([
            (ADOPTIUM_JRE_25_LINUX_ARM64.to_owned(), b"[]".to_vec()),
            (
                jdk_url.clone(),
                fixture("adoptium/adoptium-latest-25-jre-linux-aarch64.json"),
            ),
        ]),
        FakeDownloader::with([(TEMURIN_LINK.to_owned(), temurin_tar_gz())]),
        FakeJavaProbe::with([(java_path.clone(), java(25))]),
        runtimes.path().to_path_buf(),
    );

    let runtime = ensure(
        &provider,
        &epsilon(),
        &Platform::new(OsName::Linux, Arch::Arm64),
    )
    .await
    .unwrap();

    assert_eq!(runtime.executable, java_path);
    assert_eq!(
        provider.http.requests(),
        vec![ADOPTIUM_JRE_25_LINUX_ARM64.to_owned(), jdk_url]
    );
}

#[tokio::test]
async fn no_mojang_jre_and_no_adoptium_build_is_unavailable() {
    let runtimes = tempfile::tempdir().unwrap();
    let provider = ManagedJavaRuntimes::new(
        FakeHttpClient::with([
            (
                MOJANG_RUNTIME_INDEX_URL.to_owned(),
                fixture("mojang/java-runtime-all-trimmed.json"),
            ),
            (
                adoptium_assets_url(8, "mac", "aarch64", "jre"),
                b"[]".to_vec(),
            ),
            (
                adoptium_assets_url(8, "mac", "aarch64", "jdk"),
                b"[]".to_vec(),
            ),
        ]),
        FakeDownloader::with([]),
        FakeJavaProbe::default(),
        runtimes.path().to_path_buf(),
    );

    let result = ensure(&provider, &JavaRequirement::legacy(), &mac_arm64()).await;

    assert_eq!(
        result,
        Err(JavaRuntimeError::Unavailable {
            major: 8,
            platform: "mac-aarch64".into()
        })
    );
    assert_eq!(provider.http.requests().len(), 3);
    assert!(provider.downloader.requests().is_empty());
}

/// Fails the first probe (a broken installed runtime), then works.
struct FlakyProbe {
    calls: std::sync::Mutex<u32>,
}

impl crate::domain::java::JavaProbe for FlakyProbe {
    async fn probe(
        &self,
        _executable: &Path,
    ) -> Result<JavaVersion, crate::domain::java::JavaProbeError> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            Err(crate::domain::java::JavaProbeError::Unreadable(
                "corrupted runtime".into(),
            ))
        } else {
            Ok(java(25))
        }
    }
}

#[tokio::test]
async fn a_broken_installed_runtime_is_reinstalled_once() {
    let runtimes = tempfile::tempdir().unwrap();
    let manifest = fixture("mojang/java-runtime-epsilon-mac-os-arm64-trimmed.json");
    let healthy = ManagedJavaRuntimes::new(
        mojang_http(&manifest),
        FakeDownloader::with([]),
        FakeJavaProbe::with([(epsilon_java(runtimes.path()), java(25))]),
        runtimes.path().to_path_buf(),
    );
    ensure(&healthy, &epsilon(), &mac_arm64()).await.unwrap();

    let flaky = ManagedJavaRuntimes::new(
        mojang_http(&manifest),
        FakeDownloader::with([]),
        FlakyProbe {
            calls: std::sync::Mutex::new(0),
        },
        runtimes.path().to_path_buf(),
    );
    let runtime = ensure(&flaky, &epsilon(), &mac_arm64()).await.unwrap();

    assert_eq!(runtime.version, java(25));
    assert_eq!(
        *flaky.probe.calls.lock().unwrap(),
        2,
        "probed, reinstalled, probed again"
    );
    assert!(
        !flaky.downloader.requests().is_empty(),
        "the broken runtime was downloaded again"
    );
}
