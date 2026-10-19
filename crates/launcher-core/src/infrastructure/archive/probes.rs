#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use zip::write::SimpleFileOptions;

use super::extract::extract_zip_entries;
use super::*;

enum Z<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Link(&'a str, &'a str),
    /// Only the Unix exec-bit probe builds these; Windows has no exec bit.
    #[cfg(unix)]
    Exec(&'a str, &'a [u8]),
}

fn zip_bytes(entries: &[Z]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for e in entries {
        match e {
            Z::File(n, b) => {
                w.start_file(*n, SimpleFileOptions::default()).unwrap();
                w.write_all(b).unwrap();
            }
            #[cfg(unix)]
            Z::Exec(n, b) => {
                w.start_file(*n, SimpleFileOptions::default().unix_permissions(0o755))
                    .unwrap();
                w.write_all(b).unwrap();
            }
            Z::Dir(n) => {
                w.add_directory(*n, SimpleFileOptions::default()).unwrap();
            }
            Z::Link(n, t) => {
                w.add_symlink(*n, *t, SimpleFileOptions::default()).unwrap();
            }
        }
    }
    w.finish().unwrap().into_inner()
}

#[derive(Clone, Copy)]
enum T<'a> {
    File(&'a str, &'a [u8], u32),
    Sym(&'a str, &'a str),
    Hard(&'a str, &'a str),
    Fifo(&'a str),
    Dev(&'a str),
    Dir(&'a str),
}

/// Raw headers so the tar crate's own path validation is bypassed.
fn tar_bytes(entries: &[T]) -> Vec<u8> {
    let mut b = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for e in entries {
        let mut h = tar::Header::new_old();
        let (name, link, kind, body, mode): (&str, Option<&str>, tar::EntryType, &[u8], u32) =
            match *e {
                T::File(n, body, m) => (n, None, tar::EntryType::Regular, body, m),
                T::Sym(n, t) => (n, Some(t), tar::EntryType::Symlink, &[], 0o777),
                T::Hard(n, t) => (n, Some(t), tar::EntryType::Link, &[], 0o644),
                T::Fifo(n) => (n, None, tar::EntryType::Fifo, &[], 0o644),
                T::Dev(n) => (n, None, tar::EntryType::Char, &[], 0o644),
                T::Dir(n) => (n, None, tar::EntryType::Directory, &[], 0o755),
            };
        {
            let old = h.as_old_mut();
            old.name[..name.len()].copy_from_slice(name.as_bytes());
            if let Some(l) = link {
                old.linkname[..l.len()].copy_from_slice(l.as_bytes());
            }
        }
        h.set_entry_type(kind);
        h.set_mode(mode);
        h.set_size(body.len() as u64);
        h.set_cksum();
        b.append(&h, body).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

/// sandbox/{archive, out}; anything else appearing in sandbox or its parent is an escape.
struct Sb {
    dir: tempfile::TempDir,
    outside: tempfile::TempDir,
}

impl Sb {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            outside: tempfile::tempdir().unwrap(),
        }
    }
    fn out(&self) -> PathBuf {
        self.dir.path().join("a").join("b").join("out")
    }
    fn archive(&self, bytes: &[u8]) -> PathBuf {
        let p = self.dir.path().join("archive.bin");
        std::fs::write(&p, bytes).unwrap();
        p
    }
    /// Every path in the sandbox that is not the archive or under out/.
    fn escapes(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        walk(self.dir.path(), &mut found);
        walk(self.outside.path(), &mut found);
        let out = self.out();
        found
            .into_iter()
            .filter(|p| {
                !p.starts_with(&out)
                    && !p.ends_with("archive.bin")
                    && *p != self.dir.path().join("a")
                    && *p != self.dir.path().join("a/b")
            })
            .collect()
    }
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        found.push(p.clone());
        if e.file_type().unwrap().is_dir() {
            walk(&p, found);
        }
    }
}

fn report(label: &str, r: &Result<Vec<PathBuf>, crate::domain::launch::ExtractError>, sb: &Sb) {
    let esc = sb.escapes();
    eprintln!(
        "PROBE {label}: {} | escapes={:?}",
        match r {
            Ok(v) => format!("Ok({} files)", v.len()),
            Err(e) => format!("Err({e})"),
        },
        esc
    );
    assert!(esc.is_empty(), "{label} escaped: {esc:?}");
}

/// Renames every `zzB` in the archive bytes to `zzA` so duplicate names can exist.
fn dup(mut b: Vec<u8>) -> Vec<u8> {
    for i in 0..b.len().saturating_sub(2) {
        if &b[i..i + 3] == b"zzB" {
            b[i + 2] = b'A';
        }
    }
    b
}

fn zip_probe(
    label: &str,
    entries: &[Z],
) -> (
    Result<Vec<PathBuf>, crate::domain::launch::ExtractError>,
    Sb,
) {
    let sb = Sb::new();
    let a = sb.archive(&dup(zip_bytes(entries)));
    let r = extract_zip(&a, &sb.out(), &[]);
    report(label, &r, &sb);
    (r, sb)
}

fn tar_probe(
    label: &str,
    entries: &[T],
) -> (
    Result<Vec<PathBuf>, crate::domain::launch::ExtractError>,
    Sb,
) {
    let sb = Sb::new();
    let a = sb.archive(&tar_bytes(entries));
    let r = extract_tar_gz(&a, &sb.out());
    report(label, &r, &sb);
    (r, sb)
}

#[test]
fn zip_name_probes() {
    let long = "x".repeat(5000);
    for name in [
        "../x",
        "a/../../x",
        "/etc/x",
        "C:\\x",
        "C:x",
        "\\\\server\\share\\x",
        "a\\..\\..\\x",
        "a\0b",
        "..\\x",
        "./../x",
        "a/./../../x",
    ] {
        let (r, _) = zip_probe(&format!("zip name {name:?}"), &[Z::File(name, b"p")]);
        assert!(r.is_err(), "{name:?} accepted");
    }
    let (r, _) = zip_probe("zip very long name", &[Z::File(&long, b"p")]);
    assert!(r.is_err());
    let (r, _) = zip_probe("zip dir named ..", &[Z::Dir("../")]);
    assert!(r.is_err());
    let (r, _) = zip_probe("zip dir a/../..", &[Z::Dir("a/../../")]);
    assert!(r.is_err());
    let (r, sb) = zip_probe(
        "zip duplicate entries",
        &[Z::File("zzA", b"one"), Z::File("zzB", b"two")],
    );
    eprintln!(
        "  dup result {:?} content={:?}",
        r.as_ref().map(|v| v.len()),
        std::fs::read(sb.out().join("zzA")).ok()
    );
}

#[test]
fn zip_symlink_probes() {
    let (r, _) = zip_probe(
        "zip symlink ../.. then file through it",
        &[Z::Link("l", "../../.."), Z::File("l/pwn", b"p")],
    );
    assert!(r.is_err());
    let (r, _) = zip_probe(
        "zip symlink absolute then file",
        &[Z::Link("l", "/tmp"), Z::File("l/pwn", b"p")],
    );
    assert!(r.is_err());
    let (r, sb) = zip_probe(
        "zip symlink . then file",
        &[Z::Link("l", "."), Z::File("l/ok", b"p")],
    );
    assert!(r.is_ok(), "{r:?}");
    assert!(sb.out().join("ok").is_file());
    let (r, _) = zip_probe(
        "zip chain a->. a/b->..",
        &[
            Z::Link("a", "."),
            Z::Link("a/b", ".."),
            Z::File("a/b/pwn", b"p"),
        ],
    );
    assert!(r.is_err());
    let (r, _) = zip_probe(
        "zip symlink then overwrite link target file",
        &[
            Z::Link("zzA", "../../../../../../../../../../tmp/zz"),
            Z::File("zzB", b"p"),
        ],
    );
    eprintln!("  dup link/file -> {:?}", r.as_ref().map(|v| v.len()));
    // file at link path replaces link rather than writing through.
    let (r, sb) = zip_probe(
        "zip internal link then file at same path",
        &[
            Z::File("t", b"orig"),
            Z::Link("zzA", "t"),
            Z::File("zzB", b"new"),
        ],
    );
    eprintln!(
        "  t={:?} l_is_symlink={}",
        std::fs::read(sb.out().join("t")).ok(),
        std::fs::symlink_metadata(sb.out().join("zzA"))
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    );
    eprintln!("  -> {:?}", r.as_ref().map(|v| v.len()));
    assert_eq!(std::fs::read(sb.out().join("t")).unwrap(), b"orig");
    let (r, sb) = zip_probe(
        "zip file then dir-link same name then file through",
        &[
            Z::File("zzA", b"x"),
            Z::Link("zzB", "."),
            Z::File("zzA/inner", b"p"),
        ],
    );
    eprintln!(
        "  -> {:?} inner_at_root={}",
        r.as_ref().map(|v| v.len()),
        sb.out().join("inner").exists()
    );
    // link target containing a backslash or a mid `..`
    let (r, _) = zip_probe(
        "zip link sub/../..",
        &[Z::Dir("sub/"), Z::Link("l", "sub/../..")],
    );
    assert!(r.is_err());
    let (r, _) = zip_probe("zip link target empty", &[Z::Link("l", "")]);
    assert!(r.is_err());
    // Natives extractor path with a symlink escape
    let sb = Sb::new();
    let a = sb.archive(&zip_bytes(&[
        Z::Link("l", "../../.."),
        Z::File("l/pwn.so", b"p"),
    ]));
    let r = extract_zip_entries(&a, &sb.out(), &["META-INF/".into()], false);
    report("natives symlink escape", &r, &sb);
    assert!(r.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let (r, sb) = zip_probe("zip exec bit", &[Z::Exec("bin/x", b"#!")]);
        assert!(r.is_ok());
        let mode = std::fs::metadata(sb.out().join("bin/x"))
            .unwrap()
            .permissions()
            .mode();
        eprintln!("  zip exec mode {:o}", mode & 0o777);
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn tar_probes() {
    let cases: Vec<(&str, Vec<T>, bool)> = vec![
        (
            "tar symlink /etc + file",
            vec![T::Sym("l", "/etc"), T::File("l/pwn", b"p", 0o644)],
            false,
        ),
        (
            "tar symlink ../..",
            vec![T::Sym("l", "../.."), T::File("l/pwn", b"p", 0o644)],
            false,
        ),
        (
            "tar chain a->. a/b->..",
            vec![
                T::Sym("a", "."),
                T::Sym("a/b", ".."),
                T::File("a/b/pwn", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar chain d/a->.. (inside), d/a/b->..",
            vec![
                T::Dir("d"),
                T::Sym("d/a", ".."),
                T::Sym("d/a/b", ".."),
                T::File("d/a/b/pwn", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar hard link absolute",
            vec![T::Hard("h", "/etc/passwd")],
            false,
        ),
        (
            "tar hard link ../",
            vec![T::Hard("h", "../../archive.bin")],
            false,
        ),
        (
            "tar hard link via symlink to outside",
            vec![T::Sym("l", "."), T::Hard("h", "l/../archive.bin")],
            false,
        ),
        (
            "tar symlink /tmp/outside then file",
            vec![
                T::Sym("link", "/tmp/outside"),
                T::File("link/file", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar symlink to file outside then file over link",
            vec![
                T::Sym("link", "../../../archive.bin"),
                T::File("link", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar ./ prefix",
            vec![
                T::File("./ok/f", b"p", 0o755),
                T::File("./ok/../../x", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar fifo",
            vec![T::Fifo("fifo"), T::File("ok", b"p", 0o644)],
            true,
        ),
        (
            "tar char device",
            vec![T::Dev("dev"), T::File("ok", b"p", 0o644)],
            true,
        ),
        (
            "tar name with backslash ..",
            vec![T::File("a\\..\\..\\x", b"p", 0o644)],
            false,
        ),
        (
            "tar absolute file",
            vec![T::File("/tmp/abs_probe_x", b"p", 0o644)],
            false,
        ),
        ("tar setuid file", vec![T::File("suid", b"p", 0o4755)], true),
        (
            "tar internal symlink chain ok",
            vec![
                T::Dir("x/y"),
                T::Sym("x/y/up", "../.."),
                T::File("x/y/up/f", b"p", 0o644),
            ],
            true,
        ),
        (
            "tar symlink through internal link resolving up then ..",
            vec![
                T::Dir("x/y"),
                T::Sym("x/y/up", "../.."),
                T::Sym("x/y/up/esc", ".."),
                T::File("x/y/up/esc/pwn", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar symlink in dir reached via symlink",
            vec![
                T::Dir("p/q"),
                T::Sym("deep", "p/q"),
                T::Sym("deep/l", "../../.."),
                T::File("deep/l/pwn", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar dir symlink dangling then dir entry through",
            vec![
                T::Sym("dl", "notyet"),
                T::Dir("dl/sub"),
                T::File("dl/sub/f", b"p", 0o644),
            ],
            false,
        ),
        (
            "tar file, link->file, file at link path",
            vec![
                T::File("t", b"orig", 0o644),
                T::Sym("l", "t"),
                T::File("l", b"new", 0o644),
            ],
            true,
        ),
        (
            "tar link->dir then dir entry same name",
            vec![T::Dir("d"), T::Sym("l", "d"), T::Dir("l")],
            true,
        ),
        (
            "tar hard link to symlink to dir",
            vec![T::Dir("d"), T::Sym("l", "d"), T::Hard("h", "l")],
            false,
        ),
        (
            "tar hard link to internal file",
            vec![T::File("f", b"x", 0o755), T::Hard("h", "f")],
            true,
        ),
    ];
    for (label, entries, ok) in cases {
        let (r, sb) = tar_probe(label, &entries);
        assert_eq!(r.is_ok(), ok, "{label}: {r:?}");
        if label == "tar setuid file" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let m = std::fs::metadata(sb.out().join("suid"))
                    .unwrap()
                    .permissions()
                    .mode();
                eprintln!("  suid mode {:o}", m & 0o7777);
                assert_eq!(m & 0o7000, 0);
            }
        }
        if label.starts_with("tar file, link->file") {
            assert_eq!(std::fs::read(sb.out().join("t")).unwrap(), b"orig");
            assert!(
                !std::fs::symlink_metadata(sb.out().join("l"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        if label == "tar fifo" || label == "tar char device" {
            let n = if label == "tar fifo" { "fifo" } else { "dev" };
            assert!(std::fs::symlink_metadata(sb.out().join(n)).is_err());
        }
    }
    let _ = Path::new("/tmp/outside");
    assert!(!Path::new("/tmp/abs_probe_x").exists());
}

#[test]
fn create_link_inside_probes() {
    let sb = Sb::new();
    std::fs::create_dir_all(sb.out()).unwrap();
    for (k, t) in [
        ("a", "../x"),
        ("../a", "x"),
        ("/a", "x"),
        ("a/b", "../../x"),
        ("b", "/etc"),
        ("c", "d/../../x"),
    ] {
        let r = create_link_inside(&sb.out(), k, t);
        eprintln!("PROBE create_link_inside {k:?}->{t:?}: {r:?}");
        assert!(r.is_err());
    }
    assert!(sb.escapes().is_empty());
}
