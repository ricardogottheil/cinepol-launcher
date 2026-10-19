//! Finishes a pack install with the files the player downloaded by hand.
//!
//! For each file still pending in the instance's manifest, the Downloads
//! folder is searched for a file with its name (or the name a browser gives a
//! second copy, `name (1).jar`). A candidate is used only when its hash
//! matches the one the pack declares; it is copied into the game folder and
//! the original stays in the Downloads folder.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::application::modpack_files::has_checksum;
use crate::domain::instance::InstanceId;
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::sync::ManualFile;
use crate::domain::modpack::{PackFileError, PackFiles, PackManifestStore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ResolveManualDownloadsError {
    #[error(transparent)]
    Files(#[from] PackFileError),
}

/// Everything the resolver talks to.
pub struct ResolveManualDownloadsDeps<'a, P, M> {
    pub files: &'a P,
    pub manifests: &'a M,
    pub paths: &'a LauncherPaths,
}

/// The pending files that are now installed, and those still missing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManualDownloads {
    pub resolved: Vec<ManualFile>,
    pub remaining: Vec<ManualFile>,
}

/// Looks for the instance's pending files in `downloads` and installs every
/// one it can verify. The manifest is saved only when something was resolved.
pub async fn resolve_manual_downloads<P, M>(
    id: &InstanceId,
    downloads: &Path,
    deps: &ResolveManualDownloadsDeps<'_, P, M>,
) -> Result<ManualDownloads, ResolveManualDownloadsError>
where
    P: PackFiles,
    M: PackManifestStore,
{
    let mut manifest = deps.manifests.load(id).await?;
    if manifest.pending.is_empty() {
        return Ok(ManualDownloads::default());
    }
    let found = deps.files.list_dir(downloads).await?;
    let game_dir = deps.paths.game_dir(id);
    let mut outcome = ManualDownloads::default();
    for file in std::mem::take(&mut manifest.pending) {
        let target = file.path.under(&game_dir);
        // The player may have put it in the game folder directly.
        let mut installed = has_checksum(deps.files, &target, &file.checksum).await?;
        // A different file already at the path is the player's: never replace
        // it (the file stays pending, like a sync keeps player changes).
        let free = installed || deps.files.sha1(&target).await?.is_none();
        if !installed
            && free
            && let Some(candidate) = find(deps.files, &found, &file).await?
        {
            deps.files.place(&candidate, &target).await?;
            // The download may have changed between the check and the copy.
            installed = has_checksum(deps.files, &target, &file.checksum).await?;
            if !installed {
                deps.files.remove_file(&target).await?;
            }
        }
        match deps.files.sha1(&target).await? {
            Some(sha1) if installed => {
                manifest.files.insert(file.path.clone(), sha1);
                outcome.resolved.push(file);
            }
            _ => outcome.remaining.push(file),
        }
    }
    if !outcome.resolved.is_empty() {
        manifest.pending = outcome.remaining.clone();
        deps.manifests.save(id, &manifest).await?;
    }
    Ok(outcome)
}

/// The first candidate for `file` whose hash matches, exact names first.
async fn find<P: PackFiles>(
    files: &P,
    found: &[PathBuf],
    file: &ManualFile,
) -> Result<Option<PathBuf>, PackFileError> {
    let named = |path: &&PathBuf| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| (name == file.filename, is_candidate(name, &file.filename)))
    };
    let mut candidates: Vec<(bool, &PathBuf)> = found
        .iter()
        .filter_map(|path| match named(&path) {
            Some((exact, true)) => Some((exact, path)),
            _ => None,
        })
        .collect();
    candidates.sort_by_key(|(exact, _)| !exact);
    for (_, candidate) in candidates {
        if has_checksum(files, candidate, &file.checksum).await? {
            return Ok(Some(candidate.clone()));
        }
    }
    Ok(None)
}

/// `name` is `wanted`, or `wanted` with a browser's duplicate suffix before
/// the extension: `jei (1).jar` or `jei(2).jar` for `jei.jar`.
fn is_candidate(name: &str, wanted: &str) -> bool {
    if name == wanted {
        return true;
    }
    let (stem, extension) = match wanted.rfind('.') {
        Some(dot) if dot > 0 => wanted.split_at(dot),
        _ => (wanted, ""),
    };
    let Some(suffix) = name
        .strip_prefix(stem)
        .and_then(|rest| rest.strip_suffix(extension))
    else {
        return false;
    };
    let suffix = suffix.strip_prefix(' ').unwrap_or(suffix);
    suffix
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::modpack_files::fixtures::{World, rel};
    use crate::domain::integrity::{Checksum, HashAlgorithm, curseforge_fingerprint};
    use crate::domain::modpack::sync::PackManifest;
    use crate::testing::sha1_hex;

    const ID: &str = "cinepol-pack";

    fn id() -> InstanceId {
        InstanceId::parse(ID).unwrap()
    }

    fn manual(path: &str, bytes: &[u8]) -> ManualFile {
        ManualFile {
            path: rel(path),
            name: path.into(),
            filename: path.rsplit('/').next().unwrap().into(),
            checksum: Checksum::parse(
                HashAlgorithm::Murmur2,
                &curseforge_fingerprint(bytes).to_string(),
            )
            .unwrap(),
            project_id: 238_222,
            file_id: 1,
        }
    }

    fn world_with(pending: Vec<ManualFile>) -> World {
        let world = World::new(vec![]);
        world.manifests.manifests.lock().unwrap().insert(
            id(),
            PackManifest {
                files: [(rel("mods/sodium.jar"), sha1_hex(b"sodium"))].into(),
                pending,
            },
        );
        world
    }

    fn downloads(world: &World, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = world.paths.temp_dir().join("Downloads");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        dir
    }

    async fn resolve(world: &World, dir: &Path) -> ManualDownloads {
        let deps = ResolveManualDownloadsDeps {
            files: &world.files,
            manifests: &world.manifests,
            paths: &world.paths,
        };
        resolve_manual_downloads(&id(), dir, &deps).await.unwrap()
    }

    fn manifest(world: &World) -> PackManifest {
        world.manifests.manifests.lock().unwrap()[&id()].clone()
    }

    #[tokio::test]
    async fn installs_exact_and_duplicate_named_downloads_and_keeps_the_originals() {
        let jei = manual("mods/jei.jar", b"jei");
        let iris = manual("mods/iris.jar", b"iris");
        let world = world_with(vec![jei.clone(), iris.clone()]);
        let dir = downloads(
            &world,
            &[
                ("jei.jar", b"jei"),
                ("iris (1).jar", b"iris"),
                ("unrelated.jar", b"iris"),
            ],
        );

        let outcome = resolve(&world, &dir).await;

        assert_eq!(outcome.resolved, [jei, iris]);
        assert!(outcome.remaining.is_empty());
        assert_eq!(world.read_game_file(ID, "mods/jei.jar").unwrap(), b"jei");
        assert_eq!(world.read_game_file(ID, "mods/iris.jar").unwrap(), b"iris");
        assert!(dir.join("jei.jar").exists() && dir.join("iris (1).jar").exists());
        let manifest = manifest(&world);
        assert!(manifest.pending.is_empty());
        assert_eq!(manifest.files[&rel("mods/jei.jar")], sha1_hex(b"jei"));
        assert_eq!(manifest.files[&rel("mods/iris.jar")], sha1_hex(b"iris"));
        assert_eq!(manifest.files[&rel("mods/sodium.jar")], sha1_hex(b"sodium"));
    }

    #[tokio::test]
    async fn ignores_a_download_with_the_wrong_hash() {
        let jei = manual("mods/jei.jar", b"jei");
        let world = world_with(vec![jei.clone()]);
        let dir = downloads(&world, &[("jei.jar", b"not jei"), ("jei(2).jar", b"jei")]);
        let outcome = resolve(&world, &dir).await;
        assert_eq!(outcome.resolved, std::slice::from_ref(&jei));
        assert_eq!(world.read_game_file(ID, "mods/jei.jar").unwrap(), b"jei");

        let world = world_with(vec![jei.clone()]);
        let dir = downloads(&world, &[("jei.jar", b"not jei")]);
        let before = manifest(&world);
        let outcome = resolve(&world, &dir).await;
        assert!(outcome.resolved.is_empty());
        assert_eq!(outcome.remaining, [jei]);
        assert!(world.read_game_file(ID, "mods/jei.jar").is_none());
        assert_eq!(manifest(&world), before);
    }

    #[tokio::test]
    async fn a_file_already_in_the_game_folder_counts_as_installed() {
        let jei = manual("mods/jei.jar", b"jei");
        let world = world_with(vec![jei.clone()]);
        world.write_game_file(ID, "mods/jei.jar", b"jei");
        let outcome = resolve(&world, &world.paths.temp_dir().join("Missing")).await;
        assert_eq!(outcome.resolved, [jei]);
        assert!(manifest(&world).pending.is_empty());
    }

    #[test]
    fn matches_browser_duplicate_names_only() {
        for name in ["jei.jar", "jei (1).jar", "jei(2).jar", "jei (12).jar"] {
            assert!(is_candidate(name, "jei.jar"), "{name}");
        }
        for name in [
            "jei.zip",
            "jei-1.jar",
            "jei ().jar",
            "jei (a).jar",
            "jei  (1).jar",
            "xjei.jar",
            "jei.jar.part",
        ] {
            assert!(!is_candidate(name, "jei.jar"), "{name}");
        }
        assert!(is_candidate("README (3)", "README"));
    }

    #[tokio::test]
    async fn never_replaces_a_different_file_the_player_put_at_the_path() {
        let jei = manual("mods/jei.jar", b"jei");
        let world = world_with(vec![jei.clone()]);
        world.write_game_file(ID, "mods/jei.jar", b"player own jar");
        let dir = downloads(&world, &[("jei.jar", b"jei")]);

        let outcome = resolve(&world, &dir).await;

        assert!(outcome.resolved.is_empty());
        assert_eq!(outcome.remaining, [jei]);
        assert_eq!(
            world.read_game_file(ID, "mods/jei.jar").as_deref(),
            Some(b"player own jar".as_slice())
        );
    }
}
