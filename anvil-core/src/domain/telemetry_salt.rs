//! Salted, non-reversible actor-name hashing for the step-measurement sink,
//! plus the two constants and the one fingerprint that live in the same
//! keyspace: the unknown-conversation-hash sentinel and the key epoch.
//!
//! Mirrors foundry-evaluation-hearth's `telemetry_emitter.py::redact_token`: a
//! salted, truncated SHA-256. The salt is per-DEPLOYMENT (not per-hearth), so the
//! same actor name hashes to the same value across every hearth — which is what
//! makes cross-hearth distinct-actor counts correct (union the `actor_hash` sets
//! before counting).
//!
//! Fail-safe: when no salt can be established the caller passes `None` and NO
//! actor hash is emitted — the sink stores `actor_hash = None` rather than a raw
//! or unsalted (reversible-by-rainbow-table) actor name. Recording an unsalted
//! hash would be a privacy regression, so the absence of a salt MUST degrade to
//! "no actor counting", never to "leak".

use sha2::{Digest, Sha256};

/// The conversation hash recorded when no engine answer exists, or when the
/// engine has no salt to hash with — never a raw id and never an omitted field.
///
/// One constant, not a literal per site: the join's unjoinable bucket keys off
/// this exact string, so a hand-typed copy that drifts turns unjoinable rows
/// into joinable-looking ones with a hash nobody can match.
pub const UNKNOWN_CONVERSATION_HASH: &str = "unknown_conversation_hash";

/// Number of hex characters retained from the SHA-256 digest. 16 hex chars = 64
/// bits of the digest — enough to make collisions negligible for the actor
/// cardinalities we count, while keeping the stored token compact.
pub const ACTOR_HASH_HEX_LEN: usize = 16;

/// Domain prefix for the key-epoch digest. It is what keeps an epoch out of the
/// conversation-hash value space: `actor_hash` digests `salt || ":" || value`,
/// so without a distinct domain in front an epoch would be the truncated hash of
/// some conversation id.
pub const KEY_EPOCH_DOMAIN: &str = "anvil-key-epoch-v1";

/// Hex characters retained for a key epoch. Deliberately NOT
/// [`ACTOR_HASH_HEX_LEN`]: the two lengths are the second half of the domain
/// separation, so a hash and an epoch cannot be mistaken for one another by
/// shape either.
pub const KEY_EPOCH_HEX_LEN: usize = 12;

/// The epoch reported when no salt is configured. An absent field would read as
/// "not applicable"; the unknown keyspace has to be nameable.
pub const UNKNOWN_KEY_EPOCH: &str = "unknown_key_epoch";

/// Fingerprint the DEPLOYMENT salt so two reports can declare whether they were
/// computed in the same keyspace, without either of them carrying the salt.
///
/// `key_epoch(Some(s))` for non-empty `s` = `hex(sha256(KEY_EPOCH_DOMAIN || ":" || s))[..12]`.
/// `key_epoch(None)` == `key_epoch(Some(""))` == [`UNKNOWN_KEY_EPOCH`].
///
/// Non-reversible and domain-separated from [`actor_hash`] by both the prefix and
/// the truncation length: an epoch and a conversation hash must never collide in
/// one value space, because consumers compare epochs as opaque strings.
pub fn key_epoch(salt: Option<&str>) -> String {
    let salt = match salt {
        Some(salt) if !salt.is_empty() => salt,
        _ => return UNKNOWN_KEY_EPOCH.to_string(),
    };
    let mut hasher = Sha256::new();
    hasher.update(KEY_EPOCH_DOMAIN.as_bytes());
    hasher.update(b":");
    hasher.update(salt.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    hex[..KEY_EPOCH_HEX_LEN].to_string()
}

/// Compute the salted, truncated actor hash, or `None` when no salt is
/// configured.
///
/// `actor_hash = hex(sha256(salt || ":" || actor))[..16]`.
///
/// - `salt = None` → `None` (fail-safe: never emit a raw or unsalted actor).
/// - `actor` empty → `None` (no actor to count; e.g. orientation checkin).
///
/// Deterministic: the same `(salt, actor)` always yields the same hash, so an
/// actor is counted once per period regardless of how many steps it completed.
pub fn actor_hash(salt: Option<&str>, actor: &str) -> Option<String> {
    let salt = salt?;
    if actor.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(actor.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    Some(hex[..ACTOR_HASH_HEX_LEN].to_string())
}
