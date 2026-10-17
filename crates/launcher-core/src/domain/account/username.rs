use std::fmt;

use thiserror::Error;

pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 16;

/// A Minecraft player name: 3-16 characters of `[A-Za-z0-9_]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(String);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UsernameError {
    #[error("username must be {MIN_LEN}-{MAX_LEN} characters long, got {0}")]
    Length(usize),
    #[error("username may only contain letters, digits and underscores")]
    InvalidCharacter,
}

impl Username {
    pub fn parse(raw: &str) -> Result<Self, UsernameError> {
        let len = raw.chars().count();
        if !(MIN_LEN..=MAX_LEN).contains(&len) {
            return Err(UsernameError::Length(len));
        }
        if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(UsernameError::InvalidCharacter);
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names_at_both_length_limits() {
        for name in ["abc", "polispol1", "Under_Score_16ch"] {
            assert_eq!(
                Username::parse(name).map(|u| u.to_string()),
                Ok(name.to_owned())
            );
        }
    }

    #[test]
    fn rejects_names_outside_3_to_16_characters() {
        assert_eq!(Username::parse("ab"), Err(UsernameError::Length(2)));
        assert_eq!(
            Username::parse("seventeen_chars_x"),
            Err(UsernameError::Length(17))
        );
        assert_eq!(Username::parse(""), Err(UsernameError::Length(0)));
    }

    #[test]
    fn rejects_characters_outside_the_allowed_set() {
        for name in [
            "with space",
            "dash-name",
            "dot.name",
            "\u{f1}andu",
            "../etc",
        ] {
            assert!(Username::parse(name).is_err(), "{name}");
        }
    }
}
