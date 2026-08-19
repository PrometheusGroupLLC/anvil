//! Step module for `telemetry_salt.feature`.
//!
//! Exercises the pure `actor_hash` salted-hash function: determinism, salt
//! sensitivity, and the fail-safe `None` outcomes (no salt / empty actor); and
//! the pure `key_epoch` fingerprint of the salt itself, which must stay
//! domain-separated from `actor_hash` so an epoch and a conversation hash can
//! never collide in one value space.
//!
//! Brine retains ONLY a step's declared `provides` keys between steps, so every
//! step that wants prior state (the salt, an earlier hash) to survive must
//! consume + re-provide it. Absence of a salt is modeled by the `NO_SALT_KEY`
//! flag; an absent actor hash is modeled by the empty string.

use anvil_core::domain::telemetry_salt::{
    actor_hash, key_epoch, ACTOR_HASH_HEX_LEN, KEY_EPOCH_HEX_LEN, UNKNOWN_KEY_EPOCH,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const SALT_KEY: &str = "tsalt_salt";
const NO_SALT_KEY: &str = "tsalt_no_salt";
const HASH1_KEY: &str = "tsalt_hash1";
const HASH2_KEY: &str = "tsalt_hash2";
const EPOCH1_KEY: &str = "tsalt_epoch1";
const EPOCH2_KEY: &str = "tsalt_epoch2";

fn resolve_salt(ctx: &Context) -> Option<String> {
    let no_salt = ctx.get::<bool>(NO_SALT_KEY).copied().unwrap_or(false);
    if no_salt {
        None
    } else {
        ctx.get::<String>(SALT_KEY).cloned()
    }
}

fn carry_salt(ctx: &Context, out: &mut Context) {
    if let Some(s) = ctx.get::<String>(SALT_KEY) {
        out.set::<String>(SALT_KEY, s.clone());
    }
    if let Some(b) = ctx.get::<bool>(NO_SALT_KEY) {
        out.set::<bool>(NO_SALT_KEY, *b);
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the telemetry salt {string}",
            &[],
            &[(SALT_KEY, "String")],
            |_ctx, params| {
                let salt = params.get_string(0).ok_or("Expected salt")?.to_string();
                let mut out = Context::new();
                out.set::<String>(SALT_KEY, salt);
                out.set::<bool>(NO_SALT_KEY, false);
                Ok(out)
            },
        ),
        step_def(
            "no telemetry salt is configured",
            &[],
            &[(NO_SALT_KEY, "bool")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set::<bool>(NO_SALT_KEY, true);
                Ok(out)
            },
        ),
        // Re-salt mid-scenario; carry the first hash forward so the comparison
        // check can still see it.
        step_def(
            "the telemetry salt is changed to {string}",
            &[(HASH1_KEY, "String")],
            &[(SALT_KEY, "String"), (HASH1_KEY, "String")],
            |ctx, params| {
                let salt = params.get_string(0).ok_or("Expected salt")?.to_string();
                let h1 = ctx.get::<String>(HASH1_KEY).cloned().unwrap_or_default();
                let mut out = Context::new();
                out.set::<String>(SALT_KEY, salt);
                out.set::<bool>(NO_SALT_KEY, false);
                out.set::<String>(HASH1_KEY, h1);
                Ok(out)
            },
        ),
        step_def(
            "the actor {string} is hashed",
            &[],
            &[
                (HASH1_KEY, "String"),
                (SALT_KEY, "String"),
                (NO_SALT_KEY, "bool"),
            ],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let hash = actor_hash(resolve_salt(&ctx).as_deref(), &actor);
                let mut out = Context::new();
                out.set::<String>(HASH1_KEY, hash.unwrap_or_default());
                carry_salt(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the actor {string} is hashed again",
            &[(HASH1_KEY, "String")],
            &[(HASH1_KEY, "String"), (HASH2_KEY, "String")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let hash = actor_hash(resolve_salt(&ctx).as_deref(), &actor);
                let h1 = ctx.get::<String>(HASH1_KEY).cloned().unwrap_or_default();
                let mut out = Context::new();
                out.set::<String>(HASH1_KEY, h1);
                out.set::<String>(HASH2_KEY, hash.unwrap_or_default());
                Ok(out)
            },
        ),
        check_def(
            "both actor hashes are equal",
            &[(HASH1_KEY, "String"), (HASH2_KEY, "String")],
            |ctx, _params| {
                let h1 = ctx.get::<String>(HASH1_KEY).ok_or("No first hash")?;
                let h2 = ctx.get::<String>(HASH2_KEY).ok_or("No second hash")?;
                if h1 == h2 {
                    Ok(())
                } else {
                    Err(format!("hashes differ: '{}' vs '{}'", h1, h2))
                }
            },
        ),
        check_def(
            "the two actor hashes differ",
            &[(HASH1_KEY, "String"), (HASH2_KEY, "String")],
            |ctx, _params| {
                let h1 = ctx.get::<String>(HASH1_KEY).ok_or("No first hash")?;
                let h2 = ctx.get::<String>(HASH2_KEY).ok_or("No second hash")?;
                if h1 != h2 {
                    Ok(())
                } else {
                    Err(format!("hashes are equal but should differ: '{}'", h1))
                }
            },
        ),
        check_def(
            "the actor hash is a non-empty 16-character hex string",
            &[(HASH1_KEY, "String")],
            |ctx, _params| {
                let h1 = ctx.get::<String>(HASH1_KEY).ok_or("No first hash")?;
                if h1.len() != ACTOR_HASH_HEX_LEN {
                    return Err(format!(
                        "expected {}-char hash, got {} ('{}')",
                        ACTOR_HASH_HEX_LEN,
                        h1.len(),
                        h1
                    ));
                }
                if h1.chars().all(|c| c.is_ascii_hexdigit()) {
                    Ok(())
                } else {
                    Err(format!("hash is not hex: '{}'", h1))
                }
            },
        ),
        check_def(
            "the actor hash is absent",
            &[(HASH1_KEY, "String")],
            |ctx, _params| {
                let h1 = ctx.get::<String>(HASH1_KEY).ok_or("No first hash slot")?;
                if h1.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected absent (empty) hash, got '{}'", h1))
                }
            },
        ),
        // ---- the key epoch: a fingerprint of the salt itself ----
        step_def(
            "the key epoch is fingerprinted",
            &[],
            &[
                (EPOCH1_KEY, "String"),
                (SALT_KEY, "String"),
                (NO_SALT_KEY, "bool"),
            ],
            |ctx, _params| {
                let epoch = key_epoch(resolve_salt(&ctx).as_deref());
                let mut out = Context::new();
                out.set::<String>(EPOCH1_KEY, epoch);
                carry_salt(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the key epoch is fingerprinted again",
            &[(EPOCH1_KEY, "String")],
            &[
                (EPOCH1_KEY, "String"),
                (EPOCH2_KEY, "String"),
                (SALT_KEY, "String"),
                (NO_SALT_KEY, "bool"),
            ],
            |ctx, _params| {
                let epoch = key_epoch(resolve_salt(&ctx).as_deref());
                let e1 = ctx.get::<String>(EPOCH1_KEY).cloned().unwrap_or_default();
                let mut out = Context::new();
                out.set::<String>(EPOCH1_KEY, e1);
                out.set::<String>(EPOCH2_KEY, epoch);
                carry_salt(&ctx, &mut out);
                Ok(out)
            },
        ),
        // Fingerprint a second, named salt without disturbing the configured one,
        // so a scenario can compare two keyspaces side by side.
        step_def(
            "the key epoch is fingerprinted for salt {string}",
            &[(EPOCH1_KEY, "String")],
            &[
                (EPOCH1_KEY, "String"),
                (EPOCH2_KEY, "String"),
                (SALT_KEY, "String"),
                (NO_SALT_KEY, "bool"),
            ],
            |ctx, params| {
                let salt = params.get_string(0).ok_or("Expected salt")?.to_string();
                let e1 = ctx.get::<String>(EPOCH1_KEY).cloned().unwrap_or_default();
                let mut out = Context::new();
                out.set::<String>(EPOCH1_KEY, e1);
                out.set::<String>(EPOCH2_KEY, key_epoch(Some(&salt)));
                carry_salt(&ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "both key epochs are equal",
            &[(EPOCH1_KEY, "String"), (EPOCH2_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No first key epoch")?;
                let e2 = ctx.get::<String>(EPOCH2_KEY).ok_or("No second key epoch")?;
                if e1 == e2 {
                    Ok(())
                } else {
                    Err(format!("key epochs differ: '{}' vs '{}'", e1, e2))
                }
            },
        ),
        check_def(
            "the two key epochs differ",
            &[(EPOCH1_KEY, "String"), (EPOCH2_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No first key epoch")?;
                let e2 = ctx.get::<String>(EPOCH2_KEY).ok_or("No second key epoch")?;
                if e1 != e2 {
                    Ok(())
                } else {
                    Err(format!("key epochs are equal but should differ: '{}'", e1))
                }
            },
        ),
        check_def(
            "the key epoch is a non-empty 12-character lowercase hex string",
            &[(EPOCH1_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No key epoch")?;
                if e1.len() != KEY_EPOCH_HEX_LEN {
                    return Err(format!(
                        "expected {}-char key epoch, got {} ('{}')",
                        KEY_EPOCH_HEX_LEN,
                        e1.len(),
                        e1
                    ));
                }
                if e1.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
                    Ok(())
                } else {
                    Err(format!("key epoch is not lowercase hex: '{}'", e1))
                }
            },
        ),
        check_def(
            "the key epoch is the unknown key-epoch sentinel",
            &[(EPOCH1_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No key epoch")?;
                if e1 == UNKNOWN_KEY_EPOCH {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the sentinel '{}', got '{}'",
                        UNKNOWN_KEY_EPOCH, e1
                    ))
                }
            },
        ),
        // Domain separation against the conversation-hash value space: the epoch
        // is not the actor hash of the salt under itself, which is what a
        // truncated-`actor_hash` implementation would produce.
        check_def(
            "the key epoch is not the actor hash of the salt, nor a prefix or suffix of it",
            &[(EPOCH1_KEY, "String"), (SALT_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No key epoch")?;
                let salt = ctx.get::<String>(SALT_KEY).ok_or("No salt")?;
                let hash = actor_hash(Some(salt), salt).ok_or("No actor hash for the salt")?;
                if e1 == &hash {
                    return Err(format!("key epoch IS the actor hash: '{}'", e1));
                }
                if hash.starts_with(e1.as_str()) {
                    return Err(format!(
                        "key epoch '{}' is a prefix of the actor hash '{}'",
                        e1, hash
                    ));
                }
                if hash.ends_with(e1.as_str()) {
                    return Err(format!(
                        "key epoch '{}' is a suffix of the actor hash '{}'",
                        e1, hash
                    ));
                }
                Ok(())
            },
        ),
        // The other half of domain separation: the digest input must carry the
        // domain, so the epoch is not the same digest taken without one.
        check_def(
            "the key epoch is not the undomained digest of the salt",
            &[(EPOCH1_KEY, "String"), (SALT_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No key epoch")?;
                let salt = ctx.get::<String>(SALT_KEY).ok_or("No salt")?;
                // `actor_hash` with an empty salt digests `":" || salt` — the
                // same input the epoch takes with its domain dropped.
                let undomained = actor_hash(Some(""), salt).ok_or("No undomained digest")?;
                if undomained.starts_with(e1.as_str()) {
                    Err(format!(
                        "key epoch '{}' is the undomained digest '{}'",
                        e1, undomained
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the key epoch does not contain the salt",
            &[(EPOCH1_KEY, "String"), (SALT_KEY, "String")],
            |ctx, _params| {
                let e1 = ctx.get::<String>(EPOCH1_KEY).ok_or("No key epoch")?;
                let salt = ctx.get::<String>(SALT_KEY).ok_or("No salt")?;
                if e1.contains(salt.as_str()) {
                    Err(format!("key epoch '{}' contains the salt '{}'", e1, salt))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
