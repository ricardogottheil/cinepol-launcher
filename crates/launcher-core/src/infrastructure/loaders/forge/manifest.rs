//! The `Main-Class` of a processor jar, from its `META-INF/MANIFEST.MF`.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::domain::loader::LoaderInstallError;

const MANIFEST: &str = "META-INF/MANIFEST.MF";
/// Real manifests are a few KiB; a larger one is not read.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// The `Main-Class` of a manifest's main section (up to the first blank
/// line). Lines starting with one space continue the previous header. Only
/// Java class names are accepted, so a manifest can never pass a JVM option.
pub(super) fn main_class(manifest: &str) -> Option<String> {
    let mut headers: Vec<String> = Vec::new();
    for line in manifest.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            break;
        }
        match line.strip_prefix(' ') {
            Some(continued) => headers.last_mut()?.push_str(continued),
            None => headers.push(line.to_owned()),
        }
    }
    let value = headers.iter().find_map(|header| {
        let (name, value) = header.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("Main-Class")
            .then(|| value.trim().to_owned())
    })?;
    let valid = value.split('.').all(|part| {
        part.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
            && part
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    });
    valid.then_some(value)
}

/// Reads the `Main-Class` of a jar. The manifest is read into memory; nothing
/// in the jar is written to disk.
pub(super) async fn read_main_class(jar: &Path) -> Result<String, LoaderInstallError> {
    let path = jar.to_path_buf();
    tokio::task::spawn_blocking(move || read_blocking(&path))
        .await
        .map_err(|error| LoaderInstallError::ProcessorFailed(error.to_string()))?
}

fn read_blocking(jar: &Path) -> Result<String, LoaderInstallError> {
    let failed = |reason: String| {
        LoaderInstallError::ProcessorFailed(format!("{}: {reason}", jar.display()))
    };
    let file = File::open(jar).map_err(|error| failed(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| failed(error.to_string()))?;
    let entry = archive
        .by_name(MANIFEST)
        .map_err(|error| failed(format!("no manifest ({error})")))?;
    let mut bytes = Vec::new();
    entry
        .take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| failed(error.to_string()))?;
    main_class(&String::from_utf8_lossy(&bytes))
        .ok_or_else(|| failed("the manifest names no valid Main-Class".to_owned()))
}
