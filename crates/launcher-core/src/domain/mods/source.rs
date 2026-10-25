//! Which catalog a mod id belongs to. CurseForge project and file ids are
//! numbers, so they are kept as `curseforge:<number>`; Modrinth ids never
//! contain a colon. Ids keep their catalog wherever they go (the registry,
//! dependencies, the frontend) without a separate field.

use crate::domain::modpack::ModpackSourceKind;

const CURSEFORGE_PREFIX: &str = "curseforge:";

/// The catalog a project or version id names.
pub fn mod_catalog(id: &str) -> ModpackSourceKind {
    if id.starts_with(CURSEFORGE_PREFIX) {
        ModpackSourceKind::CurseForge
    } else {
        ModpackSourceKind::Modrinth
    }
}

/// The id of a CurseForge project or file.
pub fn curseforge_mod_id(number: u64) -> String {
    format!("{CURSEFORGE_PREFIX}{number}")
}

/// The CurseForge number behind a `curseforge:` id; `None` for any other id,
/// including one whose number is not a plain positive decimal.
pub fn curseforge_number(id: &str) -> Option<u64> {
    let digits = id.strip_prefix(CURSEFORGE_PREFIX)?;
    if digits.is_empty() || digits.len() > 12 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|number| *number > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curseforge_ids_carry_their_catalog_and_round_trip() {
        assert_eq!(curseforge_mod_id(306612), "curseforge:306612");
        assert_eq!(curseforge_number("curseforge:306612"), Some(306612));
        assert_eq!(
            mod_catalog("curseforge:306612"),
            ModpackSourceKind::CurseForge
        );
        assert_eq!(mod_catalog("AANobbMI"), ModpackSourceKind::Modrinth);
        assert_eq!(mod_catalog("sodium"), ModpackSourceKind::Modrinth);
    }

    #[test]
    fn only_plain_positive_numbers_are_curseforge_ids() {
        for id in [
            "306612",
            "curseforge:",
            "curseforge:0",
            "curseforge:-1",
            "curseforge:+1",
            "curseforge:12a",
            "curseforge:1/../2",
            "curseforge:1234567890123",
        ] {
            assert_eq!(curseforge_number(id), None, "{id}");
        }
    }
}
