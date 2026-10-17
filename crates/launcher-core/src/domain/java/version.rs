use thiserror::Error;

/// A Java version as printed by `java -version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    /// Feature release: 8 for "1.8.0_402", 21 for "21.0.8".
    pub major: u32,
    /// The full version string, e.g. "21.0.8".
    pub full: String,
    /// `true` for a 64-bit VM (the launcher refuses 32-bit runtimes on 64-bit OSes).
    pub is_64_bit: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JavaVersionError {
    #[error("no version line found in `java -version` output")]
    NoVersionLine,
    #[error("unrecognized Java version {0:?}")]
    Unrecognized(String),
}

/// Parses `java -version` output (stderr). Handles `java version "1.8.0_402"`,
/// `openjdk version "21.0.8" 2025-07-15` and early-access `"25-ea"`.
pub fn parse_java_version_output(output: &str) -> Result<JavaVersion, JavaVersionError> {
    let line = output
        .lines()
        .find(|line| line.contains(" version \""))
        .ok_or(JavaVersionError::NoVersionLine)?;
    let full = line
        .split('"')
        .nth(1)
        .filter(|version| !version.is_empty())
        .ok_or(JavaVersionError::NoVersionLine)?
        .to_owned();

    let mut parts = full
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty());
    let first: u32 = parts
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| JavaVersionError::Unrecognized(full.clone()))?;
    let major = if first == 1 {
        parts
            .next()
            .and_then(|part| part.parse().ok())
            .ok_or_else(|| JavaVersionError::Unrecognized(full.clone()))?
    } else {
        first
    };

    let is_64_bit = output.contains("64-Bit") || output.contains("64-bit");
    Ok(JavaVersion {
        major,
        full,
        is_64_bit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMURIN_21: &str = "openjdk version \"21.0.8\" 2025-07-15 LTS\nOpenJDK Runtime Environment Temurin-21.0.8+9 (build 21.0.8+9-LTS)\nOpenJDK 64-Bit Server VM Temurin-21.0.8+9 (build 21.0.8+9-LTS, mixed mode, sharing)";
    const ORACLE_8_32BIT: &str = "java version \"1.8.0_402\"\nJava(TM) SE Runtime Environment (build 1.8.0_402-b06)\nJava HotSpot(TM) Client VM (build 25.402-b06, mixed mode)";

    #[test]
    fn parses_modern_versions() {
        let version = parse_java_version_output(TEMURIN_21).unwrap();
        assert_eq!(
            version,
            JavaVersion {
                major: 21,
                full: "21.0.8".into(),
                is_64_bit: true
            }
        );
    }

    #[test]
    fn parses_legacy_1_x_versions_and_detects_32_bit() {
        let version = parse_java_version_output(ORACLE_8_32BIT).unwrap();
        assert_eq!(version.major, 8);
        assert_eq!(version.full, "1.8.0_402");
        assert!(!version.is_64_bit);
    }

    #[test]
    fn parses_early_access_and_skips_preamble_lines() {
        let output = "Picked up JAVA_TOOL_OPTIONS: -Dfile.encoding=UTF8\nopenjdk version \"25-ea\" 2025-09-16\nOpenJDK 64-Bit Server VM";
        assert_eq!(parse_java_version_output(output).unwrap().major, 25);
    }

    #[test]
    fn rejects_output_without_a_version() {
        assert_eq!(
            parse_java_version_output("command not found"),
            Err(JavaVersionError::NoVersionLine)
        );
        assert_eq!(
            parse_java_version_output("java version \"abc\""),
            Err(JavaVersionError::Unrecognized("abc".into()))
        );
    }
}
