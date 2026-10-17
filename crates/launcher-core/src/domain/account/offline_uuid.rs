use md5::{Digest, Md5};
use uuid::{Builder, Uuid};

use super::Username;

/// The profile UUID vanilla assigns to an offline player:
/// `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`,
/// that is, an MD5 of the bytes with the version 3 and RFC 4122 bits set
/// (no namespace, unlike `Uuid::new_v3`).
pub fn offline_uuid(username: &Username) -> Uuid {
    let digest = Md5::digest(format!("OfflinePlayer:{}", username.as_str()).as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest);
    Builder::from_md5_bytes(bytes).into_uuid()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid_of(name: &str) -> String {
        offline_uuid(&Username::parse(name).unwrap()).to_string()
    }

    #[test]
    fn matches_vanilla_offline_uuids() {
        // Reference values computed with Java's UUID.nameUUIDFromBytes.
        assert_eq!(uuid_of("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(uuid_of("polispol1"), "613bd4fe-6f7f-3488-8542-a216abf295e0");
    }

    #[test]
    fn is_case_sensitive_like_vanilla() {
        assert_ne!(uuid_of("Notch"), uuid_of("notch"));
    }

    #[test]
    fn is_a_version_3_uuid() {
        assert_eq!(
            offline_uuid(&Username::parse("jeb_").unwrap()).get_version_num(),
            3
        );
    }
}
