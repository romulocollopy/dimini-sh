use sha2::{Digest, Sha256};

/// Compute the SHA-256 hex digest of a string.
///
/// This is the canonical hash function used throughout the system — for
/// storing `url_hash` in the database and for deduplication lookups in the
/// use case layer. Both call sites must produce identical output for the same
/// input, so the implementation lives here in one place.
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}
