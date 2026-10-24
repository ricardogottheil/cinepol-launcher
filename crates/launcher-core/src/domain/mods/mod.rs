//! Individual mods: catalog search, versions and their dependencies, and
//! which versions fit an instance.

mod ports;

pub use ports::ModSource;

use crate::domain::instance::GameVersion;
use crate::domain::integrity::Checksum;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::ModpackQuery;

/// What a mod has to run on: an instance's Minecraft version and loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModTarget {
    pub game_version: GameVersion,
    pub loader: LoaderKind,
}

impl ModTarget {
    /// The loaders whose mods run on this target, its own first. Quilt also
    /// loads Fabric mods; Vanilla loads none.
    pub fn accepted_loaders(&self) -> Vec<LoaderKind> {
        match self.loader {
            LoaderKind::Vanilla => Vec::new(),
            LoaderKind::Quilt => vec![LoaderKind::Quilt, LoaderKind::Fabric],
            other => vec![other],
        }
    }

    /// Whether `version` was built for this Minecraft version and a loader
    /// this target runs.
    pub fn accepts(&self, version: &ModVersion) -> bool {
        let loaders = self.accepted_loaders();
        version.game_versions.contains(&self.game_version)
            && version
                .loaders
                .iter()
                .any(|loader| loaders.contains(loader))
    }
}

/// A mod search, always for one target; `page` is zero-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModQuery {
    pub text: String,
    pub target: ModTarget,
    pub page: u32,
    pub page_size: u32,
}

impl ModQuery {
    /// Clamps the page size like a modpack search and trims the text.
    pub fn normalized(mut self) -> Self {
        self.page_size = self.page_size.clamp(1, ModpackQuery::MAX_PAGE_SIZE);
        self.text = self.text.trim().to_owned();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModSummary {
    pub project: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModPage {
    pub items: Vec<ModSummary>,
    pub total: Option<u64>,
}

/// How finished the author says a version is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
    Release,
    Beta,
    Alpha,
}

/// One release of a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModVersion {
    pub id: String,
    pub project: String,
    pub name: String,
    /// The version label the author gave it (`1.8.8+1.21.1-fabric`).
    pub version_number: String,
    pub channel: ReleaseChannel,
    pub game_versions: Vec<GameVersion>,
    pub loaders: Vec<LoaderKind>,
    /// RFC 3339 publication time, when the catalog reports it.
    pub published: Option<String>,
    /// The jar to install, when the version has one this launcher accepts.
    pub file: Option<ModFile>,
    pub dependencies: Vec<ModDependency>,
}

/// A downloadable mod jar and the hashes it is verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub sha512: Checksum,
    pub sha1: Option<Checksum>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// The mod does not run without it.
    Required,
    /// The mod works better with it.
    Optional,
    /// The mod breaks when both are installed.
    Incompatible,
    /// Already bundled inside the mod's jar.
    Embedded,
}

/// Another mod a version depends on: a project, a pinned version of it, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModDependency {
    pub project: Option<String>,
    pub version: Option<String>,
    pub kind: DependencyKind,
}

/// The version to install for `target` from a newest-first list: the newest
/// release that fits and has a file, else the newest beta or alpha that does.
pub fn newest_compatible<'a>(
    versions: &'a [ModVersion],
    target: &ModTarget,
) -> Option<&'a ModVersion> {
    let installable = || {
        versions
            .iter()
            .filter(|version| version.file.is_some() && target.accepts(version))
    };
    installable()
        .find(|version| version.channel == ReleaseChannel::Release)
        .or_else(|| installable().next())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::domain::integrity::HashAlgorithm;

    fn target(game: &str, loader: LoaderKind) -> ModTarget {
        ModTarget {
            game_version: GameVersion::parse(game).unwrap(),
            loader,
        }
    }

    fn version(
        id: &str,
        channel: ReleaseChannel,
        games: &[&str],
        loaders: &[LoaderKind],
    ) -> ModVersion {
        ModVersion {
            id: id.into(),
            project: "p".into(),
            name: id.into(),
            version_number: id.into(),
            channel,
            game_versions: games
                .iter()
                .map(|game| GameVersion::parse(game).unwrap())
                .collect(),
            loaders: loaders.to_vec(),
            published: None,
            file: Some(ModFile {
                url: format!("https://cdn.modrinth.com/{id}.jar"),
                filename: format!("{id}.jar"),
                size: 1,
                sha512: Checksum::parse(HashAlgorithm::Sha512, &"a".repeat(128)).unwrap(),
                sha1: None,
            }),
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn quilt_also_runs_fabric_mods_and_vanilla_runs_none() {
        use LoaderKind::{Fabric, Forge, NeoForge, Quilt, Vanilla};
        assert_eq!(target("1.21.1", Quilt).accepted_loaders(), [Quilt, Fabric]);
        assert_eq!(target("1.21.1", Fabric).accepted_loaders(), [Fabric]);
        assert_eq!(target("1.21.1", Forge).accepted_loaders(), [Forge]);
        assert_eq!(target("1.21.1", NeoForge).accepted_loaders(), [NeoForge]);
        assert!(target("1.21.1", Vanilla).accepted_loaders().is_empty());
    }

    #[test]
    fn a_version_fits_only_its_minecraft_versions_and_loaders() {
        use LoaderKind::{Fabric, NeoForge, Quilt};
        let fabric = version("a", ReleaseChannel::Release, &["1.21", "1.21.1"], &[Fabric]);
        assert!(target("1.21.1", Fabric).accepts(&fabric));
        assert!(target("1.21.1", Quilt).accepts(&fabric));
        assert!(!target("1.21.4", Fabric).accepts(&fabric));
        assert!(!target("1.21.1", NeoForge).accepts(&fabric));
    }

    #[test]
    fn the_newest_fitting_release_wins_over_newer_betas() {
        use LoaderKind::{Fabric, NeoForge};
        let versions = [
            version("beta", ReleaseChannel::Beta, &["1.21.1"], &[Fabric]),
            version(
                "other-loader",
                ReleaseChannel::Release,
                &["1.21.1"],
                &[NeoForge],
            ),
            version("release", ReleaseChannel::Release, &["1.21.1"], &[Fabric]),
            version("old", ReleaseChannel::Release, &["1.21.1"], &[Fabric]),
        ];
        let chosen = newest_compatible(&versions, &target("1.21.1", Fabric)).unwrap();
        assert_eq!(chosen.id, "release");
    }

    #[test]
    fn a_beta_is_chosen_when_no_release_fits_and_fileless_versions_never_are() {
        use LoaderKind::Fabric;
        let mut no_file = version("no-file", ReleaseChannel::Release, &["1.21.1"], &[Fabric]);
        no_file.file = None;
        let versions = [
            no_file,
            version("alpha", ReleaseChannel::Alpha, &["1.21.1"], &[Fabric]),
            version("beta", ReleaseChannel::Beta, &["1.21.1"], &[Fabric]),
        ];
        let chosen = newest_compatible(&versions, &target("1.21.1", Fabric)).unwrap();
        assert_eq!(chosen.id, "alpha");
        assert!(newest_compatible(&versions, &target("1.20.1", Fabric)).is_none());
    }
}
