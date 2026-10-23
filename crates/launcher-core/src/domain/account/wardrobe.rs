//! What a Microsoft account can wear: an uploaded skin and its capes.

use thiserror::Error;

use super::SkinUrl;

/// Mojang draws skins from 64x64 textures, or the older 64x32 layout.
const SKIN_WIDTH: u32 = 64;
const SKIN_HEIGHTS: [u32; 2] = [64, 32];
/// A skin texture is a few kilobytes; anything near this is not one.
pub const MAX_SKIN_PNG_BYTES: usize = 256 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SkinPngError {
    #[error("the file is not a PNG image")]
    NotPng,
    #[error("a skin is 64x64 or 64x32 pixels, this image is {width}x{height}")]
    WrongSize { width: u32, height: u32 },
    #[error("the image is larger than {MAX_SKIN_PNG_BYTES} bytes")]
    TooLarge,
}

/// A PNG the player picked to upload as their skin, checked before it leaves
/// the launcher: Mojang refuses anything else, and every refused upload
/// counts against the account's rate limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinPng(Vec<u8>);

impl SkinPng {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, SkinPngError> {
        if bytes.len() > MAX_SKIN_PNG_BYTES {
            return Err(SkinPngError::TooLarge);
        }
        // The first chunk of every PNG is IHDR: length 13, then width and
        // height as big-endian u32.
        let ihdr = bytes.get(8..24).ok_or(SkinPngError::NotPng)?;
        if !bytes.starts_with(&PNG_SIGNATURE) || &ihdr[4..8] != b"IHDR" {
            return Err(SkinPngError::NotPng);
        }
        let number =
            |at: usize| u32::from_be_bytes([ihdr[at], ihdr[at + 1], ihdr[at + 2], ihdr[at + 3]]);
        let (width, height) = (number(8), number(12));
        if width != SKIN_WIDTH || !SKIN_HEIGHTS.contains(&height) {
            return Err(SkinPngError::WrongSize { width, height });
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("not a Minecraft cape id: {0}")]
pub struct CapeIdError(pub String);

/// A cape's id as Mojang lists it (a UUID). It goes into a request body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapeId(String);

impl CapeId {
    pub fn parse(raw: &str) -> Result<Self, CapeIdError> {
        let hex: String = raw.chars().filter(|c| *c != '-').collect();
        let dashes_in_place = raw.len() == 32
            || (raw.len() == 36 && [8, 13, 18, 23].iter().all(|&at| raw.as_bytes()[at] == b'-'));
        if dashes_in_place && hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(CapeIdError(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A cape the profile owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cape {
    pub id: CapeId,
    /// Mojang's name for it ("Migrator", "Pan"...).
    pub alias: String,
    pub url: SkinUrl,
    /// Worn right now; at most one cape is.
    pub active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The signature and an IHDR chunk header: what `parse` reads.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn accepts_64x64_and_64x32_skins() {
        for height in [64, 32] {
            let bytes = png(64, height);
            assert_eq!(SkinPng::parse(bytes.clone()).unwrap().as_bytes(), bytes);
        }
    }

    #[test]
    fn refuses_other_sizes_other_files_and_huge_files() {
        assert_eq!(
            SkinPng::parse(png(128, 128)),
            Err(SkinPngError::WrongSize {
                width: 128,
                height: 128
            })
        );
        assert_eq!(
            SkinPng::parse(png(64, 48)),
            Err(SkinPngError::WrongSize {
                width: 64,
                height: 48
            })
        );
        assert_eq!(
            SkinPng::parse(b"GIF89a".to_vec()),
            Err(SkinPngError::NotPng)
        );
        assert_eq!(
            SkinPng::parse(PNG_SIGNATURE.to_vec()),
            Err(SkinPngError::NotPng)
        );
        let mut not_ihdr = png(64, 64);
        not_ihdr[12..16].copy_from_slice(b"tEXt");
        assert_eq!(SkinPng::parse(not_ihdr), Err(SkinPngError::NotPng));
        let mut huge = png(64, 64);
        huge.resize(MAX_SKIN_PNG_BYTES + 1, 0);
        assert_eq!(SkinPng::parse(huge), Err(SkinPngError::TooLarge));
    }

    #[test]
    fn cape_ids_are_uuids() {
        for good in [
            "1981aad3-73bd-4d8b-a8d5-4dd0f5bc8e1c",
            "1981AAD373BD4D8BA8D54DD0F5BC8E1C",
        ] {
            assert_eq!(CapeId::parse(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            "not-a-uuid",
            "1981aad3\"}",
            "1981aad3-73bd-4d8b-a8d5-4dd0f5bc8e1c0",
        ] {
            assert_eq!(CapeId::parse(bad), Err(CapeIdError(bad.to_owned())));
        }
    }
}
