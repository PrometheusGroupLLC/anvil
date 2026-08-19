//! Deployment-level telemetry-salt resolution for the step-measurement sink.
//!
//! The actor_hash written to the step-measurement sink is a salted, truncated
//! SHA-256 (see `anvil_core::domain::telemetry_salt::actor_hash`). The salt must
//! be PER-DEPLOYMENT — not per-hearth — so the same actor hashes identically
//! across every hearth, which is what makes cross-hearth distinct-actor counts
//! correct.
//!
//! Resolution priority:
//!   1. env `ANVIL_TELEMETRY_SALT` (operator-pinned; never persisted).
//!   2. a generated random salt persisted ONCE to a deployment-level
//!      `.telemetry-salt` file, under the global-playbooks-hearth if configured,
//!      else the first permitted root.
//!
//! Fail-safe: if no salt can be established (no env, no writable deployment
//! location), the result is `None` — the caller then emits `actor_hash = None`
//! (no actor counting) rather than a raw or unsalted actor. Establishing a salt
//! must NEVER fail the lifecycle transition that emits the measurement.

use std::path::{Path, PathBuf};

/// The env var an operator may set to pin the deployment salt.
pub const SALT_ENV: &str = "ANVIL_TELEMETRY_SALT";

/// The persisted salt filename at the deployment location.
pub const SALT_FILENAME: &str = ".telemetry-salt";

/// Choose the deployment-level directory that holds the persisted salt:
/// the global-playbooks-hearth if configured, else the first permitted root.
/// `None` when neither is available (then no salt can be persisted).
pub fn salt_dir<'a>(
    global_playbooks_hearth: Option<&'a Path>,
    permitted_roots: &'a [PathBuf],
) -> Option<&'a Path> {
    global_playbooks_hearth.or_else(|| permitted_roots.first().map(|p| p.as_path()))
}

/// What the read-only half of the resolution found, in enough detail for the
/// writing half to decide. Collapsing `Unreadable` into `Absent` would undo the
/// C-d.1 round-8 fix below by regenerating over a salt file that exists.
enum SaltPeek {
    Found(String),
    /// No file yet (or an empty one) at a location that exists — the
    /// first-run case, and the only case `resolve_salt` may generate into.
    Absent { path: PathBuf },
    /// A file is there and cannot be read, or there is nowhere to look.
    /// Degrades to `None` and never generates.
    Unusable,
}

/// Steps 1 and 2 of the resolution, and NOTHING else: the env override, then an
/// existing readable non-empty deployment salt file. Never generates, never
/// writes.
///
/// This is what a READ path must call. `resolve_salt` persists a generated salt
/// (step 3), so a read RPC that reached for it would be a write path whatever
/// its name said — and it would also mint a brand-new keyspace as a side effect
/// of asking a question about the old one.
fn peek(global_playbooks_hearth: Option<&Path>, permitted_roots: &[PathBuf]) -> SaltPeek {
    // 1. Env override — highest priority, never persisted.
    if let Ok(salt) = std::env::var(SALT_ENV) {
        if !salt.is_empty() {
            return SaltPeek::Found(salt);
        }
    }

    // 2. Persisted deployment salt.
    let Some(dir) = salt_dir(global_playbooks_hearth, permitted_roots) else {
        return SaltPeek::Unusable;
    };
    let path = dir.join(SALT_FILENAME);

    // C-d.1 round 8, H-3. This was `if let Ok(existing) = ...` with no other
    // arm, so an UNREADABLE salt file fell through to step 3 and was
    // regenerated and written over — fragmenting the pseudonymous actor counts
    // across the rotation, which is the exact outcome degrading to `None` exists
    // to avoid. An ABSENT file is the first-run case and still falls through; an
    // unreadable one degrades to `None`.
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let trimmed = existing.trim();
            if trimmed.is_empty() {
                SaltPeek::Absent { path }
            } else {
                SaltPeek::Found(trimmed.to_string())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SaltPeek::Absent { path },
        Err(_) => SaltPeek::Unusable,
    }
}

/// The non-generating resolution: the deployment salt if one is already
/// established, `None` otherwise. See [`peek`] for why a read path may not call
/// [`resolve_salt`].
pub fn peek_salt(
    global_playbooks_hearth: Option<&Path>,
    permitted_roots: &[PathBuf],
) -> Option<String> {
    match peek(global_playbooks_hearth, permitted_roots) {
        SaltPeek::Found(salt) => Some(salt),
        SaltPeek::Absent { .. } | SaltPeek::Unusable => None,
    }
}

/// Resolve the deployment salt (env → persisted file → generate-and-persist).
///
/// Returns `None` (fail-safe) when no salt can be established. The generated
/// salt is a 32-byte hex string. Concurrent generation is benign: the first
/// writer's value wins on re-read; a write race at worst persists two distinct
/// files across processes started simultaneously, which the env override exists
/// to prevent in real deployments.
///
/// The priority order lives ONCE, in [`peek`]; this function adds step 3 and
/// nothing else.
pub fn resolve_salt(
    global_playbooks_hearth: Option<&Path>,
    permitted_roots: &[PathBuf],
) -> Option<String> {
    match peek(global_playbooks_hearth, permitted_roots) {
        SaltPeek::Found(salt) => Some(salt),
        SaltPeek::Unusable => None,
        // 3. Generate + persist once. A failure to persist degrades to None (no
        //    actor counting) rather than emitting an ephemeral per-process salt
        //    that would fragment distinct counts across restarts.
        SaltPeek::Absent { path } => {
            let generated = generate_salt();
            match std::fs::write(&path, &generated) {
                Ok(()) => Some(generated),
                Err(_) => None,
            }
        }
    }
}

/// Generate a fresh 32-byte salt rendered as 64 hex chars.
fn generate_salt() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
