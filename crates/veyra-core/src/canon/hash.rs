//! BLAKE3 content identities and deterministic seed functions.

use crate::ids::{Hash32, ObjectId};

/// Hashes canonical or raw bytes with BLAKE3-256.
pub fn hash(bytes: &[u8]) -> Hash32 {
    Hash32(*blake3::hash(bytes).as_bytes())
}

/// Computes a BLAKE3 derive-key digest for an explicitly named context.
pub fn derive_key(context: &str, parts: &[&[u8]]) -> Hash32 {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(part);
    }
    Hash32(*hasher.finalize().as_bytes())
}

/// Computes the stable body seed from an object identity.
pub fn body_seed(object_id: ObjectId) -> u64 {
    let digest = derive_key("veyra.seed.v1", &[&object_id.0, b"body"]);
    u64::from_le_bytes(digest.0[..8].try_into().expect("fixed digest length"))
}

/// Computes a deterministic purpose-specific seed.
pub fn subseed(seed: u64, purpose: &str) -> u64 {
    let seed_bytes = seed.to_le_bytes();
    let digest = derive_key("veyra.seed.v1", &[&seed_bytes, purpose.as_bytes()]);
    u64::from_le_bytes(digest.0[..8].try_into().expect("fixed digest length"))
}

/// SplitMix64 finalizer, with wrapping arithmetic as specified.
pub const fn mix64(value: u64) -> u64 {
    let mut mixed = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

/// Stable child detail hash for one parent key and child quadrant.
pub const fn detail_hash(seed: u64, parent_key: u64, child: u64) -> u64 {
    mix64(mix64(seed ^ parent_key).wrapping_add(child.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
}

#[cfg(test)]
mod tests {
    use super::{body_seed, detail_hash, mix64, subseed};
    use crate::ids::ObjectId;

    #[test]
    fn seed_primitives_are_repeatable_and_wrap() {
        let id = ObjectId([0x42; 16]);
        let seed = body_seed(id);
        assert_eq!(body_seed(id), seed);
        assert_eq!(subseed(seed, "refine/example"), subseed(seed, "refine/example"));
        assert_eq!(detail_hash(seed, u64::MAX, 3), detail_hash(seed, u64::MAX, 3));
        assert_eq!(mix64(0), 0);
    }
}
