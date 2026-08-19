use sha2::{Digest, Sha256};

/// Hash the exact live bytes of a target. Single-sourced so preparation and
/// recovery can never disagree about what "unchanged" means.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
