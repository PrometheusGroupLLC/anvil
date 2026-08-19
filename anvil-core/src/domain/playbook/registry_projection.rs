//! C9: the canonical `playbooks.md` projection, the generations dual-read, and
//! the fail-loud hearth directory move.
//!
//! Four rules live here, and each replaces something that used to be silent.
//!
//! 1. **The projection is built from the LOADED DEFINITION SET, never from a
//!    directory listing.** A directory on disk that the loader could not resolve
//!    (no `machine.yaml`, unparseable, dropped under enforcement) is ABSENT from
//!    the projection and its exclusion is STATED IN the projection. A listing-
//!    derived projection advertises definitions the engine cannot serve, which is
//!    the failure `playbooks.md` exists to prevent.
//!
//! 2. **`workflows.md` is retired only through a receipt.** Retiring it without
//!    one is refused. The projection is a durable operator-facing artifact; a
//!    file that disappears with no record is indistinguishable from data loss.
//!
//! 3. **Generations dual-read, canonical-write.** Reads resolve BOTH
//!    `playbook_generations/` and the legacy `workflow_generations/`; writes go
//!    to the canonical directory only. When the SAME identity exists under both,
//!    the read REFUSES with a typed error naming both paths — it never merges,
//!    renames, or picks a winner. Ordering matters: dual-read ships BEFORE any
//!    migration of the legacy directory, so nothing is moved out from under a
//!    reader.
//!
//! 4. **The top-level `workflows/` -> `playbooks/` move FAILS LOUD.** It used to
//!    `eprintln!` a warning and continue, scanning canonical only — so a failed
//!    move left Foundry writing into a legacy location Anvil would not scan, and
//!    the only trace was a log line nobody reads.
//!
//! BASENAMES AND ARTIFACT IDS ARE PRESERVED. A directory basename containing
//! `playbook` (e.g. `20260528T2321_workflow_generation`) is NOT renamed for
//! aesthetics; only the containing directory moves. Renaming a basename changes
//! an artifact's identity, which is a data migration wearing a cosmetic disguise.
//!
//! DECLARED EXCLUSION: multi-process concurrent moves are out of scope for this
//! track. Two engines racing on the same hearth is a supervisor concern, not a
//! projection concern, and nothing here claims to serialize them.
//!
//! # NO BOOL-RETURNING FILESYSTEM PREDICATE MAY APPEAR IN THIS MODULE
//!
//! `Path::is_dir()`, `is_file()` and `exists()` map EVERY error to `false`, and
//! on this path `false` means "nothing here" — the one answer that concludes
//! nothing would be lost and clears a writer. Every filesystem question here
//! goes through [`crate::domain::playbook::fs_probe::node_kind`], which answers
//! `Result<NodeKind, _>`. `anvil-core/features/hearth_fail_loud_lint.feature`
//! REDS if any of the three predicates reappears in this file — at one of the
//! sites that carried it, or at a new one. See `fs_probe`'s module docs for why
//! the ban is mechanical rather than type-level.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::domain::playbook::fs_probe::{node_kind, NodeKind};

/// Canonical top-level hearth definition directory. The only one scanned.
pub const CANONICAL_HEARTH_DIR: &str = "playbooks";
/// Legacy top-level hearth definition directory. Migrated once, never scanned.
pub const LEGACY_HEARTH_DIR: &str = "workflows";
/// Canonical generations directory. New generations are written here.
pub const CANONICAL_GENERATIONS_DIR: &str = "playbook_generations";
/// Legacy generations directory. Still READ (dual-read); never written.
pub const LEGACY_GENERATIONS_DIR: &str = "workflow_generations";
/// Canonical registry projection filename.
pub const CANONICAL_REGISTRY_FILE: &str = "playbooks.md";
/// Legacy registry projection filename, retired through a receipt.
pub const LEGACY_REGISTRY_FILE: &str = "workflows.md";

/// Test-seam: force the hearth directory move to fail deterministically.
///
/// THE SAME SANCTIONED INJECTOR CLASS C-r.4 USES (plan.md §0.3): it injects an
/// IO failure at a production boundary; it does NOT replace the code under test.
/// The move still runs through the real function — only the syscall's result is
/// forced. Without it, scenario 5's red path is unconstructible on a filesystem
/// that cooperates, and an untestable failure branch is how the warn-and-continue
/// survived this long.
pub const MOVE_FAILURE_INJECTION_ENV: &str = "ANVIL_HEARTH_MOVE_FAILURE";

/// Typed failures for the C9 surfaces. Every one names the paths involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryProjectionError {
    /// The top-level `workflows/` -> `playbooks/` move could not complete.
    ///
    /// Registration MUST NOT proceed past this: continuing would write
    /// definitions into a location the scanner does not read.
    HearthDirectoryMoveFailed {
        from: PathBuf,
        to: PathBuf,
        cause: String,
    },
    /// The same generation identity exists under BOTH directories.
    ///
    /// Refused, never merged: picking a winner silently discards one side's
    /// history, and the two sides are not knowably equivalent.
    GenerationIdentityCollision {
        identity: String,
        legacy_path: PathBuf,
        canonical_path: PathBuf,
    },
    /// BOTH `playbooks/` and `playbooks/` exist at the top of the hearth.
    ///
    /// This used to be `Ok(false)` — "nothing to do". It was not nothing: the
    /// scan reads `playbooks/` ONLY, so every definition under `playbooks/`
    /// VANISHED SILENTLY from the registry, with no rename and no diagnostic.
    /// An operator reading the projection concludes those definitions were
    /// never there. Merging the two is not this code's decision to make: a
    /// same-basename pair under both roots is not knowably the same artifact,
    /// and picking one discards the other's history.
    HearthDirectoryCollision {
        legacy: PathBuf,
        canonical: PathBuf,
        /// Every definition under the legacy root — all of them are shadowed,
        /// because the scan never reads that root.
        shadowed: Vec<String>,
        /// The subset that also exists under canonical, i.e. the identities an
        /// automatic merge would have to resolve.
        colliding: Vec<String>,
    },
    /// `workflows.md` retirement was attempted with no receipt entry naming it.
    RetirementNotReceipted {
        path: PathBuf,
        /// What the receipt DID name, so the near miss is diagnosable rather
        /// than a bare refusal.
        receipt: Vec<String>,
    },
    /// A hearth root EXISTS and could not be ENUMERATED.
    ///
    /// C-d.1 round 4. This used to be `Vec::new()` — a `read_dir` failure became
    /// "this root holds nothing", and "holds nothing" is the input on which every
    /// guard here decides that nothing would be lost. On a legacy root that
    /// cannot be enumerated but holds a real definition, the shadowing refusal
    /// answered `Ok(false)`, `registration_blocked_detail()` answered `None`, and
    /// the persist WRITE boundary allowed the write — a silent fallback inside
    /// the guard whose entire stated purpose is to fail loud, and the trigger is
    /// ordinary: any `read_dir` failure, which on a desktop app reading hearths
    /// in user directories means TCC-protected locations, stale mounts, and
    /// descriptor exhaustion.
    ///
    /// "I could not enumerate this root" is the same class of answer as "I could
    /// not resolve that parent", which the receipt gate already rules is not an
    /// authorization. It is DISTINCT from an absent root: a root that does not
    /// exist genuinely holds nothing, and that stays `Ok(vec![])`.
    HearthRootUnreadable {
        root: PathBuf,
        cause: String,
    },
    /// A hearth root path exists and is NOT a directory.
    ///
    /// C-d.1 round 4. `{hearth}/workflows` as a plain FILE with canonical absent
    /// used to be RENAMED to `playbooks` and reported as `Ok(true)` — a
    /// successful migration that left the canonical root as a file, after which
    /// the loader's own `read_dir` failure was swallowed into an empty registry.
    /// The mirror case — `{hearth}/playbooks` as a FILE — refused with a message
    /// calling the file a "root", which is a diagnostic about a state that is not
    /// the one on disk.
    HearthRootNotADirectory {
        root: PathBuf,
    },
}

impl std::fmt::Display for RegistryProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HearthDirectoryMoveFailed { from, to, cause } => write!(
                f,
                "hearth_directory_move_failed: could not move {} -> {}: {cause}. \
                 Registration is refused before any definition is written, because a \
                 definition written to the legacy location would not be scanned.",
                from.display(),
                to.display()
            ),
            Self::GenerationIdentityCollision {
                identity,
                legacy_path,
                canonical_path,
            } => write!(
                f,
                "generation_identity_collision: identity {identity:?} exists under BOTH \
                 {} and {}. An explicit merge decision is required; this is not merged, \
                 renamed, or resolved by preference.",
                legacy_path.display(),
                canonical_path.display()
            ),
            Self::HearthDirectoryCollision {
                legacy,
                canonical,
                shadowed,
                colliding,
            } => write!(
                f,
                "hearth_directory_collision: BOTH {} and {} exist. Only the canonical root is \
                 scanned, so {} definition(s) under the legacy root are SHADOWED and would \
                 vanish from the registry with no diagnostic: {:?}. Of those, {:?} also exist \
                 under the canonical root. An explicit merge decision is required; this is not \
                 merged, renamed, or resolved by preference.",
                legacy.display(),
                canonical.display(),
                shadowed.len(),
                shadowed,
                colliding
            ),
            Self::RetirementNotReceipted { path, receipt } => write!(
                f,
                "retirement_not_receipted: {} may not be retired without a receipt entry naming \
                 it EXACTLY. The receipt names {:?} — a receipt naming a different file is not a \
                 receipt for this one, and acting on it would delete evidence the operator never \
                 recorded.",
                path.display(),
                receipt
            ),
            Self::HearthRootUnreadable { root, cause } => write!(
                f,
                "hearth_root_unreadable: {} EXISTS and could not be enumerated: {cause}. This is \
                 NOT the same answer as \"it holds nothing\" — every guard on this path decides \
                 what would be LOST from what it can enumerate, so reading an unenumerable root \
                 as empty authorizes exactly the silent loss the guard exists to refuse.",
                root.display()
            ),
            Self::HearthRootNotADirectory { root } => write!(
                f,
                "hearth_root_not_a_directory: {} exists and is NOT a directory. A hearth root is \
                 a directory of definitions; renaming a non-directory into the canonical root, \
                 or scanning one, produces a registry nothing can serve.",
                root.display()
            ),
        }
    }
}

/// One definition as the loader resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedDefinition {
    /// Directory basename — preserved exactly, never renamed.
    pub id: String,
    /// The governed artifact kind from the machine.
    pub governed_kind: String,
}

/// A directory present on disk that the loader did NOT resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedDirectory {
    pub id: String,
    pub reason: String,
}

/// Build the canonical `playbooks.md` body from the LOADED set.
///
/// `loaded` is what the production loader resolved. `excluded` is every
/// directory present on disk that it did not — and they are written INTO the
/// projection, because a projection that silently omits a directory is how an
/// operator concludes a definition was never there.
pub fn build_projection(
    loaded: &[ProjectedDefinition],
    excluded: &[ExcludedDirectory],
) -> String {
    let mut by_id: BTreeMap<&str, &ProjectedDefinition> = BTreeMap::new();
    for d in loaded {
        by_id.insert(d.id.as_str(), d);
    }

    let mut out = String::from("# Playbooks\n\n");
    out.push_str(
        "Projection of the definitions the loader RESOLVED. Built from the loaded set, never\n\
         from a directory listing: a directory that cannot be loaded is not a definition the\n\
         engine can serve, and advertising it here would be a promise nothing keeps.\n\n",
    );

    out.push_str("## active\n\n");
    if by_id.is_empty() {
        out.push_str("_No definition resolved._\n");
    } else {
        for d in by_id.values() {
            out.push_str(&format!("- [{}](playbooks/{}/) — governed kind `{}`\n", d.id, d.id, d.governed_kind));
        }
    }

    // The exclusions are part of the artifact, not a log line.
    out.push_str("\n## present on disk but not loaded\n\n");
    if excluded.is_empty() {
        out.push_str("_None._\n");
    } else {
        let mut sorted: Vec<&ExcludedDirectory> = excluded.iter().collect();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        for e in sorted {
            out.push_str(&format!("- `{}` — excluded: {}\n", e.id, e.reason));
        }
    }
    out
}

/// Does a receipt entry name EXACTLY this file, IN THIS HEARTH?
///
/// Substring matching was a data-loss path straight through the gate whose only
/// purpose is to prevent data loss: a receipt reading `backup_of_workflows.md.bak`
/// — a DIFFERENT file — contains the token `workflows.md` and therefore
/// authorized deleting the real `workflows.md`. The gate must act only on the
/// file the receipt names.
///
/// **Basename equality is NECESSARY AND NOT SUFFICIENT**, and shipping only the
/// basename test left the same class open one directory level up: a receipt
/// naming `/some/other/hearth/workflows.md` — genuine evidence, about a file in
/// a hearth this call is not operating on — authorized deleting THIS hearth's
/// registry. This function DELETES, so a false accept is data loss; the path
/// must resolve to this hearth or the receipt is about something else.
///
/// An entry is free-form operator prose (`retired: workflows.md`,
/// `retired /abs/path/workflows.md at 12:00`). A token names the file when:
///
/// * its basename equals `file` exactly (a name that merely embeds it is not a
///   match), AND
/// * it is a BARE basename, or its parent directory resolves — absolutely, via
///   the filesystem, so `..` and symlinks are followed rather than compared as
///   text — to `hearth` itself.
///
/// A token with a trailing separator names a DIRECTORY, and a directory is not
/// this file whatever it is called.
///
/// ## The bare-basename residual, and why it is now bounded (C-d.1 round 4)
///
/// `retired: workflows.md` must be accepted — it is the ordinary receipt, and
/// the previous review's own prescribed fix was "a bare basename, or a path
/// whose parent resolves to this hearth". But a bare basename is only
/// self-evidently about THIS hearth when the entry points nowhere else, and the
/// shipped tokenizer split on `[ ] ( )`, so a markdown link
/// `[workflows.md](/some/other/hearth/workflows.md)` DEGENERATED to a bare
/// basename and deleted this hearth's registry on evidence about another's.
/// `retired workflows.md from /some/other/hearth` did the same with no markdown
/// at all.
///
/// Two changes, both in the refusing direction, because this gate DELETES:
///
/// 1. Brackets and parentheses are NOT separators. `[workflows.md](x)` is one
///    token whose basename is not `workflows.md`, so it names nothing.
/// 2. A bare basename authorizes only when NO token in the same entry names a
///    LOCATION other than this hearth. An entry that mentions another place is
///    ambiguous about which `workflows.md` it is a receipt for, and ambiguity is
///    not authorization.
///
/// An explicit token whose parent resolves to this hearth still authorizes
/// regardless of the rest of the entry: it is unambiguous by construction.
///
/// ## Round 5: `/` was not the only way to name a place (M-3)
///
/// Round 4's `token_names_a_foreign_location` returned `false` for any token
/// with no `std::path::MAIN_SEPARATOR` in it, so on Unix EVERY location
/// expressed without `/` was invisible to it and the gate still DELETED on
/// `C:\other\hearth\workflows.md`, `\\server\share\workflows.md`, and a bare
/// relative directory naming another hearth. Those are unambiguously
/// path-shaped tokens naming another place; carrying them implicitly in a gate
/// that deletes is what this residual was written down to stop. A token now
/// names a foreign location when it is path-shaped **in any convention** —
/// `/`, `\` (Windows and UNC), a `~` home prefix, a `scheme://` URL — or when
/// it resolves to a real DIRECTORY beside or inside this hearth, and in every
/// case does not resolve to this hearth.
///
/// ## Round 5: two safe-direction false negatives, taken (L-3)
///
/// Backticks are now separators, so ``retired: `workflows.md` `` is accepted —
/// that is how this codebase's own prose writes a filename, and the operator
/// writing the retirement receipt for the live `workflows.md` by hand is the
/// one user this gate has. And a markdown link is now read by its TARGET rather
/// than discarded whole, so `[workflows.md](<this-hearth>/workflows.md)` — a
/// correctly-formed link to this hearth's own file — is accepted, while
/// `[workflows.md](x)` still refuses (its target names nothing) and
/// `[workflows.md](/other/hearth/workflows.md)` still refuses (its target is a
/// foreign location). Reading the target is strictly stronger than discarding
/// the token: the degenerate-to-bare-basename hole round 4 closed stays closed,
/// because the LABEL is never read.
///
/// DECLARED RESIDUAL, stated because this gate deletes files, and now stated
/// completely:
///
/// * An entry naming this file by bare basename alongside prose that identifies
///   another hearth with NO path-shaped token and NO token that resolves to a
///   real directory (`retired: workflows.md from the brine hearth`) is still
///   accepted. The tokenizer reads paths, not English.
/// * The sibling-directory test resolves a bare token against this hearth and
///   against this hearth's PARENT. A foreign hearth named by a bare directory
///   name that is neither is still invisible.
/// * Case folding is ASCII-only (`eq_ignore_ascii_case`), unchanged and still
///   declared: it is not APFS's Unicode folding.
fn receipt_names_exactly(entry: &str, hearth: &Path, file: &str) -> bool {
    let raw: Vec<&str> = entry
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '"' | '\'' | '`'))
        .filter(|token| !token.is_empty())
        .collect();
    // A markdown link contributes its TARGET, never its label. `[a](b)` is still
    // one token — round 4's fix — but the token it is read AS is `b`.
    let tokens: Vec<&str> = raw.into_iter().map(markdown_link_target).collect();

    // (a) An explicitly-scoped token is unambiguous on its own.
    if tokens
        .iter()
        .any(|token| token_names_file_in_hearth(token, hearth, file, false))
    {
        return true;
    }
    // (b) A bare basename authorizes only when the entry names no other place.
    if !tokens
        .iter()
        .any(|token| token_names_file_in_hearth(token, hearth, file, true))
    {
        return false;
    }
    !tokens
        .iter()
        .any(|token| token_names_a_foreign_location(token, hearth))
}

/// Does `token` name `file` in `hearth`? `bare` selects WHICH half is being
/// asked: `false` = an explicit path whose parent resolves to the hearth;
/// `true` = a bare basename with no directory component at all.
fn token_names_file_in_hearth(token: &str, hearth: &Path, file: &str, bare: bool) -> bool {
    if token.ends_with(std::path::MAIN_SEPARATOR) {
        return false;
    }
    let path = Path::new(token);
    if path.file_name().map(|name| name != file).unwrap_or(true) {
        return false;
    }
    match path.parent() {
        None => bare,
        Some(parent) if parent.as_os_str().is_empty() => bare,
        Some(parent) => {
            if bare {
                return false;
            }
            resolves_to_hearth(parent, hearth)
        }
    }
}

/// A token that carries a directory component and resolves somewhere OTHER than
/// this hearth — the thing whose presence makes a bare basename ambiguous.
///
/// Both the token itself and its parent are tested, because a receipt naming
/// another place names it either as a file (`/other/hearth/workflows.md`) or as
/// a directory (`from /other/hearth`).
fn token_names_a_foreign_location(token: &str, hearth: &Path) -> bool {
    let path = Path::new(token);
    if !token_is_location_shaped(token, hearth) {
        return false;
    }
    if resolves_to_hearth(path, hearth) {
        return false;
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => !resolves_to_hearth(parent, hearth),
        _ => true,
    }
}

/// Does `token` name a PLACE, in any convention this gate might meet?
///
/// C-d.1 round 5, M-3. This used to be `token.contains(MAIN_SEPARATOR)`, which
/// on Unix is `/` and nothing else — so `C:\other\hearth\workflows.md`,
/// `\\server\share\workflows.md` and a bare relative directory name were all
/// invisible to the ambiguity test, and the gate DELETED this hearth's registry
/// on a receipt about another's. Anvil being Unix-only is a reason those shapes
/// are unlikely; "unlikely" is the argument that already lost twice on this
/// gate.
fn token_is_location_shaped(token: &str, hearth: &Path) -> bool {
    // C-d.1 round 6, L-1. The shape test read the token's RAW bytes, so a
    // percent-encoded path — a real convention for writing one, and the form a
    // path takes the moment it passes through a URL, a log line or a shell
    // history — carried none of the four markers below and was invisible to the
    // ambiguity test. `%2Fother%2Fhearth%2Fworkflows.md` therefore left a bare
    // `workflows.md` beside it looking unambiguous, and this DELETING gate
    // deleted. Decoding the separators before the test is the fix, and it is in
    // the refusing direction: it can only ever make a token MORE location-shaped.
    let decoded = decode_percent_separators(token);
    let token: &str = &decoded;
    if token.contains(std::path::MAIN_SEPARATOR)
        || token.contains('/')
        || token.contains('\\')
        || token.starts_with('~')
        || token.contains("://")
    {
        return true;
    }
    // A bare name that IS a real directory beside or inside this hearth is a
    // place, whatever convention wrote it. Checked against the hearth and the
    // hearth's parent only — see the declared residual on
    // `receipt_names_exactly`.
    for base in [Some(hearth), hearth.parent()].into_iter().flatten() {
        match node_kind(&base.join(token)) {
            Ok(NodeKind::Directory) => return true,
            Ok(_) => {}
            // "I could not tell" is not an authorization. This predicate feeds a
            // DELETE, and answering `false` here would let an uninspectable
            // sibling clear a bare basename. Refusing costs an operator one more
            // explicit receipt; the other direction costs them the file.
            Err(_) => return true,
        }
    }
    false
}

/// Percent-decode ONLY the path separators, case-insensitively.
///
/// C-d.1 round 6, L-1. Deliberately not a general percent-decoder: this feeds a
/// SHAPE test, and the only question it has to answer is "does this token name a
/// place". `%2F` and `%5C` are the two encodings that hide a separator; decoding
/// `%20` or `%C3%A9` would change a filename's bytes and could turn a token that
/// names THIS hearth's file into one that does not, which is a change in the
/// ACCEPTING direction on a gate that deletes. Every substitution here can only
/// add a separator, so it can only make a token look MORE like a location.
///
/// DECLARED: an operator who double-encodes (`%252F`) still gets past the shape
/// test, and encodings other than `%2F`/`%5C` are not decoded. Recursive
/// decoding is not done — an unbounded decode loop on operator prose is a worse
/// idea than a declared residual.
fn decode_percent_separators(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let bytes = token.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // `is_char_boundary` is not decoration: `%` followed by a multi-byte
        // character (`%é`) would otherwise slice a char in half and PANIC, and
        // this function reads operator-written prose.
        if bytes[i] == b'%' && i + 3 <= bytes.len() && token.is_char_boundary(i + 3) {
            let hex = &token[i + 1..i + 3];
            if hex.eq_ignore_ascii_case("2f") {
                out.push('/');
                i += 3;
                continue;
            }
            if hex.eq_ignore_ascii_case("5c") {
                out.push('\\');
                i += 3;
                continue;
            }
        }
        // Push one whole char, never one byte: slicing a multi-byte character
        // in half would panic on the next `&token[..]`.
        let ch = token[i..].chars().next().expect("index is a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The TARGET of a markdown link token, or the token unchanged.
///
/// `[label](target)` -> `target`. The label is deliberately never read: a
/// tokenizer that split on brackets let `[workflows.md](/other/hearth/workflows.md)`
/// degenerate into the bare basename `workflows.md` and delete this hearth's
/// registry on evidence about another's. Reading the target keeps that closed
/// while letting a correctly-formed link to THIS hearth's own file authorize.
fn markdown_link_target(token: &str) -> &str {
    let Some(rest) = token.strip_prefix('[') else {
        return token;
    };
    let Some((_label, tail)) = rest.split_once("](") else {
        return token;
    };
    match tail.strip_suffix(')') {
        Some(target) if !target.is_empty() => target,
        _ => token,
    }
}

/// Does `candidate` resolve — through the filesystem, so `..` and symlinks are
/// followed rather than compared as text — to `hearth` itself? A relative
/// candidate is read RELATIVE TO THE HEARTH, never to the process cwd.
///
/// Unresolvable answers `false`. This gate deletes; "I could not tell" is not an
/// authorization.
fn resolves_to_hearth(candidate: &Path, hearth: &Path) -> bool {
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        hearth.join(candidate)
    };
    match (std::fs::canonicalize(&absolute), std::fs::canonicalize(hearth)) {
        (Ok(resolved), Ok(root)) => resolved == root,
        _ => false,
    }
}

/// Retire the legacy `workflows.md`, but ONLY with a receipt naming it EXACTLY.
///
/// `receipt` is the operator's record. If no entry names this file, retirement
/// is refused and the file is left byte-for-byte intact. "Names it" is exact
/// basename equality — see [`receipt_names_exactly`] for why a substring test
/// was a live deletion-on-mismatched-evidence path.
pub fn retire_legacy_registry(
    hearth: &Path,
    receipt: &[String],
) -> Result<Option<PathBuf>, RegistryProjectionError> {
    let legacy = hearth.join(LEGACY_REGISTRY_FILE);
    // C-d.1 round 5: was `legacy.exists()`. An unreadable hearth reported
    // "there is no legacy registry to retire" — a report of completion over a
    // file still on disk. Refusing is the only answer a retirement gate may give
    // to a question it could not ask.
    if kind_under(&legacy, hearth)? == NodeKind::Absent {
        return Ok(None);
    }
    if !receipt
        .iter()
        .any(|entry| receipt_names_exactly(entry, hearth, LEGACY_REGISTRY_FILE))
    {
        return Err(RegistryProjectionError::RetirementNotReceipted {
            path: legacy,
            receipt: receipt.to_vec(),
        });
    }
    std::fs::remove_file(&legacy).map_err(|e| {
        RegistryProjectionError::HearthDirectoryMoveFailed {
            from: legacy.clone(),
            to: legacy.clone(),
            cause: e.to_string(),
        }
    })?;
    Ok(Some(legacy))
}

/// Where a NEW generation is written. Canonical, always.
pub fn canonical_generations_dir(hearth: &Path) -> PathBuf {
    hearth.join(CANONICAL_GENERATIONS_DIR)
}

/// Resolve every generation identity, reading BOTH directories.
///
/// The ENUMERATION view: identity -> resolved path over the whole hearth. A
/// collision (same identity under both) is a typed refusal, not a preference,
/// and this shape fails WHOLE — one collision anywhere and the caller gets no
/// map at all, which is correct for "describe this hearth's generations" and
/// catastrophic for "where does identity X live?".
///
/// It is a FOLD of [`resolve_generation_identity`], which is the primitive the
/// production lookup runs per identity. The COLLISION RULE is therefore
/// literally the same code on both paths and cannot drift.
///
/// THE ENUMERATION CAN STILL DISAGREE WITH THE PRIMITIVE, and the earlier claim
/// that "they cannot drift into disagreeing" was too broad: this shape has to
/// LIST both roots and the primitive only has to `is_dir()` one path, so a root
/// that is traversable but not readable (`0300`) answered `Ok([])` here —
/// "this hearth has no generations" — while the primitive resolved a generation
/// under it. That disagreement is now LOUD rather than silent: an unenumerable
/// root is `Err(HearthRootUnreadable)`, so the whole-map shape refuses instead
/// of under-reporting. The primitive is deliberately unaffected — it answers
/// about ONE identity and needs no listing to do it.
pub fn resolve_generations(
    hearth: &Path,
) -> Result<BTreeMap<String, PathBuf>, RegistryProjectionError> {
    let canonical_root = hearth.join(CANONICAL_GENERATIONS_DIR);
    let legacy_root = hearth.join(LEGACY_GENERATIONS_DIR);

    // Union of both roots, sorted, so neither root's order decides anything.
    // A root that cannot be ENUMERATED is a typed refusal, not an empty union:
    // this shape's whole claim is "every generation identity in this hearth",
    // and a swallowed `read_dir` error turns that into "every one I happened to
    // be able to see".
    let mut identities: Vec<String> = list_dirs(&legacy_root)?;
    for id in list_dirs(&canonical_root)? {
        if !identities.contains(&id) {
            identities.push(id);
        }
    }
    identities.sort();

    let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
    for id in identities {
        if let Some(path) = resolve_generation_identity(hearth, &id)? {
            out.insert(id, path);
        }
    }
    Ok(out)
}

/// Resolve ONE generation identity across both roots.
///
/// The identity-scoped half of [`resolve_generations`], and the one a lookup for
/// a single artifact must use. `resolve_generations` answers a question about
/// the two DIRECTORIES and fails whole: one colliding identity anywhere makes it
/// `Err`. A caller that asks "where does identity X live?" and treats that `Err`
/// as its own answer converts a local refusal into a hearth-wide outage — every
/// bare-id lookup in the hearth, of any kind, returning `None` because two
/// unrelated generation directories disagree.
///
/// * `Ok(Some(path))` — resolved under exactly one root.
/// * `Ok(None)` — this identity is not a generation. The caller keeps looking.
/// * `Err(..)` — THIS identity exists under both roots. Refused, never merged,
///   never resolved by preference, and scoped to the identity asked about.
pub fn resolve_generation_identity(
    hearth: &Path,
    identity: &str,
) -> Result<Option<PathBuf>, RegistryProjectionError> {
    // An identity is a single directory basename. Anything with a separator in
    // it is a relative path, not a generation identity, and joining it here
    // would let a caller address outside the two roots.
    if identity.is_empty()
        || Path::new(identity).components().count() != 1
        || identity == "."
        || identity == ".."
    {
        return Ok(None);
    }
    let legacy = hearth.join(LEGACY_GENERATIONS_DIR).join(identity);
    let canonical = hearth.join(CANONICAL_GENERATIONS_DIR).join(identity);
    // C-d.1 round 5. This was `(legacy.is_dir(), canonical.is_dir())` — a FOURTH
    // site of the same class, which the reviewer's three reproductions did not
    // reach and which the module-wide ban is what actually found. An
    // uninspectable generations root answered `(false, false)` = `Ok(None)` =
    // "this identity is not a generation", and the caller went on looking
    // somewhere else. Same swallow, same shape, one function away.
    let legacy_kind = kind_under(&legacy, &legacy)?;
    let canonical_kind = kind_under(&canonical, &canonical)?;
    match (
        legacy_kind == NodeKind::Directory,
        canonical_kind == NodeKind::Directory,
    ) {
        (true, true) => Err(RegistryProjectionError::GenerationIdentityCollision {
            identity: identity.to_string(),
            legacy_path: legacy,
            canonical_path: canonical,
        }),
        (true, false) => Ok(Some(legacy)),
        (false, true) => Ok(Some(canonical)),
        (false, false) => Ok(None),
    }
}

/// The subdirectories of `root` that carry a `machine.yaml` — the ones the
/// loader would have resolved as definitions.
///
/// Separate from [`list_dirs`] on purpose: generations are identified by their
/// directory alone (no `machine.yaml` in one), so the two questions are
/// genuinely different and collapsing them is what made the collision boundary
/// disagree with its own comment.
fn list_definition_dirs(root: &Path) -> Result<Vec<String>, RegistryProjectionError> {
    let mut out: Vec<String> = Vec::new();
    for id in list_dirs(root)? {
        // C-d.1 round 5. This was a `.filter(|id| … .is_file())`. It is THE site
        // the round-4 review reproduced as "the exact HIGH-2 signature": a legacy
        // root perfectly readable, ONE definition directory at `0000`, so the
        // `stat` of `{id}/machine.yaml` returned EACCES, `is_file()` answered
        // `false`, the definition dropped out of the shadowed set, the guard
        // answered `Ok(NothingToMove)` — and the persist write boundary emitted
        // its event. A definition that cannot be inspected is not a definition
        // that is not there.
        match kind_under(&root.join(&id).join("machine.yaml"), root)? {
            NodeKind::File | NodeKind::Other => out.push(id),
            NodeKind::Absent | NodeKind::Directory => {}
        }
    }
    Ok(out)
}

/// Enumerate `root`'s subdirectory basenames, DISTINGUISHING "there is nothing
/// here" from "I could not look".
///
/// * root absent          -> `Ok(vec![])`  — genuinely nothing, and the callers'
///   "nothing would be lost" conclusion is sound.
/// * root present, unreadable -> `Err(HearthRootUnreadable)` — the callers' whole
///   predicate is about what would be LOST, and an unenumerable root is the one
///   input on which "nothing would be lost" cannot be concluded.
/// * an individual entry unreadable -> `Err` as well: a partial listing is a
///   complete-looking answer to an incomplete question.
fn list_dirs(root: &Path) -> Result<Vec<String>, RegistryProjectionError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        // An ABSENT root holds nothing, and that is a real answer.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(RegistryProjectionError::HearthRootUnreadable {
                root: root.to_path_buf(),
                cause: e.to_string(),
            })
        }
    };
    let mut out: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| RegistryProjectionError::HearthRootUnreadable {
            root: root.to_path_buf(),
            cause: e.to_string(),
        })?;
        // C-d.1 round 5. This was `entry.path().is_dir()`. `read_dir` succeeding
        // and the per-entry `stat` failing is not a corner case: it is exactly
        // what a root at mode `0600`/`0400` does (readable, not traversable) and
        // exactly what a STALE NETWORK MOUNT does (`readdir` answers from cache,
        // `stat` returns ESTALE/EIO). `is_dir()` answered `false` for all of it,
        // so the entry silently left the listing and the caller concluded the
        // root held nothing.
        match kind_under(&entry.path(), root)? {
            NodeKind::Directory => out.push(entry.file_name().to_string_lossy().to_string()),
            NodeKind::Absent | NodeKind::File | NodeKind::Other => {}
        }
    }
    out.sort();
    Ok(out)
}

/// [`node_kind`] with its `io::Error` mapped onto the typed refusal.
///
/// Names BOTH the root the question is about — which is what an operator has to
/// fix — and the exact path that could not be inspected, which is what tells
/// them where. The whole point of this module's guard is that the caller cannot
/// receive "nothing here" when the truth is "I could not look".
fn kind_under(path: &Path, root: &Path) -> Result<NodeKind, RegistryProjectionError> {
    node_kind(path).map_err(|e| RegistryProjectionError::HearthRootUnreadable {
        root: root.to_path_buf(),
        cause: format!("{} could not be inspected: {e}", path.display()),
    })
}

/// Move a legacy `workflows/` hearth dir to canonical `playbooks/`, FAIL LOUD.
///
/// Replaces the warn-and-continue this module's header describes. Returns
/// `Ok(false)` when there was genuinely nothing to move (no legacy dir) and
/// `Ok(true)` when a move completed. Any failure is a typed error the caller
/// MUST NOT swallow.
///
/// BOTH DIRECTORIES PRESENT IS A REFUSAL, not a no-op. The previous
/// `canonical.exists() => Ok(false)` reported "nothing to do" for the one state
/// where something was badly wrong: the scan reads canonical only, so every
/// definition under the legacy root vanished from the registry silently. See
/// [`RegistryProjectionError::HearthDirectoryCollision`].
///
/// Basenames inside the moved directory are untouched — this moves the container,
/// never its contents' identities.
pub fn migrate_legacy_hearth_dir(hearth: &Path) -> Result<bool, RegistryProjectionError> {
    match hearth_root_diagnosis(hearth)? {
        HearthRootState::NothingToMove => Ok(false),
        HearthRootState::MovePending { legacy, canonical } => {
            if let Some(injected) = std::env::var_os(MOVE_FAILURE_INJECTION_ENV) {
                if !injected.is_empty() {
                    return Err(RegistryProjectionError::HearthDirectoryMoveFailed {
                        from: legacy,
                        to: canonical,
                        cause: format!("injected failure ({})", injected.to_string_lossy()),
                    });
                }
            }
            std::fs::rename(&legacy, &canonical).map_err(|e| {
                RegistryProjectionError::HearthDirectoryMoveFailed {
                    from: legacy,
                    to: canonical,
                    cause: e.to_string(),
                }
            })?;
            Ok(true)
        }
    }
}

/// What [`hearth_root_diagnosis`] concluded about the two top-level roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HearthRootState {
    /// Nothing to move, and nothing at risk: no legacy root, or a legacy root
    /// that is the SAME DIRECTORY as canonical, or one holding no definition the
    /// loader would have resolved.
    NothingToMove,
    /// A legacy root exists, canonical does not, and a migration would move it.
    MovePending { legacy: PathBuf, canonical: PathBuf },
}

/// The two hearth roots as ONE READ describes them — owned, already-fallible,
/// and carrying **no `Path`**.
///
/// # C-d.1 round 6: why this type exists
///
/// The same defect was filed four times. Each round removed the swallow from
/// the expression that had just been caught, and the next round found it in the
/// next expression. Round 5 added a source scan to stop the next spelling; an
/// independent review then wrote the same swallow **ten** different ways, all
/// of which compiled and left the scan green — one of them differing from a
/// caught spelling only by where a newline falls.
///
/// A lexical test cannot close a class whose every instance is a new lexeme.
/// The only thing that can is structural: **code that holds no `Path` cannot
/// probe one, in any spelling, in any module, and that is a compile-time
/// property rather than a scan.** So the filesystem is read ONCE, at one edge
/// ([`survey_hearth_roots`]), into this value — and [`decide_hearth_roots`],
/// which is the code that actually concludes "nothing would be lost" and clears
/// a writer, receives only this.
///
/// `NodeKind`, `bool` and `String` have no `is_dir`, no `is_file`, no `exists`.
/// There is nothing in scope to call them on. `Path::is_dir(&p)`, an alias, a
/// split line, `matches!(metadata(p), Ok(_))`, a helper in a third module — none
/// of them is *detected* in the decision; every one of them **fails to compile**,
/// because there is no `p`.
///
/// # Why a sum type rather than a struct of `Option`s
///
/// The reads are CONDITIONAL: an absent legacy root settles the question before
/// canonical is ever inspected, and the shadowed set is only computed when there
/// are two distinct directories to shadow between. A struct would have to spell
/// "this was never asked" as `None`, and a decision function that must handle
/// "never asked" acquires an arm no state can reach — an unfailable branch, which
/// is the other defect this track keeps being caught by. Here **"not asked" is
/// unrepresentable**, and the decision is a total match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HearthRootFacts {
    /// `{hearth}/playbooks` does not exist. Nothing under it, nothing at risk.
    LegacyAbsent,
    /// `{hearth}/playbooks` exists and is not a directory.
    LegacyNotADirectory,
    /// `{hearth}/playbooks` is a directory. What canonical is decides the rest.
    LegacyIsADirectory(CanonicalFacts),
}

/// What `{hearth}/playbooks` is, asked only once the legacy root is a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalFacts {
    /// Canonical does not exist: the legacy root can move into its place.
    Absent,
    /// Both paths resolve to ONE directory — the `workflows -> playbooks`
    /// symlink an operator leaves behind. Two paths to one directory shadow
    /// nothing: there is no second copy to lose.
    SameDirectoryAsLegacy,
    /// Canonical exists and is not a directory.
    NotADirectory,
    /// Two distinct directories, so the scan reads canonical ONLY and every
    /// definition under legacy is shadowed.
    Distinct {
        /// Directory basenames under the legacy root carrying a `machine.yaml`
        /// — the definitions the loader WOULD have resolved, and therefore the
        /// ones that are lost. A directory with no `machine.yaml` is not a
        /// definition either root could have served.
        shadowed: Vec<String>,
        /// The subset of `shadowed` that also exists under canonical. A
        /// DIAGNOSTIC SUBSET, never the trigger — see [`decide_hearth_roots`].
        colliding: Vec<String>,
    },
}

/// The verdict, also path-free, so the decision cannot name a place either.
///
/// The edge that holds the two `PathBuf`s turns this into the typed error. That
/// split is the point: the decision decides, and the code that owns the paths
/// reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HearthRootVerdict {
    NothingToMove,
    MovePending,
    LegacyNotADirectory,
    CanonicalNotADirectory,
    Collision {
        shadowed: Vec<String>,
        colliding: Vec<String>,
    },
}

/// THE DECISION. Takes facts, returns a verdict, touches no filesystem.
///
/// This is the function that concludes "nothing would be lost", and that
/// conclusion is the one input on which every guard on this path clears a
/// writer.
///
/// **C-d.1 round 7's M-1 correction, reaching this site in round 8 — this
/// comment used to end "so the swallow is not banned here, it is *unwritable*",
/// and that is false.** The round-6 reviewer falsified it by construction:
/// three bypasses written INSIDE this function, with the binding created on the
/// line above, compile and leave both the lint and the projection suite green.
/// The correction was made in four hearth sites and two shipping files and this
/// third shipping file — the doc comment on the function the claim is ABOUT —
/// was not grepped for.
///
/// **The property that IS true, stated precisely and not more:** *the decision
/// holds no path naming the hearth.* `HearthRootFacts` carries `Vec<String>`
/// basenames and unit variants and nothing else, so a predicate written here can
/// only probe a path the author constructs out of thin air, which names a place
/// rather than THIS hearth's roots. To make the decision answer wrongly about
/// this hearth you must also change the edge or the type. That is narrower than
/// "unwritable", it is real, and it is a stronger property than a ban list.
///
/// The one real rule it carries is the last arm, and that rule caused a live
/// outage when it was keyed wrong: refusing on a NAME COLLISION rather than on
/// SHADOWING passed every scenario written against a fixture that always seeded a
/// shared identity, while silently dropping the likelier state — a partially
/// migrated hearth whose legacy definitions have no canonical namesake. Those
/// are exactly as lost, because the scan reads canonical only. And in the other
/// direction, counting any subdirectory as shadowed made a leftover empty
/// directory under the legacy root hard-refuse every write into a hearth that
/// had already finished migrating and had zero definitions at risk. The refusal
/// is keyed to what is LOST.
fn decide_hearth_roots(facts: &HearthRootFacts) -> HearthRootVerdict {
    match facts {
        HearthRootFacts::LegacyAbsent => HearthRootVerdict::NothingToMove,
        HearthRootFacts::LegacyNotADirectory => HearthRootVerdict::LegacyNotADirectory,
        HearthRootFacts::LegacyIsADirectory(canonical) => match canonical {
            CanonicalFacts::Absent => HearthRootVerdict::MovePending,
            CanonicalFacts::SameDirectoryAsLegacy => HearthRootVerdict::NothingToMove,
            CanonicalFacts::NotADirectory => HearthRootVerdict::CanonicalNotADirectory,
            CanonicalFacts::Distinct {
                shadowed,
                colliding,
            } => {
                if shadowed.is_empty() {
                    HearthRootVerdict::NothingToMove
                } else {
                    HearthRootVerdict::Collision {
                        shadowed: shadowed.clone(),
                        colliding: colliding.clone(),
                    }
                }
            }
        },
    }
}

/// THE EDGE. Every filesystem read the hearth-root guard performs is here, and
/// nowhere else on the decision path.
///
/// This function holds the paths, so this function is where a bool-returning
/// predicate could still be written — that is not eliminated, it is
/// CONCENTRATED, into one function of five reads whose entire output is a value
/// with no `Path` in it. Concentrating it is the whole gain: reviewing five
/// reads in one place is a job that finishes, and reviewing "every expression on
/// the guard path" is the job that has now failed four times.
///
/// The reads are ordered and conditional so that no question is asked whose
/// answer cannot matter — an absent legacy root settles it without touching
/// canonical, and the shadowed set is computed only when there are two distinct
/// directories for one to shadow the other.
fn survey_hearth_roots(
    legacy: &Path,
    canonical: &Path,
) -> Result<HearthRootFacts, RegistryProjectionError> {
    // C-d.1 round 5. These were `legacy.exists()` and `legacy.is_dir()`. An
    // unreadable `{hearth}` itself made BOTH answer `false`, so a hearth nobody
    // could look into answered "no legacy root — nothing to move" and cleared
    // every caller. One `stat`, one typed answer, four distinguishable states.
    match kind_under(legacy, legacy)? {
        NodeKind::Absent => return Ok(HearthRootFacts::LegacyAbsent),
        // `{hearth}/workflows` as a plain FILE used to be RENAMED to `playbooks`
        // and reported as a completed migration, leaving the canonical root a
        // file that the scan then failed to enumerate — silently.
        NodeKind::File | NodeKind::Other => return Ok(HearthRootFacts::LegacyNotADirectory),
        NodeKind::Directory => {}
    }

    let canonical_kind = kind_under(canonical, canonical)?;
    if canonical_kind == NodeKind::Absent {
        return Ok(HearthRootFacts::LegacyIsADirectory(CanonicalFacts::Absent));
    }
    // SELF-SHADOW IS NOT SHADOWING. `{hearth}/playbooks` as a SYMLINK TO
    // `{hearth}/playbooks` presented as "both roots exist" and refused, naming
    // the canonical root's OWN definitions as shadowed — and once the refusal
    // was wired to the persist WRITE boundary, that false positive hard-refused
    // every write into a perfectly healthy hearth.
    if let (Ok(l), Ok(c)) = (
        std::fs::canonicalize(legacy),
        std::fs::canonicalize(canonical),
    ) {
        if l == c {
            return Ok(HearthRootFacts::LegacyIsADirectory(
                CanonicalFacts::SameDirectoryAsLegacy,
            ));
        }
    }
    if canonical_kind != NodeKind::Directory {
        return Ok(HearthRootFacts::LegacyIsADirectory(
            CanonicalFacts::NotADirectory,
        ));
    }

    let shadowed = list_definition_dirs(legacy)?;
    // C-d.1 round 5: this was `canonical.join(id).exists()`. Only a diagnostic
    // subset, but a swallow here UNDER-reports the collision in an
    // operator-facing string.
    let mut colliding: Vec<String> = Vec::new();
    for id in &shadowed {
        if kind_under(&canonical.join(id), canonical)? != NodeKind::Absent {
            colliding.push(id.clone());
        }
    }
    Ok(HearthRootFacts::LegacyIsADirectory(
        CanonicalFacts::Distinct {
            shadowed,
            colliding,
        },
    ))
}

/// The hearth-root guard, computed WITHOUT MUTATING ANYTHING.
///
/// C-d.1 round 4. `migrate_legacy_hearth_dir` was both the guard and the
/// mutation, so every caller that merely wanted to ask "may this hearth be
/// registered into?" performed a top-level directory move to find out — and
/// `HearthPlaybookRegistry::new()` is on that path, which put a live-hearth
/// structural mutation AHEAD of the guard that governs it on both engine persist
/// seams. This function is the question; `migrate_legacy_hearth_dir` is the
/// question followed by the answer's consequence. A caller that only needs the
/// answer must call THIS one.
///
/// It reads the filesystem and writes nothing. Every refusal below is a state in
/// which a move must not happen, so the mutation is unreachable while the guard
/// refuses.
/// C-d.1 round 6: THREE STATEMENTS, and they are the whole guard.
///
/// 1. join the two paths;
/// 2. read the filesystem ONCE, at the edge, into a value with no `Path` in it;
/// 3. decide from that value, and report using the paths this function still
///    holds.
///
/// The decision is [`decide_hearth_roots`] and it receives no path, so no
/// spelling of a bool-returning filesystem predicate can be written inside it.
/// Every filesystem read is in [`survey_hearth_roots`] and nowhere else on this
/// path. See [`HearthRootFacts`] for why that is the fix and a source scan is
/// not.
pub fn hearth_root_diagnosis(hearth: &Path) -> Result<HearthRootState, RegistryProjectionError> {
    let legacy = hearth.join(LEGACY_HEARTH_DIR);
    let canonical = hearth.join(CANONICAL_HEARTH_DIR);
    let facts = survey_hearth_roots(&legacy, &canonical)?;
    match decide_hearth_roots(&facts) {
        // Canonical alone may still be a non-directory; that is the LOADER's
        // problem (it reports its own unreadable root) and not a move decision.
        HearthRootVerdict::NothingToMove => Ok(HearthRootState::NothingToMove),
        HearthRootVerdict::MovePending => Ok(HearthRootState::MovePending { legacy, canonical }),
        HearthRootVerdict::LegacyNotADirectory => {
            Err(RegistryProjectionError::HearthRootNotADirectory { root: legacy })
        }
        HearthRootVerdict::CanonicalNotADirectory => {
            Err(RegistryProjectionError::HearthRootNotADirectory { root: canonical })
        }
        HearthRootVerdict::Collision {
            shadowed,
            colliding,
        } => Err(RegistryProjectionError::HearthDirectoryCollision {
            legacy,
            canonical,
            shadowed,
            colliding,
        }),
    }
}
