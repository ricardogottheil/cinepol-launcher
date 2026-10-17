use std::fmt;

use thiserror::Error;

const MAX_NAME_CHARS: usize = 64;
const MAX_ID_LEN: usize = 48;
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstanceNameError {
    #[error("instance name must not be empty")]
    Empty,
    #[error("instance name must be at most {MAX_NAME_CHARS} characters")]
    TooLong,
    #[error("instance name must not contain control characters")]
    ControlCharacter,
}

/// The display name of an instance. Free text (any language, emoji), but
/// never used as a path: the directory comes from `InstanceId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceName(String);

impl InstanceName {
    pub fn parse(raw: &str) -> Result<Self, InstanceNameError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InstanceNameError::Empty);
        }
        if trimmed.chars().count() > MAX_NAME_CHARS {
            return Err(InstanceNameError::TooLong);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(InstanceNameError::ControlCharacter);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid instance id {0:?}: use 1-48 characters of [a-z0-9-], not a reserved name")]
pub struct InstanceIdError(pub String);

/// Stable identifier and directory name of an instance: `[a-z0-9-]`, safe as a
/// path segment on every OS (no separators, dots or Windows device names).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(String);

impl InstanceId {
    pub fn parse(raw: &str) -> Result<Self, InstanceIdError> {
        let valid = !raw.is_empty()
            && raw.len() <= MAX_ID_LEN
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !raw.starts_with('-')
            && !raw.ends_with('-')
            && !WINDOWS_RESERVED.contains(&raw);
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(InstanceIdError(raw.to_owned()))
        }
    }

    /// Derives an id from a display name: "My World 1.21!" -> "my-world-1-21".
    /// Uniqueness against existing instances is the caller's job.
    pub fn slug_from(name: &InstanceName) -> Self {
        let mut slug = String::new();
        for c in name.as_str().chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let mut slug: String = slug
            .trim_end_matches('-')
            .chars()
            .take(MAX_ID_LEN)
            .collect();
        slug = slug.trim_end_matches('-').to_owned();
        if slug.is_empty() {
            slug = "instance".to_owned();
        }
        if WINDOWS_RESERVED.contains(&slug.as_str()) {
            slug = format!("instance-{slug}");
        }
        Self(slug)
    }

    /// `base`, or `base-2`, `base-3`... for the first one `taken` rejects.
    pub fn unique(base: &Self, taken: impl Fn(&Self) -> bool) -> Self {
        if !taken(base) {
            return base.clone();
        }
        (2u32..)
            .map(|n| {
                let suffix = format!("-{n}");
                let stem: String = base.0.chars().take(MAX_ID_LEN - suffix.len()).collect();
                Self(format!("{}{suffix}", stem.trim_end_matches('-')))
            })
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| base.clone())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> InstanceName {
        InstanceName::parse(raw).unwrap()
    }

    #[test]
    fn names_are_trimmed_free_text() {
        assert_eq!(name("  Survival World ✨ ").as_str(), "Survival World ✨");
        assert_eq!(InstanceName::parse("   "), Err(InstanceNameError::Empty));
        assert_eq!(
            InstanceName::parse(&"x".repeat(65)),
            Err(InstanceNameError::TooLong)
        );
        assert_eq!(
            InstanceName::parse("a\u{0}b"),
            Err(InstanceNameError::ControlCharacter)
        );
    }

    #[test]
    fn slugs_are_safe_path_segments() {
        let cases = [
            ("My World 1.21!", "my-world-1-21"),
            ("../../etc/passwd", "etc-passwd"),
            ("C:\\Windows\\System32", "c-windows-system32"),
            ("✨✨✨", "instance"),
            ("CON", "instance-con"),
        ];
        for (raw, expected) in cases {
            let id = InstanceId::slug_from(&name(raw));
            assert_eq!(id.as_str(), expected, "{raw}");
            assert_eq!(InstanceId::parse(id.as_str()), Ok(id));
        }
    }

    #[test]
    fn slugs_respect_the_length_limit() {
        let id = InstanceId::slug_from(&name(&"ab ".repeat(21)));
        assert!(id.as_str().len() <= MAX_ID_LEN);
        assert!(!id.as_str().ends_with('-'));
    }

    #[test]
    fn parse_rejects_unsafe_ids() {
        for raw in [
            "",
            "Upper",
            "a/b",
            "a.b",
            "-a",
            "a-",
            "nul",
            "com1",
            &"a".repeat(49),
        ] {
            assert!(InstanceId::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn unique_appends_the_first_free_suffix() {
        let base = InstanceId::parse("cinepol").unwrap();
        let taken = ["cinepol", "cinepol-2"];
        let id = InstanceId::unique(&base, |candidate| taken.contains(&candidate.as_str()));
        assert_eq!(id.as_str(), "cinepol-3");
    }
}
