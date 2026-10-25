//! What installing one mod takes: the mod, the dependencies it pulls in,
//! and the reasons it cannot be installed.

use std::collections::BTreeSet;

use thiserror::Error;

use super::{ModRegistry, ModVersion};
use crate::domain::modpack::ModpackSourceError;

/// The most mods one install may bring in, the requested one included. A
/// dependency chain longer than this is a broken catalog entry, not a mod.
pub const MAX_PLANNED_MODS: usize = 64;

/// One mod an install will write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMod {
    pub title: String,
    pub version: ModVersion,
    /// Whether it is the mod the player asked for.
    pub requested: bool,
    /// The planned mods that need it.
    pub required_by: BTreeSet<String>,
}

/// The mods an install writes, the requested one first, and the required
/// dependencies the instance already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInstallPlan {
    pub mods: Vec<PlannedMod>,
    /// `(dependency, needed by)` project pairs already installed.
    pub present: Vec<(String, String)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModPlanError {
    #[error("instances without a mod loader cannot run mods")]
    NoLoader,
    #[error("{title} is already installed")]
    AlreadyInstalled { title: String },
    #[error("{title} has no version for this instance's game version and loader")]
    NoCompatibleVersion { title: String },
    #[error("{title} can only be downloaded from its catalog's site: {page}")]
    ManualDownload { title: String, page: String },
    #[error("{title} cannot run together with {with}")]
    Incompatible { title: String, with: String },
    #[error("installing this mod would bring in more than {MAX_PLANNED_MODS} mods")]
    TooManyDependencies,
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
}

/// The first `(mod, other)` project pair that cannot run together, where
/// `mod` is planned and `other` is planned or installed. Either side may be
/// the one declaring the incompatibility.
pub fn find_conflict(plan: &[PlannedMod], installed: &ModRegistry) -> Option<(String, String)> {
    for planned in plan {
        let project = &planned.version.project;
        for other in planned.version.incompatible_projects() {
            let present = plan
                .iter()
                .any(|candidate| candidate.version.project == other)
                || installed.get(other).is_some();
            if present && other != project {
                return Some((project.clone(), other.to_owned()));
            }
        }
        for existing in installed.mods() {
            if existing.incompatible.contains(project) {
                return Some((project.clone(), existing.project.clone()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::domain::mods::{DependencyKind, InstalledMod, ModDependency, ReleaseChannel};

    fn planned(project: &str, incompatible: &[&str]) -> PlannedMod {
        PlannedMod {
            title: project.into(),
            version: ModVersion {
                id: format!("{project}-v"),
                project: project.into(),
                name: project.into(),
                version_number: "1".into(),
                channel: ReleaseChannel::Release,
                game_versions: Vec::new(),
                loaders: Vec::new(),
                published: None,
                file: None,
                manual_page: None,
                dependencies: incompatible
                    .iter()
                    .map(|other| ModDependency {
                        project: Some((*other).into()),
                        version: None,
                        kind: DependencyKind::Incompatible,
                    })
                    .collect(),
            },
            requested: true,
            required_by: BTreeSet::new(),
        }
    }

    fn installed(project: &str, incompatible: &[&str]) -> InstalledMod {
        InstalledMod {
            project: project.into(),
            title: project.into(),
            version: "v".into(),
            version_number: "1".into(),
            filename: format!("{project}.jar"),
            sha512: "a".repeat(128),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: incompatible
                .iter()
                .map(|other| (*other).to_owned())
                .collect(),
        }
    }

    #[test]
    fn a_planned_mod_declaring_an_installed_one_incompatible_conflicts() {
        let registry = ModRegistry::new([installed("optifine", &[])]);
        assert_eq!(
            find_conflict(&[planned("sodium", &["optifine"])], &registry),
            Some(("sodium".into(), "optifine".into()))
        );
    }

    #[test]
    fn an_installed_mod_declaring_a_planned_one_incompatible_conflicts() {
        let registry = ModRegistry::new([installed("optifine", &["sodium"])]);
        assert_eq!(
            find_conflict(&[planned("sodium", &[])], &registry),
            Some(("sodium".into(), "optifine".into()))
        );
    }

    #[test]
    fn two_planned_mods_can_conflict_with_each_other() {
        let plan = [planned("iris", &[]), planned("sodium", &["iris"])];
        assert_eq!(
            find_conflict(&plan, &ModRegistry::default()),
            Some(("sodium".into(), "iris".into()))
        );
    }

    #[test]
    fn incompatibilities_with_absent_mods_do_not_matter() {
        let registry = ModRegistry::new([installed("lithium", &["phosphor"])]);
        let plan = [planned("sodium", &["optifine"])];
        assert_eq!(find_conflict(&plan, &registry), None);
    }
}
