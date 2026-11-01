//! Deciding which pack files to write, keep or remove when a modpack is
//! installed or updated, and what an export puts in the index.
//!
//! The launcher records the files a pack installed (path and sha1) in a
//! [`PackManifest`]. A file is the pack's to replace or delete only while it is
//! intact, that is, while its content still matches that record. Anything the
//! player added or changed is kept.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use crate::domain::integrity::Checksum;
use crate::domain::launch::RelativePath;

use super::mrpack::{EnvSupport, MrpackFile, is_allowed_download, is_exportable};

/// The files a pack installed into an instance, by path, with their sha1
/// (lowercase hex), and the files still waiting for a manual download.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackManifest {
    pub files: BTreeMap<RelativePath, String>,
    /// The mod each recorded file belongs to (see [`IncomingFile::project`]),
    /// for the files whose mod is known.
    pub projects: BTreeMap<RelativePath, String>,
    /// Files the player must download by hand before the pack is complete.
    pub pending: Vec<ManualFile>,
}

/// A pack file the launcher may not download itself (CurseForge files it
/// cannot resolve, or whose author forbids third-party distribution): the
/// player downloads it from its page and the launcher picks it up from the
/// Downloads folder, verified with `checksum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualFile {
    /// Where it goes in the game folder.
    pub path: RelativePath,
    /// The mod's display name.
    pub name: String,
    /// The file name CurseForge serves it under (one path segment).
    pub filename: String,
    pub checksum: Checksum,
    pub project_id: u64,
    pub file_id: u64,
    /// Installed as `<path>.disabled`: a new file of a mod the player turned
    /// off, so it arrives turned off.
    pub disabled: bool,
}

impl ManualFile {
    /// The project page the player downloads it from.
    pub fn page_url(&self) -> String {
        format!("https://www.curseforge.com/projects/{}", self.project_id)
    }
}

/// Where a pack file comes from, for the shared sync pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    /// Fetched from the first URL and verified against `checksum`.
    Download {
        urls: Vec<String>,
        checksum: Checksum,
        size: Option<u64>,
    },
    /// Copied from a local file (a local pack folder) after verifying `checksum`.
    Local { path: PathBuf, checksum: Checksum },
    /// Already extracted into staging (an archive's overrides), with its sha1.
    Staged { path: PathBuf, sha1: String },
}

/// One file a pack wants in the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    pub path: RelativePath,
    pub source: EntrySource,
    /// Written once and never replaced by a later update (packwiz `preserve`).
    pub preserve: bool,
    /// The mod it belongs to, when the source names it; a Modrinth CDN
    /// download is identified by its URL otherwise (see [`PackEntry::project`]).
    pub project: Option<String>,
}

impl PackEntry {
    /// The mod this file belongs to, if known.
    pub fn project(&self) -> Option<String> {
        self.project.clone().or_else(|| match &self.source {
            EntrySource::Download { urls, .. } => {
                urls.first().and_then(|url| modrinth_project(url))
            }
            EntrySource::Local { .. } | EntrySource::Staged { .. } => None,
        })
    }
}

/// A file the new pack version wants at `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingFile {
    pub path: RelativePath,
    pub sha1: String,
    /// The mod the file belongs to, when known (`modrinth:<id>`,
    /// `curseforge:<id>`): it follows a mod the player turned off when a new
    /// version renames its file.
    pub project: Option<String>,
}

/// What to do with each file, and the manifest to save once it is done.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncPlan {
    /// Absent, or still the pack's intact previous version: write the new one.
    pub write: Vec<RelativePath>,
    /// Already holds the new content.
    pub unchanged: Vec<RelativePath>,
    /// The player added or changed it: left alone.
    pub kept: Vec<RelativePath>,
    /// The previous version installed it, the new one does not, and it is intact.
    pub remove: Vec<RelativePath>,
    /// The paths of `write`, `unchanged` and `remove` the player turned off:
    /// they apply to `<path>.disabled`, so a mod stays off across updates.
    pub disabled: BTreeSet<RelativePath>,
    pub manifest: PackManifest,
}

/// The Modrinth project a CDN download URL belongs to
/// (`https://cdn.modrinth.com/data/<project>/...`), as a mod identity.
pub fn modrinth_project(url: &str) -> Option<String> {
    let project = url
        .strip_prefix("https://cdn.modrinth.com/data/")?
        .split('/')
        .next()?;
    let valid = !project.is_empty()
        && project
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| format!("modrinth:{project}"))
}

/// The identity of a CurseForge project.
pub fn curseforge_project(project_id: u64) -> String {
    format!("curseforge:{project_id}")
}

/// The mods the player turned off whose file the new version no longer wants
/// (`is_wanted`), and those still waiting as a manual download marked to
/// arrive turned off: a new file of the same mod arrives turned off too.
pub fn turned_off_projects(
    previous: &PackManifest,
    is_wanted: impl Fn(&RelativePath) -> bool,
    disabled: &BTreeMap<RelativePath, String>,
) -> BTreeSet<String> {
    let renamed = previous
        .files
        .keys()
        .filter(|path| !is_wanted(path) && disabled.contains_key(*path))
        .filter_map(|path| previous.projects.get(path).cloned());
    let pending = previous
        .pending
        .iter()
        .filter(|file| file.disabled)
        .map(|file| curseforge_project(file.project_id));
    renamed.chain(pending).collect()
}

/// Plans an install (empty `previous`) or an update.
///
/// `on_disk` holds the current sha1 of every path in `previous` and `incoming`
/// (`None` or a missing key means the file does not exist). `disabled` holds
/// the sha1 of `<path>.disabled` for those paths that are absent because the
/// player turned them off. When `incoming` lists a path twice, the last entry
/// wins (overrides replace index files).
pub fn plan_sync(
    previous: &PackManifest,
    incoming: &[IncomingFile],
    on_disk: &BTreeMap<RelativePath, Option<String>>,
    disabled: &BTreeMap<RelativePath, String>,
) -> SyncPlan {
    let current = |path: &RelativePath| on_disk.get(path).cloned().flatten();
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let wanted: BTreeMap<&RelativePath, &IncomingFile> =
        incoming.iter().map(|file| (&file.path, file)).collect();
    let renamed_off = turned_off_projects(previous, |path| wanted.contains_key(path), disabled);

    let mut plan = SyncPlan::default();
    for (path, file) in &wanted {
        let sha1 = file.sha1.as_str();
        let intact = |disk: &str| previous.files.get(*path).is_some_and(|old| same(old, disk));
        let path = (*path).clone();
        match (current(&path), disabled.get(&path)) {
            (Some(disk), _) if same(&disk, sha1) => plan.unchanged.push(path.clone()),
            (Some(disk), _) if intact(&disk) => plan.write.push(path.clone()),
            (Some(_), _) => {
                plan.kept.push(path);
                continue;
            }
            (None, Some(off)) if same(off, sha1) => {
                plan.disabled.insert(path.clone());
                plan.unchanged.push(path.clone());
            }
            (None, Some(off)) if intact(off) => {
                plan.disabled.insert(path.clone());
                plan.write.push(path.clone());
            }
            (None, Some(_)) => {
                plan.kept.push(path);
                continue;
            }
            (None, None) => {
                if file
                    .project
                    .as_deref()
                    .is_some_and(|project| renamed_off.contains(project))
                {
                    plan.disabled.insert(path.clone());
                }
                plan.write.push(path.clone());
            }
        }
        if let Some(project) = &file.project {
            plan.manifest.projects.insert(path.clone(), project.clone());
        }
        plan.manifest.files.insert(path, sha1.to_ascii_lowercase());
    }
    for (path, old) in &previous.files {
        if wanted.contains_key(path) {
            continue;
        }
        match (current(path), disabled.get(path)) {
            (Some(disk), _) if same(&disk, old) => plan.remove.push(path.clone()),
            (Some(_), _) => plan.kept.push(path.clone()),
            (None, Some(off)) if same(off, old) => {
                plan.disabled.insert(path.clone());
                plan.remove.push(path.clone());
            }
            (None, Some(_)) => plan.kept.push(path.clone()),
            (None, None) => {}
        }
    }
    plan
}

/// A file found in the game folder for an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    pub path: RelativePath,
    pub sha1: Checksum,
    pub sha512: Checksum,
    pub size: u64,
}

/// Index entries (files the catalog can serve) and files to copy into `overrides/`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportPlan {
    pub files: Vec<MrpackFile>,
    pub overrides: Vec<RelativePath>,
}

/// Splits exportable local files into index entries, for those whose sha512
/// the catalog knows (`known` maps sha512 to download URLs), and overrides.
pub fn plan_export(local: &[LocalFile], known: &HashMap<String, Vec<String>>) -> ExportPlan {
    let mut plan = ExportPlan::default();
    for file in local.iter().filter(|file| is_exportable(&file.path)) {
        // Only hosts an import accepts, or the exported pack would not install.
        let urls: Vec<String> = known
            .get(file.sha512.value())
            .into_iter()
            .flatten()
            .filter(|url| is_allowed_download(url))
            .cloned()
            .collect();
        match (!urls.is_empty()).then_some(urls) {
            Some(urls) => plan.files.push(MrpackFile {
                path: file.path.clone(),
                sha1: file.sha1.clone(),
                sha512: file.sha512.clone(),
                size: file.size,
                downloads: urls,
                client: EnvSupport::Required,
                server: EnvSupport::Required,
            }),
            None => plan.overrides.push(file.path.clone()),
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::integrity::HashAlgorithm;

    fn path(raw: &str) -> RelativePath {
        RelativePath::parse(raw).unwrap()
    }

    fn manifest(entries: &[(&str, &str)]) -> PackManifest {
        PackManifest {
            files: entries
                .iter()
                .map(|(p, sha1)| (path(p), (*sha1).to_owned()))
                .collect(),
            ..PackManifest::default()
        }
    }

    fn incoming(entries: &[(&str, &str)]) -> Vec<IncomingFile> {
        entries
            .iter()
            .map(|(p, sha1)| IncomingFile {
                path: path(p),
                sha1: (*sha1).to_owned(),
                project: None,
            })
            .collect()
    }

    fn disk(entries: &[(&str, Option<&str>)]) -> BTreeMap<RelativePath, Option<String>> {
        entries
            .iter()
            .map(|(p, sha1)| (path(p), sha1.map(str::to_owned)))
            .collect()
    }

    fn paths(list: &[RelativePath]) -> Vec<&str> {
        list.iter().map(RelativePath::as_str).collect()
    }

    #[test]
    fn a_fresh_install_writes_everything_and_records_it() {
        let plan = plan_sync(
            &PackManifest::default(),
            &incoming(&[("mods/a.jar", "AA"), ("config/a.toml", "bb")]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(paths(&plan.write), ["config/a.toml", "mods/a.jar"]);
        assert!(plan.kept.is_empty() && plan.remove.is_empty() && plan.unchanged.is_empty());
        assert_eq!(
            plan.manifest,
            manifest(&[("mods/a.jar", "aa"), ("config/a.toml", "bb")])
        );
    }

    #[test]
    fn an_update_replaces_intact_files_and_keeps_player_changes() {
        let previous = manifest(&[
            ("mods/a.jar", "a1"),
            ("config/a.toml", "c1"),
            ("mods/same.jar", "s1"),
        ]);
        let plan = plan_sync(
            &previous,
            &incoming(&[
                ("mods/a.jar", "a2"),
                ("config/a.toml", "c2"),
                ("mods/same.jar", "s1"),
            ]),
            &disk(&[
                ("mods/a.jar", Some("a1")),
                ("config/a.toml", Some("edited")),
                ("mods/same.jar", Some("s1")),
            ]),
            &BTreeMap::new(),
        );
        assert_eq!(paths(&plan.write), ["mods/a.jar"]);
        assert_eq!(paths(&plan.kept), ["config/a.toml"]);
        assert_eq!(paths(&plan.unchanged), ["mods/same.jar"]);
        assert_eq!(
            plan.manifest,
            manifest(&[("mods/a.jar", "a2"), ("mods/same.jar", "s1")])
        );
    }

    #[test]
    fn an_update_removes_only_intact_dropped_files() {
        let previous = manifest(&[
            ("mods/old.jar", "o1"),
            ("config/old.toml", "c1"),
            ("mods/gone.jar", "g1"),
        ]);
        let plan = plan_sync(
            &previous,
            &[],
            &disk(&[
                ("mods/old.jar", Some("O1")),
                ("config/old.toml", Some("edited")),
            ]),
            &BTreeMap::new(),
        );
        assert_eq!(paths(&plan.remove), ["mods/old.jar"]);
        assert_eq!(paths(&plan.kept), ["config/old.toml"]);
        assert!(plan.manifest.files.is_empty());
    }

    #[test]
    fn files_the_player_added_are_never_touched() {
        let plan = plan_sync(
            &PackManifest::default(),
            &incoming(&[("mods/mine.jar", "pack")]),
            &disk(&[("mods/mine.jar", Some("player"))]),
            &BTreeMap::new(),
        );
        assert!(plan.write.is_empty());
        assert_eq!(paths(&plan.kept), ["mods/mine.jar"]);
        assert!(plan.manifest.files.is_empty());
    }

    #[test]
    fn a_later_entry_for_the_same_path_wins() {
        let plan = plan_sync(
            &PackManifest::default(),
            &incoming(&[("config/a.toml", "index"), ("config/a.toml", "override")]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(plan.manifest, manifest(&[("config/a.toml", "override")]));
    }

    fn off(entries: &[(&str, &str)]) -> BTreeMap<RelativePath, String> {
        entries
            .iter()
            .map(|(p, sha1)| (path(p), (*sha1).to_owned()))
            .collect()
    }

    #[test]
    fn a_turned_off_file_is_updated_in_place_and_stays_tracked() {
        let previous = manifest(&[("mods/a.jar", "a1"), ("mods/b.jar", "b1")]);
        let plan = plan_sync(
            &previous,
            &incoming(&[("mods/a.jar", "a2"), ("mods/b.jar", "b1")]),
            &BTreeMap::new(),
            &off(&[("mods/a.jar", "a1"), ("mods/b.jar", "b1")]),
        );
        assert_eq!(paths(&plan.write), ["mods/a.jar"]);
        assert_eq!(paths(&plan.unchanged), ["mods/b.jar"]);
        assert_eq!(
            plan.disabled
                .iter()
                .map(RelativePath::as_str)
                .collect::<Vec<_>>(),
            ["mods/a.jar", "mods/b.jar"]
        );
        assert_eq!(
            plan.manifest,
            manifest(&[("mods/a.jar", "a2"), ("mods/b.jar", "b1")])
        );
    }

    #[test]
    fn a_turned_off_file_the_player_changed_is_kept() {
        let plan = plan_sync(
            &manifest(&[("mods/a.jar", "a1")]),
            &incoming(&[("mods/a.jar", "a2")]),
            &BTreeMap::new(),
            &off(&[("mods/a.jar", "player")]),
        );
        assert_eq!(paths(&plan.kept), ["mods/a.jar"]);
        assert!(plan.write.is_empty() && plan.disabled.is_empty());
    }

    #[test]
    fn intact_turned_off_files_the_pack_drops_are_removed() {
        let plan = plan_sync(
            &manifest(&[("mods/a.jar", "a1"), ("mods/b.jar", "b1")]),
            &[],
            &BTreeMap::new(),
            &off(&[("mods/a.jar", "a1"), ("mods/b.jar", "player")]),
        );
        assert_eq!(paths(&plan.remove), ["mods/a.jar"]);
        assert_eq!(paths(&plan.kept), ["mods/b.jar"]);
        assert!(plan.disabled.contains(&path("mods/a.jar")));
    }

    #[test]
    fn a_renamed_file_of_a_turned_off_mod_arrives_turned_off() {
        let mut previous = manifest(&[("mods/a-1.jar", "a1"), ("mods/b-1.jar", "b1")]);
        previous.projects = [
            (path("mods/a-1.jar"), "modrinth:a".to_owned()),
            (path("mods/b-1.jar"), "modrinth:b".to_owned()),
        ]
        .into();
        let with_project = |p: &str, sha1: &str, project: &str| IncomingFile {
            path: path(p),
            sha1: sha1.to_owned(),
            project: Some(project.to_owned()),
        };
        let plan = plan_sync(
            &previous,
            &[
                with_project("mods/a-2.jar", "a2", "modrinth:a"),
                with_project("mods/b-2.jar", "b2", "modrinth:b"),
            ],
            &disk(&[("mods/b-1.jar", Some("b1"))]),
            &off(&[("mods/a-1.jar", "a1")]),
        );
        assert_eq!(paths(&plan.write), ["mods/a-2.jar", "mods/b-2.jar"]);
        assert_eq!(paths(&plan.remove), ["mods/a-1.jar", "mods/b-1.jar"]);
        assert_eq!(
            plan.disabled
                .iter()
                .map(RelativePath::as_str)
                .collect::<Vec<_>>(),
            ["mods/a-1.jar", "mods/a-2.jar"],
            "only the mod the player turned off"
        );
        assert_eq!(
            plan.manifest.projects.get(&path("mods/a-2.jar")).unwrap(),
            "modrinth:a"
        );
    }

    #[test]
    fn reads_the_modrinth_project_from_cdn_urls() {
        assert_eq!(
            modrinth_project("https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar"),
            Some("modrinth:AANobbMI".to_owned())
        );
        assert_eq!(
            modrinth_project("https://cdn.modrinth.com/data//a.jar"),
            None
        );
        assert_eq!(
            modrinth_project("https://edge.forgecdn.net/files/1/2/a.jar"),
            None
        );
        assert_eq!(
            modrinth_project("https://cdn.modrinth.com/data/a%2F/x.jar"),
            None
        );
    }

    fn local(raw: &str, sha512_char: char) -> LocalFile {
        LocalFile {
            path: path(raw),
            sha1: Checksum::parse(HashAlgorithm::Sha1, &"1".repeat(40)).unwrap(),
            sha512: Checksum::parse(HashAlgorithm::Sha512, &sha512_char.to_string().repeat(128))
                .unwrap(),
            size: 3,
        }
    }

    #[test]
    fn exports_catalog_files_on_disallowed_hosts_as_overrides() {
        let known = HashMap::from([
            (
                "a".repeat(128),
                vec!["https://evil.example/a.jar".to_owned()],
            ),
            (
                "b".repeat(128),
                vec![
                    "https://evil.example/b.jar".to_owned(),
                    "https://cdn.modrinth.com/b.jar".to_owned(),
                ],
            ),
        ]);
        let plan = plan_export(
            &[local("mods/a.jar", 'a'), local("mods/b.jar", 'b')],
            &known,
        );
        assert_eq!(paths(&plan.overrides), ["mods/a.jar"]);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].downloads, ["https://cdn.modrinth.com/b.jar"]);
    }

    #[test]
    fn exports_known_files_as_entries_and_the_rest_as_overrides() {
        let known = HashMap::from([(
            "a".repeat(128),
            vec!["https://cdn.modrinth.com/a.jar".to_owned()],
        )]);
        let plan = plan_export(
            &[
                local("mods/a.jar", 'a'),
                local("mods/custom.jar", 'b'),
                local("config/x.toml", 'c'),
                local("saves/w/level.dat", 'a'),
            ],
            &known,
        );
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].path.as_str(), "mods/a.jar");
        assert_eq!(plan.files[0].downloads, ["https://cdn.modrinth.com/a.jar"]);
        assert_eq!(paths(&plan.overrides), ["mods/custom.jar", "config/x.toml"]);
    }
}
