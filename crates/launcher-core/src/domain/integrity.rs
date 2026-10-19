//! Content hashes declared by Mojang, Modrinth, CurseForge and packwiz.

use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    /// CurseForge fingerprint: MurmurHash2 (32-bit) over whitespace-stripped bytes.
    Murmur2,
}

impl HashAlgorithm {
    /// Parses the names used by packwiz (`sha1`, `sha256`, `sha512`, `murmur2`).
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "sha512" => Some(Self::Sha512),
            "murmur2" => Some(Self::Murmur2),
            _ => None,
        }
    }

    /// Hex length of a digest (murmur2 is a decimal u32, so it has none).
    fn hex_len(self) -> Option<usize> {
        match self {
            Self::Sha1 => Some(40),
            Self::Sha256 => Some(64),
            Self::Sha512 => Some(128),
            Self::Murmur2 => None,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChecksumError {
    #[error("{algorithm:?} digest must be {expected} hex characters, got {actual}")]
    Length {
        algorithm: HashAlgorithm,
        expected: usize,
        actual: usize,
    },
    #[error("digest is not valid hexadecimal")]
    NotHex,
    #[error("murmur2 fingerprint must be an unsigned 32-bit decimal number")]
    NotMurmur2,
}

/// An expected digest. Values are normalized to lowercase hex (or decimal for murmur2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Checksum {
    algorithm: HashAlgorithm,
    value: String,
}

impl Checksum {
    pub fn parse(algorithm: HashAlgorithm, raw: &str) -> Result<Self, ChecksumError> {
        let raw = raw.trim();
        let value = match algorithm.hex_len() {
            Some(expected) => {
                if raw.len() != expected {
                    return Err(ChecksumError::Length {
                        algorithm,
                        expected,
                        actual: raw.len(),
                    });
                }
                if !raw.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(ChecksumError::NotHex);
                }
                raw.to_ascii_lowercase()
            }
            None => raw
                .parse::<u32>()
                .map_err(|_| ChecksumError::NotMurmur2)?
                .to_string(),
        };
        Ok(Self { algorithm, value })
    }

    pub fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Compares a computed digest, ignoring hex case.
    pub fn matches(&self, computed: &str) -> bool {
        self.value.eq_ignore_ascii_case(computed.trim())
    }
}

/// CurseForge's file fingerprint: MurmurHash2 (32-bit, seed 1) over the bytes
/// with every tab, line feed, carriage return and space removed.
pub fn curseforge_fingerprint(bytes: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let data: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    // The length is part of the seed, so it is the filtered length (truncated
    // to 32 bits like CurseForge's implementation).
    let mut h: u32 = 1 ^ (data.len() as u32);
    let (blocks, rest) = data.as_chunks::<4>();
    for block in blocks {
        let mut k = u32::from_le_bytes(*block);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    if rest.len() == 3 {
        h ^= u32::from(rest[2]) << 16;
    }
    if rest.len() >= 2 {
        h ^= u32::from(rest[1]) << 8;
    }
    if !rest.is_empty() {
        h ^= u32::from(rest[0]);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}:{}", self.algorithm, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "A94A8FE5CCB19BA61C4C0873D391E987982FBBD3";

    #[test]
    fn normalizes_hex_to_lowercase_and_matches_any_case() {
        let checksum = Checksum::parse(HashAlgorithm::Sha1, SHA1).unwrap();
        assert_eq!(checksum.value(), SHA1.to_ascii_lowercase());
        assert!(checksum.matches(SHA1));
        assert!(!checksum.matches("0000000000000000000000000000000000000000"));
    }

    #[test]
    fn rejects_wrong_length_and_non_hex_digests() {
        assert_eq!(
            Checksum::parse(HashAlgorithm::Sha256, SHA1),
            Err(ChecksumError::Length {
                algorithm: HashAlgorithm::Sha256,
                expected: 64,
                actual: 40
            })
        );
        assert_eq!(
            Checksum::parse(HashAlgorithm::Sha1, &"z".repeat(40)),
            Err(ChecksumError::NotHex)
        );
    }

    #[test]
    fn parses_murmur2_fingerprints_as_u32() {
        assert_eq!(
            Checksum::parse(HashAlgorithm::Murmur2, "3608199863")
                .unwrap()
                .value(),
            "3608199863"
        );
        assert_eq!(
            Checksum::parse(HashAlgorithm::Murmur2, "-1"),
            Err(ChecksumError::NotMurmur2)
        );
        assert_eq!(
            Checksum::parse(HashAlgorithm::Murmur2, "4294967296"),
            Err(ChecksumError::NotMurmur2)
        );
    }

    #[test]
    fn parses_packwiz_algorithm_names() {
        assert_eq!(HashAlgorithm::parse("SHA512"), Some(HashAlgorithm::Sha512));
        assert_eq!(HashAlgorithm::parse("md5"), None);
    }

    #[test]
    fn curseforge_fingerprints_ignore_whitespace() {
        // Reference values from an independent MurmurHash2 implementation.
        for (bytes, expected) in [
            (b"".as_slice(), 1_540_447_798_u32),
            (b"a", 626_045_324),
            (b"ab", 1_692_487_918),
            (b"abc", 1_621_425_345),
            (b"abcd", 3_376_380_438),
            (b"hello world", 2_824_650_221),
            (b"Hello, Cinepol!\n", 3_748_367_926),
        ] {
            assert_eq!(curseforge_fingerprint(bytes), expected, "{bytes:?}");
        }
        assert_eq!(
            curseforge_fingerprint(b" \t\r\n"),
            curseforge_fingerprint(b"")
        );
        assert_eq!(
            curseforge_fingerprint(b"hel lo\nworld"),
            curseforge_fingerprint(b"helloworld")
        );
    }
}
