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
