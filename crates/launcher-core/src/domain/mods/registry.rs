//! The mods the launcher installed into an instance, and why each is there.

use std::collections::BTreeSet;

/// A mod the launcher installed into an instance's `mods/` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledMod {
    pub project: String,
    pub title: String,
    /// The installed version id and its label.
    pub version: String,
    pub version_number: String,
    /// The jar's name inside `mods/`, a single safe file name.
    pub filename: String,
    /// Lowercase hex sha512 of the jar as installed.
    pub sha512: String,
    /// Whether the player asked for it, rather than another mod needing it.
    pub requested: bool,
    /// Installed mods that need this one.
    pub required_by: BTreeSet<String>,
    /// Projects this version declares it cannot run with.
    pub incompatible: BTreeSet<String>,
}

/// Every mod the launcher installed into one instance, one per project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModRegistry {
    mods: Vec<InstalledMod>,
}

impl ModRegistry {
    pub fn new(mods: impl IntoIterator<Item = InstalledMod>) -> Self {
        let mut registry = Self::default();
        for installed in mods {
            registry.insert(installed);
        }
        registry
    }

    pub fn mods(&self) -> &[InstalledMod] {
        &self.mods
    }

    pub fn get(&self, project: &str) -> Option<&InstalledMod> {
        self.mods
            .iter()
            .find(|installed| installed.project == project)
    }

    pub fn get_mut(&mut self, project: &str) -> Option<&mut InstalledMod> {
        self.mods
            .iter_mut()
            .find(|installed| installed.project == project)
    }

    /// Removes a project's record, and the project from what others need.
    pub fn remove(&mut self, project: &str) -> Option<InstalledMod> {
        let index = self
            .mods
            .iter()
            .position(|installed| installed.project == project)?;
        let removed = self.mods.remove(index);
        for installed in &mut self.mods {
            installed.required_by.remove(project);
        }
        Some(removed)
    }

    /// The mods left needed by nothing once `project` is removed: mods the
    /// player did not ask for, whose every dependent is removed too.
    pub fn orphans_after_removing(&self, project: &str) -> Vec<String> {
        let mut left = self.clone();
        if left.remove(project).is_none() {
            return Vec::new();
        }
        let mut orphans = Vec::new();
        while let Some(orphan) = left
            .mods
            .iter()
            .find(|installed| !installed.requested && installed.required_by.is_empty())
            .map(|installed| installed.project.clone())
        {
            left.remove(&orphan);
            orphans.push(orphan);
        }
        orphans
    }

    /// Adds a mod, replacing the record of the same project.
    pub fn insert(&mut self, installed: InstalledMod) {
        match self.get_mut(&installed.project) {
            Some(existing) => *existing = installed,
            None => self.mods.push(installed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(project: &str, version: &str) -> InstalledMod {
        InstalledMod {
            project: project.into(),
            title: project.into(),
            version: version.into(),
            version_number: version.into(),
            filename: format!("{project}.jar"),
            sha512: "a".repeat(128),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: BTreeSet::new(),
        }
    }

    fn needed_by(mut installed: InstalledMod, needers: &[&str]) -> InstalledMod {
        installed.requested = false;
        installed.required_by = needers.iter().map(|needer| (*needer).to_owned()).collect();
        installed
    }

    #[test]
    fn removing_a_mod_orphans_the_dependencies_only_it_needed() {
        // Iris needs Sodium and Indium; Indium needs Sodium; Lithium needs
        // Fabric API, which the player also asked for.
        let mut fabric_api = installed("fabric-api", "1");
        fabric_api.required_by = BTreeSet::from(["lithium".to_owned()]);
        let registry = ModRegistry::new([
            installed("iris", "1"),
            needed_by(installed("indium", "1"), &["iris"]),
            needed_by(installed("sodium", "1"), &["iris", "indium"]),
            installed("lithium", "1"),
            fabric_api,
        ]);
        assert_eq!(
            registry.orphans_after_removing("iris"),
            ["indium", "sodium"]
        );
        assert!(registry.orphans_after_removing("lithium").is_empty());
        assert!(registry.orphans_after_removing("missing").is_empty());

        let mut left = registry.clone();
        assert!(left.remove("indium").is_some());
        assert_eq!(
            left.get("sodium").unwrap().required_by,
            BTreeSet::from(["iris".to_owned()])
        );
        assert!(left.remove("indium").is_none());
    }

    #[test]
    fn keeps_one_record_per_project_the_latest_winning() {
        let registry = ModRegistry::new([
            installed("sodium", "1"),
            installed("iris", "1"),
            installed("sodium", "2"),
        ]);
        assert_eq!(registry.mods().len(), 2);
        assert_eq!(
            registry.get("sodium").map(|m| m.version.as_str()),
            Some("2")
        );
        assert_eq!(registry.mods()[0].project, "sodium");
        assert!(registry.get("lithium").is_none());
    }
}
