#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use zip::write::SimpleFileOptions;

use super::*;
use crate::domain::launch::{Artifact, ExtractError, NativeJar, NativesExtractor, RelativePath};

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in entries {
        if name.ends_with('/') {
            writer
                .add_directory(*name, SimpleFileOptions::default())
                .unwrap();
        } else {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body).unwrap();
        }
    }
    let bytes = writer.finish().unwrap().into_inner();
    std::fs::write(path, bytes).unwrap();
}

/// Builds a tar.gz; `links` are `(name, target)` symlinks.
fn write_tar_gz(path: &Path, files: &[(&str, &[u8], u32)], links: &[(&str, &str)]) {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (name, body, mode) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(*mode);
        header.set_entry_type(tar::EntryType::Regular);
        builder.append_data(&mut header, name, *body).unwrap();
    }
    for (name, target) in links {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        builder.append_link(&mut header, name, target).unwrap();
    }
    let bytes = builder.into_inner().unwrap().finish().unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A temp root with `archives/` and `out/`; escapes would land in the root.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("archives")).unwrap();
        Self { root }
    }

    fn archive(&self, name: &str) -> PathBuf {
        self.root.path().join("archives").join(name)
    }

    fn out(&self) -> PathBuf {
        self.root.path().join("out")
    }

    fn root_entries(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

#[test]
fn zip_with_a_parent_dir_entry_is_rejected_and_writes_nothing_outside() {
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("evil.zip");
    write_zip(&archive, &[("ok.txt", b"ok"), ("../evil.txt", b"pwned")]);

    let result = extract_zip(&archive, &sandbox.out(), &[]);

    assert_eq!(result, Err(ExtractError::UnsafeEntry("../evil.txt".into())));
    assert!(!sandbox.root.path().join("evil.txt").exists());
    assert_eq!(sandbox.root_entries(), vec!["archives", "out"]);
}

#[test]
fn zip_with_an_absolute_entry_is_rejected() {
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("absolute.zip");
    let absolute = sandbox.root.path().join("abs.txt");
    let absolute_name = absolute.to_string_lossy().replace('\\', "/");
    write_zip(&archive, &[(absolute_name.as_str(), b"pwned")]);

    let result = extract_zip(&archive, &sandbox.out(), &[]);

    assert!(matches!(result, Err(ExtractError::UnsafeEntry(_))));
    assert!(!absolute.exists());
}

#[test]
fn zip_with_a_windows_drive_or_unc_entry_is_rejected() {
    for name in [
        "C:/Windows/evil.dll",
        "C:evil.dll",
        "\\\\server\\share\\x",
        "a\\..\\..\\x",
    ] {
        let sandbox = Sandbox::new();
        let archive = sandbox.archive("windows.zip");
        write_zip(&archive, &[(name, b"pwned")]);
        assert_eq!(
            extract_zip(&archive, &sandbox.out(), &[]),
            Err(ExtractError::UnsafeEntry(name.into())),
            "{name}"
        );
    }
}

#[test]
fn zip_extracts_files_and_honors_exclude_prefixes() {
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("natives.jar");
    write_zip(
        &archive,
        &[
            ("META-INF/", b""),
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0"),
            ("liblwjgl.so", b"elf"),
            ("sub/", b""),
            ("sub/libopenal.so", b"elf2"),
        ],
    );

    let mut files = extract_zip(&archive, &sandbox.out(), &["META-INF/".to_owned()]).unwrap();
    files.sort();

    let out = sandbox.out().canonicalize().unwrap();
    assert_eq!(
        files,
        vec![
            out.join("liblwjgl.so"),
            out.join("sub").join("libopenal.so")
        ]
    );
    assert_eq!(std::fs::read(out.join("liblwjgl.so")).unwrap(), b"elf");
    assert!(!out.join("META-INF").exists());
}

#[test]
fn tar_gz_with_a_symlink_pointing_outside_is_rejected() {
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("evil.tar.gz");
    write_tar_gz(&archive, &[], &[("jre/escape", "../../secret")]);

    let result = extract_tar_gz(&archive, &sandbox.out());

    assert_eq!(result, Err(ExtractError::UnsafeEntry("jre/escape".into())));
    assert!(std::fs::symlink_metadata(sandbox.out().join("jre/escape")).is_err());
}

#[test]
fn tar_gz_with_an_absolute_symlink_target_is_rejected() {
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("absolute-link.tar.gz");
    write_tar_gz(&archive, &[], &[("passwd", "/etc/passwd")]);

    assert_eq!(
        extract_tar_gz(&archive, &sandbox.out()),
        Err(ExtractError::UnsafeEntry("passwd".into()))
    );
}

#[test]
fn tar_gz_rejects_a_link_chain_that_escapes_through_an_earlier_symlink() {
    // `here -> .` is inside; `here/up -> ..` looks inside lexically but resolves
    // to the parent of the destination once `here` is followed.
    let sandbox = Sandbox::new();
    let archive = sandbox.archive("chain.tar.gz");
    write_tar_gz(&archive, &[], &[("here", "."), ("here/up", "..")]);

    let result = extract_tar_gz(&archive, &sandbox.out());
    #[cfg(unix)]
    assert_eq!(result, Err(ExtractError::UnsafeEntry("here/up".into())));
    // Windows creates no links (`link_or_copy`), so nothing can resolve outside.
    #[cfg(not(unix))]
    assert!(result.is_ok(), "{result:?}");
    // On every platform nothing was written next to the destination.
    assert_eq!(sandbox.root_entries(), vec!["archives", "out"]);
}

#[cfg(unix)]
#[test]
fn tar_gz_preserves_internal_symlinks_and_executable_bits() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let archive = sandbox.archive("jre.tar.gz");
    write_tar_gz(
        &archive,
        &[
            ("jdk/bin/java", b"#!/bin/sh\n", 0o755),
            ("jdk/lib/libjvm.so", b"elf", 0o644),
        ],
        &[("jdk/lib/current", "libjvm.so"), ("jdk/java", "bin/java")],
    );

    let files = extract_tar_gz(&archive, &sandbox.out()).unwrap();

    let out = sandbox.out().canonicalize().unwrap();
    assert!(files.contains(&out.join("jdk/bin/java")));
    let java = out.join("jdk/bin/java");
    assert_eq!(
        std::fs::metadata(&java).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::metadata(out.join("jdk/lib/libjvm.so"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    let link = out.join("jdk/lib/current");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        PathBuf::from("libjvm.so")
    );
    assert_eq!(std::fs::read(out.join("jdk/java")).unwrap(), b"#!/bin/sh\n");
}

#[tokio::test]
async fn natives_extractor_extracts_every_jar_and_excludes_meta_inf() {
    let sandbox = Sandbox::new();
    let first = sandbox.archive("lwjgl-natives.jar");
    let second = sandbox.archive("openal-natives.jar");
    write_zip(
        &first,
        &[
            ("META-INF/", b""),
            ("META-INF/LWJGL.SF", b"sig"),
            ("liblwjgl.dylib", b"a"),
        ],
    );
    write_zip(
        &second,
        &[("META-INF/MANIFEST.MF", b"m"), ("libopenal.dylib", b"b")],
    );
    let jar = |name: &str| NativeJar {
        artifact: Artifact {
            path: RelativePath::parse(name).unwrap(),
            url: format!("https://libraries.minecraft.net/{name}"),
            checksum: None,
            size: None,
        },
        exclude: vec!["META-INF/".to_owned()],
    };
    let natives = sandbox.root.path().join("natives");

    ZipNativesExtractor
        .extract(
            &[
                (first, jar("lwjgl-natives.jar")),
                (second, jar("openal-natives.jar")),
            ],
            &natives,
        )
        .await
        .unwrap();

    assert_eq!(std::fs::read(natives.join("liblwjgl.dylib")).unwrap(), b"a");
    assert_eq!(
        std::fs::read(natives.join("libopenal.dylib")).unwrap(),
        b"b"
    );
    assert!(!natives.join("META-INF").exists());
}

#[tokio::test]
async fn natives_extractor_rejects_unsafe_entries() {
    let sandbox = Sandbox::new();
    let jar_path = sandbox.archive("evil.jar");
    write_zip(&jar_path, &[("../../evil.dll", b"x")]);
    let jar = NativeJar {
        artifact: Artifact {
            path: RelativePath::parse("evil.jar").unwrap(),
            url: "https://example.invalid/evil.jar".into(),
            checksum: None,
            size: None,
        },
        exclude: vec![],
    };

    let result = ZipNativesExtractor
        .extract(&[(jar_path, jar)], &sandbox.out())
        .await;

    assert_eq!(
        result,
        Err(ExtractError::UnsafeEntry("../../evil.dll".into()))
    );
}
