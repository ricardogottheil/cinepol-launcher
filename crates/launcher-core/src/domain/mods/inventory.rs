//! What an instance's `mods/` folder holds, as the player sees it.

use super::InstalledMod;

/// The suffix that keeps a jar in `mods/` without the loader reading it.
pub const DISABLED_SUFFIX: &str = ".disabled";
const JAR_EXTENSION: &str = ".jar";

/// Where a jar in `mods/` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModOrigin {
    /// Installed by the launcher and recorded in its registry.
    Launcher,
    /// Put there by hand or by a pack, and recognised by the catalog from
    /// its hash.
    Identified,
    /// Not known to the catalog.
    Unknown,
}

/// One jar in `mods/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModEntry {
    /// The jar's name as the loader reads it, without [`DISABLED_SUFFIX`].
    pub filename: String,
    pub enabled: bool,
    /// Lowercase hex sha512 of the jar.
    pub sha512: String,
    pub origin: ModOrigin,
    /// What is known about it; `None` for [`ModOrigin::Unknown`].
    pub record: Option<InstalledMod>,
}

impl ModEntry {
    /// The name the jar has on disk.
    pub fn file_on_disk(&self) -> String {
        if self.enabled {
            self.filename.clone()
        } else {
            format!("{}{DISABLED_SUFFIX}", self.filename)
        }
    }

    /// The mod's title, or its file name when the catalog does not know it.
    pub fn title(&self) -> &str {
        self.record
            .as_ref()
            .map_or(self.filename.as_str(), |record| record.title.as_str())
    }
}

/// For a mod jar's name on disk (`x.jar` or `x.jar.disabled`, any case of
/// `.jar`), the name the loader reads and whether it is enabled.
pub fn mod_jar_name(name: &str) -> Option<(&str, bool)> {
    let (base, enabled) = match name.strip_suffix(DISABLED_SUFFIX) {
        Some(base) => (base, false),
        None => (name, true),
    };
    let is_jar =
        base.len() > JAR_EXTENSION.len() && base.to_ascii_lowercase().ends_with(JAR_EXTENSION);
    is_jar.then_some((base, enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_enabled_and_disabled_jar_names() {
        assert_eq!(mod_jar_name("sodium.jar"), Some(("sodium.jar", true)));
        assert_eq!(mod_jar_name("Sodium.JAR"), Some(("Sodium.JAR", true)));
        assert_eq!(
            mod_jar_name("sodium.jar.disabled"),
            Some(("sodium.jar", false))
        );
        for other in [
            "notes.txt",
            ".jar",
            "sodium.zip.disabled",
            "x.disabled",
            "sodium.jar.bak",
        ] {
            assert_eq!(mod_jar_name(other), None, "{other}");
        }
    }
}
