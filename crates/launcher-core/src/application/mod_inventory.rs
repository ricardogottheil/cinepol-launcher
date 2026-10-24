//! Reads an instance's `mods/` folder: the jars the launcher installed, the
//! ones the catalog recognises by hash, and the ones it does not know.
//! Shared by the use cases that list, install, enable and remove mods.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::domain::integrity::HashAlgorithm;
use crate::domain::launch::RelativePath;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::{
    DISABLED_SUFFIX, InstalledMod, ModEntry, ModOrigin, ModRegistry, ModSource, ModVersion,
    mod_jar_name,
};

/// The jars in `mods/` and the registry they make up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInventory {
    /// Every mod jar, sorted by title.
    pub entries: Vec<ModEntry>,
    /// The saved registry brought up to date: records whose jar is gone or
    /// was replaced are dropped, and identified jars are added (one per
    /// project), so planning sees every mod the instance really has.
    pub registry: ModRegistry,
}

/// Reads `mods_dir` against the `saved` registry. Recognising loose jars is
/// best-effort: when the catalog cannot answer they are listed as unknown.
pub async fn read_mod_inventory<S: ModSource, P: PackFiles>(
    source: &S,
    files: &P,
    mods_dir: &Path,
    saved: &ModRegistry,
) -> Result<ModInventory, PackFileError> {
    let mut jars = Vec::new();
    for path in files.list_dir(mods_dir).await? {
        let Some((filename, enabled)) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(mod_jar_name)
        else {
            continue;
        };
        // A jar removed since the folder was listed is simply not there.
        let Some(sha512) = files.digest(&path, HashAlgorithm::Sha512).await? else {
            continue;
        };
        jars.push((filename.to_owned(), enabled, sha512.to_ascii_lowercase()));
    }

    let tracked = |filename: &str, sha512: &str| {
        saved
            .mods()
            .iter()
            .find(|installed| installed.filename == filename && installed.sha512 == sha512)
    };
    // A name the registry would refuse to load again is never recorded.
    let recordable = |filename: &str| RelativePath::is_file_name(filename);
    let loose: Vec<String> = jars
        .iter()
        .filter(|(filename, _, sha512)| tracked(filename, sha512).is_none())
        .map(|(_, _, sha512)| sha512.clone())
        .collect();
    let identified = identify(source, &loose).await;
    let titles = titles(source, identified.values()).await;

    let mut entries: Vec<ModEntry> = jars
        .into_iter()
        .map(|(filename, enabled, sha512)| {
            let (origin, record) = if let Some(record) = tracked(&filename, &sha512) {
                (ModOrigin::Launcher, Some(record.clone()))
            } else if let Some(version) = identified.get(&sha512).filter(|_| recordable(&filename))
            {
                let title = titles
                    .get(&version.project)
                    .cloned()
                    .unwrap_or_else(|| filename.clone());
                let record = adopted(version, title, &filename, &sha512);
                (ModOrigin::Identified, Some(record))
            } else {
                (ModOrigin::Unknown, None)
            };
            ModEntry {
                filename,
                enabled,
                sha512,
                origin,
                record,
                orphans: Vec::new(),
            }
        })
        .collect();

    // Launcher records first, so an identified copy never replaces one, and
    // of several identified copies of a project an enabled one.
    let mut registry = ModRegistry::default();
    for origin in [ModOrigin::Launcher, ModOrigin::Identified] {
        let mut candidates: Vec<&ModEntry> = entries
            .iter()
            .filter(|entry| entry.origin == origin)
            .collect();
        candidates.sort_by_key(|entry| !entry.enabled);
        for entry in candidates {
            if let Some(record) = &entry.record
                && registry.get(&record.project).is_none()
            {
                registry.insert(record.clone());
            }
        }
    }
    // A mod whose jar is gone no longer needs anything.
    let present: BTreeSet<String> = registry
        .mods()
        .iter()
        .map(|installed| installed.project.clone())
        .collect();
    let prune = |record: &mut InstalledMod| {
        record.required_by.retain(|needer| present.contains(needer));
    };
    let registry = ModRegistry::new(registry.mods().iter().cloned().map(|mut record| {
        prune(&mut record);
        record
    }));
    for entry in &mut entries {
        let Some(record) = entry.record.as_mut() else {
            continue;
        };
        prune(record);
        // Only the copy the registry keeps stands for its project.
        if registry
            .get(&record.project)
            .is_some_and(|kept| kept.filename == entry.filename)
        {
            entry.orphans = registry.orphans_after_removing(&record.project);
        }
    }
    entries.sort_by_cached_key(|entry| (entry.title().to_lowercase(), entry.filename.clone()));
    Ok(ModInventory { entries, registry })
}

/// Turns `mods_dir/<filename>` on or off by renaming it to or from
/// `<filename>.disabled`. Doing what is already done is not an error; a
/// different file already under the target name is.
pub async fn set_jar_enabled<P: PackFiles>(
    files: &P,
    mods_dir: &Path,
    filename: &str,
    enabled: bool,
) -> Result<(), PackFileError> {
    let on = mods_dir.join(filename);
    let off = mods_dir.join(format!("{filename}{DISABLED_SUFFIX}"));
    let (from, to) = if enabled { (off, on) } else { (on, off) };
    if files.digest(&from, HashAlgorithm::Sha512).await?.is_none() {
        return Ok(());
    }
    // Another jar under the target name is never replaced.
    if files.digest(&to, HashAlgorithm::Sha512).await?.is_some() {
        return Err(PackFileError::Io {
            path: to.display().to_string(),
            reason: "a file with this name already exists".into(),
        });
    }
    files.place(&from, &to).await?;
    files.remove_file(&from).await
}

/// `filename` as a single safe mod jar name in its enabled form (`x.jar`),
/// the name the player-facing use cases take.
pub fn checked_jar(filename: &str) -> Option<RelativePath> {
    let enabled_jar = mod_jar_name(filename) == Some((filename, true));
    RelativePath::parse(filename)
        .ok()
        .filter(|_| enabled_jar && RelativePath::is_file_name(filename))
}

/// The record of a jar the launcher did not install. The player is taken
/// to have wanted it, so it is never removed as an orphan.
fn adopted(version: &ModVersion, title: String, filename: &str, sha512: &str) -> InstalledMod {
    InstalledMod {
        project: version.project.clone(),
        title,
        version: version.id.clone(),
        version_number: version.version_number.clone(),
        filename: filename.to_owned(),
        sha512: sha512.to_owned(),
        requested: true,
        required_by: BTreeSet::new(),
        incompatible: version.incompatible_projects().map(str::to_owned).collect(),
    }
}

async fn identify<S: ModSource>(source: &S, sha512: &[String]) -> HashMap<String, ModVersion> {
    if sha512.is_empty() {
        return HashMap::new();
    }
    match source.identify(sha512).await {
        Ok(identified) => identified,
        Err(error) => {
            tracing::warn!(%error, "could not identify mod jars; listing them as unknown");
            HashMap::new()
        }
    }
}

async fn titles<'a, S: ModSource>(
    source: &S,
    versions: impl Iterator<Item = &'a ModVersion>,
) -> HashMap<String, String> {
    let ids: Vec<String> = versions
        .map(|version| version.project.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return HashMap::new();
    }
    match source.projects(&ids).await {
        Ok(summaries) => summaries
            .into_iter()
            .map(|summary| (summary.project, summary.title))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "could not read mod titles; using file names");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::testing::FakePackFiles;

    #[tokio::test]
    async fn toggles_a_jar_and_never_replaces_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let files = FakePackFiles::default();
        std::fs::write(dir.path().join("a.jar"), b"a").unwrap();
        set_jar_enabled(&files, dir.path(), "a.jar", false)
            .await
            .unwrap();
        assert!(!dir.path().join("a.jar").exists());
        assert_eq!(
            std::fs::read(dir.path().join("a.jar.disabled")).unwrap(),
            b"a"
        );
        // Already disabled: nothing to do.
        set_jar_enabled(&files, dir.path(), "a.jar", false)
            .await
            .unwrap();

        std::fs::write(dir.path().join("a.jar"), b"another a").unwrap();
        assert!(
            set_jar_enabled(&files, dir.path(), "a.jar", true)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(dir.path().join("a.jar")).unwrap(),
            b"another a"
        );
        assert!(dir.path().join("a.jar.disabled").exists());
    }
}
