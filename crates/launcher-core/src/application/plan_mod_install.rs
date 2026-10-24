//! What installing one mod into an instance would take: the version to
//! install, the required dependencies it pulls in, and whether anything
//! installed stands in the way. Nothing is written.

use std::collections::{BTreeSet, HashMap};

use crate::domain::modpack::ModpackSourceError;
use crate::domain::mods::{
    DependencyKind, MAX_PLANNED_MODS, ModDependency, ModInstallPlan, ModPlanError, ModRegistry,
    ModSource, ModTarget, ModVersion, PlannedMod, find_conflict, newest_compatible,
};

/// Plans installing `project` for `target` next to the `installed` mods.
///
/// Each required dependency is resolved once: a mod already installed or
/// already planned is reused; a pinned version is taken when it fits the
/// target, else the newest version of its project that does. Optional and
/// embedded dependencies are left alone.
pub async fn plan_mod_install<S: ModSource>(
    source: &S,
    target: &ModTarget,
    installed: &ModRegistry,
    project: &str,
) -> Result<ModInstallPlan, ModPlanError> {
    if target.accepted_loaders().is_empty() {
        return Err(ModPlanError::NoLoader);
    }
    if let Some(existing) = installed.get(project) {
        return Err(ModPlanError::AlreadyInstalled {
            title: existing.title.clone(),
        });
    }
    let root = newest(source, target, project).await?;
    // Each planned version with the planned mods that need it; the first is
    // the one the player asked for.
    let mut planned: Vec<(ModVersion, BTreeSet<String>)> = vec![(root, BTreeSet::new())];
    let mut present = Vec::new();

    let mut next = 0;
    while let Some((version, _)) = planned.get(next) {
        let needer = version.project.clone();
        let required: Vec<ModDependency> = version
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind == DependencyKind::Required)
            .cloned()
            .collect();
        next += 1;
        for dependency in required {
            let already = |project: &str| {
                installed.get(project).is_some()
                    || planned
                        .iter()
                        .any(|(version, _)| version.project == project)
            };
            // A dependency named only by version is known once it is fetched.
            let fetched = match &dependency.project {
                Some(project) if already(project) => None,
                _ => Some(resolve(source, target, &dependency).await?),
            };
            let Some(project) = fetched
                .as_ref()
                .map(|version| version.project.clone())
                .or_else(|| dependency.project.clone())
            else {
                continue;
            };
            if installed.get(&project).is_some() {
                present.push((project, needer.clone()));
            } else if let Some((_, needers)) = planned
                .iter_mut()
                .find(|(version, _)| version.project == project)
            {
                if project != needer {
                    needers.insert(needer.clone());
                }
            } else if let Some(version) = fetched {
                if planned.len() == MAX_PLANNED_MODS {
                    return Err(ModPlanError::TooManyDependencies);
                }
                planned.push((version, BTreeSet::from([needer.clone()])));
            }
        }
    }

    let ids: Vec<String> = planned
        .iter()
        .map(|(version, _)| version.project.clone())
        .collect();
    let titles = titles(source, &ids).await;
    let title_of = |project: &str| {
        titles
            .get(project)
            .cloned()
            .or_else(|| {
                installed
                    .get(project)
                    .map(|existing| existing.title.clone())
            })
            .unwrap_or_else(|| project.to_owned())
    };
    let mods: Vec<PlannedMod> = planned
        .into_iter()
        .enumerate()
        .map(|(index, (version, required_by))| PlannedMod {
            title: title_of(&version.project),
            version,
            requested: index == 0,
            required_by,
        })
        .collect();
    if let Some((project, other)) = find_conflict(&mods, installed) {
        return Err(ModPlanError::Incompatible {
            title: title_of(&project),
            with: title_of(&other),
        });
    }
    Ok(ModInstallPlan { mods, present })
}

/// The version to install for one required dependency.
async fn resolve<S: ModSource>(
    source: &S,
    target: &ModTarget,
    dependency: &ModDependency,
) -> Result<ModVersion, ModPlanError> {
    let mut project = dependency.project.clone();
    if let Some(id) = &dependency.version {
        match source.version(id).await {
            Ok(pinned) => {
                if pinned.file.is_some() && target.accepts(&pinned) {
                    return Ok(pinned);
                }
                project.get_or_insert(pinned.project);
            }
            // The author removed the pinned version; the project may still fit.
            Err(ModpackSourceError::NotFound(_)) if project.is_some() => {}
            Err(error) => return Err(error.into()),
        }
    }
    match project {
        Some(project) => newest(source, target, &project).await,
        // Unreachable for catalog data: a dependency names a project or a version.
        None => Err(ModPlanError::NoCompatibleVersion {
            title: String::new(),
        }),
    }
}

/// The newest installable version of `project` for `target`.
async fn newest<S: ModSource>(
    source: &S,
    target: &ModTarget,
    project: &str,
) -> Result<ModVersion, ModPlanError> {
    let versions = source.versions(project, target).await?;
    match newest_compatible(&versions, target) {
        Some(version) => Ok(version.clone()),
        None => {
            let title = titles(source, &[project.to_owned()])
                .await
                .remove(project)
                .unwrap_or_else(|| project.to_owned());
            Err(ModPlanError::NoCompatibleVersion { title })
        }
    }
}

/// Display names only: when the catalog cannot answer, callers fall back to
/// installed titles or project ids rather than refusing the install.
async fn titles<S: ModSource>(source: &S, projects: &[String]) -> HashMap<String, String> {
    match source.projects(projects).await {
        Ok(summaries) => summaries
            .into_iter()
            .map(|summary| (summary.project, summary.title))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "could not read mod titles; using project ids");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::instance::GameVersion;
    use crate::domain::loader::LoaderKind::{Fabric, NeoForge, Quilt, Vanilla};
    use crate::domain::mods::{DependencyKind::*, InstalledMod};
    use crate::testing::{FakeModSource, depends, mod_version};

    fn target(loader: crate::domain::loader::LoaderKind) -> ModTarget {
        ModTarget {
            game_version: GameVersion::parse("1.21.1").unwrap(),
            loader,
        }
    }

    fn installed(project: &str, title: &str) -> InstalledMod {
        InstalledMod {
            project: project.into(),
            title: title.into(),
            version: "v".into(),
            version_number: "1".into(),
            filename: format!("{project}.jar"),
            sha512: "a".repeat(128),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: BTreeSet::new(),
        }
    }

    fn projects(plan: &ModInstallPlan) -> Vec<(&str, &str, Option<&str>)> {
        plan.mods
            .iter()
            .map(|planned| {
                (
                    planned.version.project.as_str(),
                    planned.version.id.as_str(),
                    planned.required_by.iter().next().map(String::as_str),
                )
            })
            .collect()
    }

    /// Iris needs Sodium and suggests Mod Menu; Sodium needs Fabric API.
    fn catalog() -> FakeModSource {
        FakeModSource::default()
            .with(
                "iris",
                "Iris Shaders",
                vec![
                    depends(
                        depends(
                            mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                            Required,
                            Some("sodium"),
                            None,
                        ),
                        Optional,
                        Some("modmenu"),
                        None,
                    ),
                    mod_version("iris", "iris-old", "1.20.1", &[Fabric]),
                ],
            )
            .with(
                "sodium",
                "Sodium",
                vec![
                    depends(
                        mod_version("sodium", "sodium-2", "1.21.1", &[Fabric]),
                        Required,
                        Some("fabric-api"),
                        None,
                    ),
                    mod_version("sodium", "sodium-neo", "1.21.1", &[NeoForge]),
                ],
            )
            .with(
                "fabric-api",
                "Fabric API",
                vec![mod_version("fabric-api", "fapi-1", "1.21.1", &[Fabric])],
            )
            .with(
                "modmenu",
                "Mod Menu",
                vec![mod_version("modmenu", "modmenu-1", "1.21.1", &[Fabric])],
            )
    }

    #[tokio::test]
    async fn plans_the_mod_and_its_required_chain_but_not_optional_ones() {
        let plan = plan_mod_install(&catalog(), &target(Quilt), &ModRegistry::default(), "iris")
            .await
            .unwrap();
        assert_eq!(
            projects(&plan),
            [
                ("iris", "iris-2", None),
                ("sodium", "sodium-2", Some("iris")),
                ("fabric-api", "fapi-1", Some("sodium")),
            ]
        );
        assert_eq!(plan.mods[0].title, "Iris Shaders");
        assert_eq!(plan.mods[2].title, "Fabric API");
        assert!(plan.present.is_empty());
    }

    #[tokio::test]
    async fn reuses_required_mods_already_installed() {
        let registry = ModRegistry::new([installed("sodium", "Sodium")]);
        let source = catalog();
        let plan = plan_mod_install(&source, &target(Fabric), &registry, "iris")
            .await
            .unwrap();
        assert_eq!(projects(&plan), [("iris", "iris-2", None)]);
        assert_eq!(plan.present, [("sodium".to_owned(), "iris".to_owned())]);
        assert_eq!(source.asked(), ["iris"]);
    }

    #[tokio::test]
    async fn takes_a_pinned_dependency_when_it_fits_and_the_newest_one_otherwise() {
        let pinned = mod_version("sodium", "sodium-pinned", "1.21.1", &[Fabric]);
        let foreign = mod_version("sodium", "sodium-forge", "1.21.1", &[NeoForge]);
        let source = |pin: &str| {
            FakeModSource::default()
                .with(
                    "iris",
                    "Iris Shaders",
                    vec![depends(
                        mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                        Required,
                        Some("sodium"),
                        Some(pin),
                    )],
                )
                .with(
                    "sodium",
                    "Sodium",
                    vec![
                        mod_version("sodium", "sodium-2", "1.21.1", &[Fabric]),
                        pinned.clone(),
                        foreign.clone(),
                    ],
                )
        };
        let fits = plan_mod_install(
            &source("sodium-pinned"),
            &target(Fabric),
            &ModRegistry::default(),
            "iris",
        )
        .await
        .unwrap();
        assert_eq!(fits.mods[1].version.id, "sodium-pinned");
        let other_loader = plan_mod_install(
            &source("sodium-forge"),
            &target(Fabric),
            &ModRegistry::default(),
            "iris",
        )
        .await
        .unwrap();
        assert_eq!(other_loader.mods[1].version.id, "sodium-2");
    }

    #[tokio::test]
    async fn a_dependency_named_only_by_version_is_matched_by_its_project() {
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris Shaders",
                vec![depends(
                    mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                    Required,
                    None,
                    Some("sodium-2"),
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version("sodium", "sodium-2", "1.21.1", &[Fabric])],
            );
        let fresh = plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "iris")
            .await
            .unwrap();
        assert_eq!(projects(&fresh)[1], ("sodium", "sodium-2", Some("iris")));

        let registry = ModRegistry::new([installed("sodium", "Sodium")]);
        let present = plan_mod_install(&source, &target(Fabric), &registry, "iris")
            .await
            .unwrap();
        assert_eq!(present.mods.len(), 1);
        assert_eq!(present.present, [("sodium".to_owned(), "iris".to_owned())]);
    }

    #[tokio::test]
    async fn refuses_vanilla_installed_mods_and_mods_without_a_fitting_version() {
        let source = catalog();
        assert_eq!(
            plan_mod_install(&source, &target(Vanilla), &ModRegistry::default(), "iris").await,
            Err(ModPlanError::NoLoader)
        );
        let registry = ModRegistry::new([installed("iris", "Iris Shaders")]);
        assert_eq!(
            plan_mod_install(&source, &target(Fabric), &registry, "iris").await,
            Err(ModPlanError::AlreadyInstalled {
                title: "Iris Shaders".into()
            })
        );
        assert_eq!(
            plan_mod_install(&source, &target(NeoForge), &ModRegistry::default(), "iris").await,
            Err(ModPlanError::NoCompatibleVersion {
                title: "Iris Shaders".into()
            })
        );
    }

    #[tokio::test]
    async fn names_a_required_dependency_with_no_fitting_version() {
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris Shaders",
                vec![depends(
                    mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                    Required,
                    Some("sodium"),
                    None,
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version("sodium", "sodium-old", "1.20.1", &[Fabric])],
            );
        assert_eq!(
            plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "iris").await,
            Err(ModPlanError::NoCompatibleVersion {
                title: "Sodium".into()
            })
        );
    }

    #[tokio::test]
    async fn refuses_a_mod_incompatible_with_an_installed_one_by_name() {
        let source = FakeModSource::default().with(
            "sodium",
            "Sodium",
            vec![depends(
                mod_version("sodium", "sodium-2", "1.21.1", &[Fabric]),
                Incompatible,
                Some("optifine"),
                None,
            )],
        );
        let registry = ModRegistry::new([installed("optifine", "OptiFine")]);
        assert_eq!(
            plan_mod_install(&source, &target(Fabric), &registry, "sodium").await,
            Err(ModPlanError::Incompatible {
                title: "Sodium".into(),
                with: "OptiFine".into()
            })
        );
    }

    #[tokio::test]
    async fn stops_a_dependency_chain_that_never_ends() {
        let mut source = FakeModSource::default();
        for n in 0..=MAX_PLANNED_MODS {
            let project = format!("mod{n}");
            let next = format!("mod{}", n + 1);
            source = source.with(
                &project,
                &project,
                vec![depends(
                    mod_version(&project, &format!("{project}-v"), "1.21.1", &[Fabric]),
                    Required,
                    Some(&next),
                    None,
                )],
            );
        }
        assert_eq!(
            plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "mod0").await,
            Err(ModPlanError::TooManyDependencies)
        );
    }

    #[tokio::test]
    async fn a_dependency_shared_by_two_planned_mods_records_both() {
        // Iris needs Sodium and Indium; Indium needs Sodium too.
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris",
                vec![depends(
                    depends(
                        mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                        Required,
                        Some("indium"),
                        None,
                    ),
                    Required,
                    Some("sodium"),
                    None,
                )],
            )
            .with(
                "indium",
                "Indium",
                vec![depends(
                    mod_version("indium", "indium-1", "1.21.1", &[Fabric]),
                    Required,
                    Some("sodium"),
                    None,
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version("sodium", "sodium-2", "1.21.1", &[Fabric])],
            );
        let plan = plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "iris")
            .await
            .unwrap();
        let sodium = plan
            .mods
            .iter()
            .find(|planned| planned.version.project == "sodium")
            .unwrap();
        assert_eq!(
            sodium.required_by,
            BTreeSet::from(["iris".to_owned(), "indium".to_owned()])
        );
        assert_eq!(plan.mods.len(), 3);
    }

    #[tokio::test]
    async fn a_removed_pinned_version_falls_back_to_the_newest_of_its_project() {
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris",
                vec![depends(
                    mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                    Required,
                    Some("sodium"),
                    Some("sodium-deleted"),
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version("sodium", "sodium-2", "1.21.1", &[Fabric])],
            );
        let plan = plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "iris")
            .await
            .unwrap();
        assert_eq!(
            projects(&plan),
            [
                ("iris", "iris-2", None),
                ("sodium", "sodium-2", Some("iris"))
            ]
        );

        // Named only by the removed version, the dependency cannot be found.
        let orphan = FakeModSource::default().with(
            "iris",
            "Iris",
            vec![depends(
                mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                Required,
                None,
                Some("sodium-deleted"),
            )],
        );
        assert!(matches!(
            plan_mod_install(&orphan, &target(Fabric), &ModRegistry::default(), "iris").await,
            Err(ModPlanError::Source(ModpackSourceError::NotFound(_)))
        ));
    }

    #[tokio::test]
    async fn titles_fall_back_to_project_ids_when_the_catalog_cannot_name_them() {
        let mut source = catalog();
        source.projects_fail = true;
        let plan = plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "iris")
            .await
            .unwrap();
        assert_eq!(plan.mods[0].title, "iris");
    }

    #[tokio::test]
    async fn dependency_cycles_end_once_every_mod_is_planned() {
        let source = FakeModSource::default()
            .with(
                "a",
                "A",
                vec![depends(
                    mod_version("a", "a-1", "1.21.1", &[Fabric]),
                    Required,
                    Some("b"),
                    None,
                )],
            )
            .with(
                "b",
                "B",
                vec![depends(
                    mod_version("b", "b-1", "1.21.1", &[Fabric]),
                    Required,
                    Some("a"),
                    None,
                )],
            );
        let plan = plan_mod_install(&source, &target(Fabric), &ModRegistry::default(), "a")
            .await
            .unwrap();
        // Each needs the other; only A was asked for.
        assert_eq!(
            projects(&plan),
            [("a", "a-1", Some("b")), ("b", "b-1", Some("a"))]
        );
        assert!(plan.mods[0].requested);
        assert!(!plan.mods[1].requested);
    }
}
