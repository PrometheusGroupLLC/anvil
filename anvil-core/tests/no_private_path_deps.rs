//! No crate in the DEFAULT workspace may reach outside this repository.
//!
//! Anvil must build from a bare clone, with no private sibling checkouts. That
//! held exactly once and then rotted, because the way it breaks is invisible:
//! cargo loads the manifest of every path dependency reachable from a workspace
//! member — dev-dependencies included, and switched-off OPTIONAL dependencies
//! included — so a single `path = "../../something"` anywhere in the default
//! workspace makes `cargo metadata --no-deps` exit 101 for everyone without
//! that sibling. "It's only a dev-dep" and "it's optional" buy nothing.
//!
//! This guard fails the moment such a dependency reappears.
//!
//! # It has to check three things, because it was breached in three ways
//!
//! 1. **Escaping path deps in a MEMBER manifest.** `anvil-engine` had
//!    `foundry-kit-broker-client = { path = "../../foundry/…", optional = true }`.
//! 2. **Escaping path deps in the ROOT manifest.** A `[patch.crates-io]` block
//!    there pointed at `../foundry/crates/…`. The root is not a workspace
//!    member, so a guard that loops over the member list only is structurally
//!    blind to this — and this guard WAS, for as long as the patch block
//!    existed.
//! 3. **A `[patch.crates-io]` section at all.** Even pointing somewhere
//!    harmless, it is the mechanism by which private siblings got wired in, and
//!    a bare clone has nothing for it to resolve against.
//!
//! Escape is decided by RESOLVING each path against the manifest's own
//! directory and asking whether the result is inside the repository — not by
//! counting `../`. The old predicate matched the literal `path = "../../`, so a
//! single-`../` escape (exactly what the root patch block used) sailed straight
//! through, while a rule loose enough to catch it would also have flagged
//! `../anvil-core`, which is how this workspace is legitimately wired.
//! Resolution distinguishes the two; substring matching cannot.
//!
//! `std::fs::canonicalize` cannot be the PRIMARY rule here: the whole point is
//! that `../foundry` does not exist on the machine where this must fail, and
//! canonicalize errors on a missing path. The components are normalised by hand
//! instead. This mirrors the gate in
//! `scripts/anvil-public-staging-EXPORT.sh` ("3. THE ACTUAL GATE"), which
//! checks the exported tree the same way.
//!
//! Nor is canonicalize used as a second rule, and that took a measurement to
//! learn. Lexical normalisation is symlink-blind — `path = "vendor/broker"`
//! where `vendor/broker` is a symlink to `../../../foundry/…` resolves "inside
//! the repository" and reads clean while cargo exits 101 on a bare clone — so
//! `escapes_via_symlink` covers that hole. The first version of it called
//! `canonicalize` and MEASURED GREEN on the very mutation it was written for,
//! because on a bare clone the link's target is exactly what does not exist and
//! canonicalize returns `Err` for a dangling link. It reads the link and
//! resolves it with the same lexical `normalize` instead, so a dangling escape
//! is caught like a live one.

use std::path::{Component, Path, PathBuf};

fn repo_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in start.ancestors() {
        if dir.join("anvil-core").join("features").is_dir() {
            return dir.to_path_buf();
        }
    }
    panic!("could not locate the repository root above {}", start.display());
}

/// The default workspace's members, per `Cargo.toml`.
const DEFAULT_WORKSPACE_MEMBERS: &[&str] =
    &["anvil-core", "anvil-core-hearth", "anvil-engine", "anvil-mcp"];

/// THERE ARE NO TOLERATED ESCAPES. Deliberately empty.
///
/// This list once held `foundry-kit-broker-client`, on the reasoning that it
/// sat behind the off-by-default `foundry-session` feature so "its absence does
/// not break a default `cargo build`". THAT CLAIM WAS FALSE, and it is the hole
/// this guard exists to close: cargo resolves the workspace's path graph BEFORE
/// it consults the feature graph, so it loaded that manifest whether or not the
/// feature was on. Measured on a machine with no `../foundry` sibling, both
/// `cargo metadata --no-deps` and `cargo check --workspace --bins --lib` exited
/// 101 — the workspace could not be ENUMERATED, let alone built.
///
/// An exemption phrased as "but it's optional" is therefore not a narrow
/// exemption; it is the whole failure. A crate that needs a private sibling
/// belongs in a workspace the default one cannot reach — `brine-tests/` for
/// `brine`, `kit-build/` for `foundry-kit-broker-client` — and then it needs no
/// exemption here at all.
const ALLOWED_ESCAPES: &[&str] = &[];

/// Normalise a path lexically: absolute-ise against `base`, then fold away `.`
/// and `..` without touching the filesystem.
///
/// Lexical on purpose. `canonicalize` would be more faithful for symlinks but
/// returns `Err` for a path that does not exist, and the paths this guard must
/// catch are precisely the ones that do not exist on a bare clone. A guard that
/// silently skips the missing case catches nothing.
///
/// A `..` that would climb above the root is dropped rather than kept, which is
/// conservative in the RIGHT direction: `/a/../../b` normalises to `/b`, still
/// outside a repo at `/a/repo`, so it is still flagged.
fn normalize(base: &Path, raw: &str) -> PathBuf {
    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    };

    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What the extractor made of one `path =` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extracted {
    /// The decoded string value.
    Value(String),
    /// A `path =` this extractor could NOT read. Never silently dropped.
    ///
    /// A skipped declaration is the single most dangerous outcome available to
    /// this guard: it reports a confident pass over a manifest it did not
    /// understand. The previous version returned early on three separate
    /// unreadable shapes and this is exactly how the `"""` bypass worked — it
    /// read the value as the empty string and moved on green. Anything the
    /// reader cannot decode is now a FINDING, so the failure mode is a false
    /// alarm somebody investigates rather than a silence nobody sees.
    Unreadable(&'static str),
}

/// Every `path = <string>` declaration in a manifest, with its line number and
/// the source line for context.
///
/// Hand-rolled rather than regex: this crate has no regex dev-dep. It reads
/// TOML string values properly rather than "first quote to next quote", which
/// is what the four spellings below have in common:
///
///   path = "../x"        spaced, basic string
///   path="../x"          unspaced
///   path = '../x'        TOML LITERAL string — single quotes are valid TOML
///   path = """../x"""    multi-line basic string
///   path = '''../x'''    multi-line literal string
///   path = "../x"   escapes inside a basic string
///   { package = "y", path = "../x" }   inline table, key not first
///
/// # What this does NOT cover — stated because the previous comment claimed
/// # total coverage and was believed
///
/// The old version of this comment said the extractor "must nevertheless
/// tolerate every form TOML allows." It did not, and saying so is what stopped
/// the next reader from testing it: `path = """../../foundry/…"""` grabbed the
/// first `"`, found the second `"` immediately after it, yielded the EMPTY
/// string, normalised that to the manifest's own directory, and read as inside
/// the repository. The guard was green while cargo exited 101.
///
/// Known remaining gaps, so the next reader starts from the truth:
///
/// * This is a LINE-ORIENTED comment stripper. A whole line whose first
///   non-space character is `#` is ignored. A trailing `# comment` after a
///   value is not stripped (harmless — the value is already read), and a line
///   inside a multi-line string that happens to start with `#` is wrongly
///   treated as a comment.
/// * A `path` key spelled with a quoted key (`"path" = "../x"`) is not
///   recognised as the key. It is valid TOML. Nothing in this repository uses
///   it, and the empty-value and unreadable rules do not help here.
/// * Values are read, not evaluated: an inline table or array on the right of
///   `path =` is skipped (neither is valid for this key).
///
/// The key must be the WHOLE key, not a suffix: `hearth_path = "…"` is a
/// different key and matching it would produce phantom findings that get the
/// guard deleted.
fn path_values(text: &str) -> Vec<(usize, Extracted, String)> {
    let bytes = text.as_bytes();
    let mut line_starts = vec![0usize];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_index = |offset: usize| match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i - 1,
    };

    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find("path") {
        let at = search_from + rel;
        search_from = at + 4;

        let li = line_index(at);
        let line = text[line_starts[li]..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim_end_matches('\r')
            .to_string();
        if line.trim_start().starts_with('#') {
            continue; // prose, not a declaration
        }

        // Whole-key check: the character before must not be part of an
        // identifier, or this is the tail of `hearth_path` / `out_path`.
        if at > 0 {
            let prev = bytes[at - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-' {
                continue;
            }
        }

        // TOML requires the `=` on the same line as the key, so only spaces
        // and tabs may be skipped here — never a newline.
        let rest = text[at + 4..].trim_start_matches([' ', '\t']);
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start_matches([' ', '\t']);

        let Some((extracted, consumed)) = read_toml_string(rest) else {
            continue; // not a string value at all; not a path dep
        };
        out.push((li + 1, extracted, line));
        // Continue scanning after this value, for inline tables that carry two
        // and for multi-line strings that span past this line.
        search_from = (text.len() - rest.len()) + consumed;
    }
    out
}

/// Read one TOML string value from the start of `s`, in all four forms.
///
/// Returns `None` when `s` does not begin a string at all (an inline table, an
/// array, a bare word) — that is not a path dependency and never was. Returns
/// `Some(Unreadable(..))` when it DOES begin a string that cannot be read to
/// its end, which is a finding rather than a skip.
///
/// The `usize` is how many bytes of `s` the value occupied, so the caller can
/// resume scanning after it.
fn read_toml_string(s: &str) -> Option<(Extracted, usize)> {
    // Order matters: `"""` must be tested before `"`.
    for (open, multiline, literal) in [
        ("\"\"\"", true, false),
        ("'''", true, true),
        ("\"", false, false),
        ("'", false, true),
    ] {
        let Some(body) = s.strip_prefix(open) else {
            continue;
        };
        if literal {
            // Literal strings have NO escapes; the delimiter ends them.
            let hunt = if multiline {
                body.find(open)
            } else {
                // A single-quoted literal cannot span a newline.
                match body.find('\n') {
                    Some(nl) => body[..nl].find(open),
                    None => body.find(open),
                }
            };
            let Some(end) = hunt else {
                return Some((
                    Extracted::Unreadable(if multiline {
                        "unterminated multi-line literal string (''')"
                    } else {
                        "unterminated literal string (')"
                    }),
                    open.len() + body.len(),
                ));
            };
            let raw = &body[..end];
            let raw = if multiline { trim_opening_newline(raw) } else { raw };
            return Some((
                Extracted::Value(raw.to_string()),
                open.len() + end + open.len(),
            ));
        }

        // Basic strings honour backslash escapes, so the closing delimiter is
        // the first UNESCAPED one. "first quote to next quote" is precisely the
        // bug this replaces.
        let mut decoded = String::new();
        let mut it = body.char_indices();
        while let Some((i, c)) = it.next() {
            match c {
                '\\' => {
                    let Some((_, esc)) = it.next() else {
                        return Some((
                            Extracted::Unreadable("string ends in a dangling backslash"),
                            open.len() + body.len(),
                        ));
                    };
                    match esc {
                        'b' => decoded.push('\u{8}'),
                        't' => decoded.push('\t'),
                        'n' => decoded.push('\n'),
                        'f' => decoded.push('\u{c}'),
                        'r' => decoded.push('\r'),
                        '"' => decoded.push('"'),
                        '\\' => decoded.push('\\'),
                        'u' | 'U' => {
                            let width = if esc == 'u' { 4 } else { 8 };
                            let start = i + 2;
                            let Some(hex) = body.get(start..start + width) else {
                                return Some((
                                    Extracted::Unreadable("truncated \\u escape"),
                                    open.len() + body.len(),
                                ));
                            };
                            let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                            else {
                                return Some((
                                    Extracted::Unreadable("invalid \\u escape"),
                                    open.len() + body.len(),
                                ));
                            };
                            decoded.push(ch);
                            for _ in 0..width {
                                it.next();
                            }
                        }
                        // A backslash at end-of-line in a MULTI-LINE basic
                        // string swallows the newline and the whitespace after
                        // it. That is a real way to spell a path across lines.
                        '\n' | '\r' | ' ' | '\t' if multiline => {
                            let run = &body[i + 1..];
                            let after = run.trim_start_matches([' ', '\t', '\r', '\n']);
                            let run_len = run.len() - after.len();
                            if !run[..run_len].contains('\n') {
                                return Some((
                                    Extracted::Unreadable("unknown backslash escape"),
                                    open.len() + body.len(),
                                ));
                            }
                            // The escape char at i+1 is already consumed; skip
                            // the remainder of the whitespace run (ASCII, so
                            // byte count == char count).
                            for _ in 1..run_len {
                                it.next();
                            }
                        }
                        _ => {
                            return Some((
                                Extracted::Unreadable("unknown backslash escape"),
                                open.len() + body.len(),
                            ));
                        }
                    }
                }
                '"' => {
                    if !multiline {
                        return Some((
                            Extracted::Value(decoded),
                            open.len() + i + 1,
                        ));
                    }
                    if body[i..].starts_with("\"\"\"") {
                        let raw = trim_opening_newline(&decoded);
                        return Some((
                            Extracted::Value(raw.to_string()),
                            open.len() + i + 3,
                        ));
                    }
                    decoded.push('"');
                }
                '\n' if !multiline => {
                    return Some((
                        Extracted::Unreadable("unterminated basic string (\")"),
                        open.len() + i,
                    ));
                }
                other => decoded.push(other),
            }
        }
        return Some((
            Extracted::Unreadable(if multiline {
                "unterminated multi-line basic string (\"\"\")"
            } else {
                "unterminated basic string (\")"
            }),
            open.len() + body.len(),
        ));
    }
    None
}

/// TOML drops a newline that immediately follows a multi-line opening delimiter.
fn trim_opening_newline(s: &str) -> &str {
    s.strip_prefix("\r\n").or_else(|| s.strip_prefix('\n')).unwrap_or(s)
}

/// Collect `(line_number, line)` for every `path = "..."` that resolves to
/// somewhere outside `repo_root`, plus a `[patch.crates-io]` declaration if the
/// manifest has one.
fn escaping_declarations(manifest: &Path, repo_root: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
    let dir = manifest
        .parent()
        .expect("a manifest has a parent directory");

    let mut problems: Vec<(usize, String)> = Vec::new();
    for (line_no, extracted, line) in path_values(&text) {
        let value = match extracted {
            // An unreadable declaration is a finding, NOT a skip. See the
            // `Extracted::Unreadable` doc comment.
            Extracted::Unreadable(why) => {
                problems.push((
                    line_no,
                    format!("declares a `path` this guard cannot read ({why}): {line}"),
                ));
                continue;
            }
            Extracted::Value(v) => v,
        };

        // An EMPTY value can never be a real dependency, and it is the exact
        // output the old first-quote-to-next-quote extractor produced for a
        // `"""…"""` escape. It normalises to the manifest's OWN directory and
        // therefore reads as "inside the repository" — a green that means the
        // reader broke, not that the tree is clean. Refuse it outright, so any
        // future extractor confusion of this shape fails loudly instead.
        if value.trim().is_empty() {
            problems.push((
                line_no,
                format!(
                    "declares an EMPTY `path` value, which resolves to the manifest's own \
                     directory and would read as 'inside the repository': {line}"
                ),
            ));
            continue;
        }

        let resolved = normalize(dir, &value);
        // Inside the repo (or the repo itself) is fine — `../anvil-core` is
        // how this workspace is wired. Anything else a stranger cannot
        // satisfy.
        if resolved != repo_root && !resolved.starts_with(repo_root) {
            problems.push((
                line_no,
                format!("escapes the repository (-> {value}): {line}"),
            ));
            continue;
        }

        // Lexically inside — but a symlink can still walk out. Second rule.
        if let Some(real) = escapes_via_symlink(&resolved, repo_root) {
            problems.push((
                line_no,
                format!(
                    "escapes the repository THROUGH A SYMLINK (-> {value} -> {}): {line}",
                    real.display()
                ),
            ));
        }
    }

    problems.extend(patch_block_lines(&text));
    problems
}

/// The second, additive escape rule: a path that is lexically inside the
/// repository but leaves it once symlinks are resolved.
///
/// `normalize` is deliberately lexical (see its comment) and therefore
/// symlink-blind. This repository already ships an out-of-repo symlink
/// (`forge` -> `../anvil-hearth`), so `vendor/broker -> ../../../foundry/…`
/// is not a hypothetical shape: it reads clean lexically and 101s cargo on a
/// bare clone.
///
/// `canonicalize` is NOT used, and the reason is worth recording because the
/// first version of this function did use it and MEASURED GREEN on the very
/// mutation it was written for. Canonicalize resolves a symlink only if the
/// symlink's TARGET exists — and on a bare clone `../../../foundry/…` is
/// precisely what does not exist, so the link dangles and canonicalize returns
/// `Err`. The one machine where the escape is invisible to canonicalize is the
/// one machine where it matters.
///
/// So the link is read and resolved LEXICALLY instead, by the same `normalize`
/// the primary rule uses: walk each component below the repository root, and
/// whenever one is a symlink, follow it on paper. A dangling link out of the
/// repository is caught exactly like a live one.
///
/// The chain is bounded at 16 hops so a symlink loop cannot hang the guard.
fn escapes_via_symlink(resolved: &Path, repo_root: &Path) -> Option<PathBuf> {
    let below_root = resolved.strip_prefix(repo_root).ok()?;
    let mut cur = repo_root.to_path_buf();
    for component in below_root.components() {
        cur = cur.join(component.as_os_str());
        for _ in 0..16 {
            let Ok(meta) = std::fs::symlink_metadata(&cur) else {
                break;
            };
            if !meta.file_type().is_symlink() {
                break;
            }
            let Ok(target) = std::fs::read_link(&cur) else {
                break;
            };
            let parent = cur.parent().unwrap_or(repo_root).to_path_buf();
            cur = normalize(&parent, &target.to_string_lossy());
            if !cur.starts_with(repo_root) {
                return Some(cur);
            }
        }
    }
    None
}

/// `[patch.crates-io]` declarations in a manifest's text.
///
/// This is how the private siblings were wired into the root manifest, and a
/// bare clone has nothing to resolve it against. There is no legitimate use of
/// it in this repository, so its mere PRESENCE is the finding — independently
/// of where its entries point, because "it currently points somewhere harmless"
/// is a property of today's contents, not of the mechanism.
/// Matched on a NORMALISED key path rather than on the literal
/// `[patch.crates-io]`, because TOML lets a key be quoted and cargo does not
/// care which spelling you use. `[patch."crates-io"]` walked straight past the
/// literal check while cargo accepted it — a green guard over a live escape
/// hatch. So does `[patch.'crates-io']`, and so does a dotted-key assignment
/// (`patch.crates-io.serde = { … }`) with no section header at all.
///
/// The rule is now "any `patch` table, however spelled", not "this one
/// registry". `[patch."https://github.com/…"]` is the same mechanism pointed at
/// a git source, and there is no legitimate `[patch]` of any kind here.
fn patch_block_lines(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, raw)| {
            let line = raw.trim();
            if line.starts_with('#') {
                return None;
            }
            let key_path = if let Some(inner) = line
                .strip_prefix('[')
                .and_then(|rest| rest.split(']').next())
            {
                // A section header: `[patch.crates-io]`, `[patch."crates-io"]`.
                inner.trim().to_string()
            } else if let Some((lhs, _)) = line.split_once('=') {
                // A dotted-key assignment at document root.
                lhs.trim().to_string()
            } else {
                return None;
            };

            let first = key_path.split('.').next().unwrap_or_default().trim();
            let first = first
                .trim_matches('"')
                .trim_matches('\'')
                .trim();
            if first != "patch" {
                return None;
            }
            Some((
                i + 1,
                format!(
                    "declares a [patch] table (`{key_path}`), which a bare clone cannot resolve"
                ),
            ))
        })
        .collect()
}

#[test]
fn the_default_workspace_never_reaches_outside_the_repository() {
    let root = repo_root();
    let mut problems = Vec::new();
    let mut manifests_checked = 0;
    let mut path_values_seen = 0;

    // The ROOT manifest is scanned too, and first. It is not a workspace
    // member, which is exactly why the `[patch.crates-io]` escape lived there
    // undetected: a member-only loop cannot see it.
    //
    // `brine-tests/` and `kit-build/` are NOT scanned FOR ESCAPING PATH DEPS.
    // They are excluded from the default workspace precisely so their
    // private-sibling path deps are unreachable from a default `cargo build`;
    // flagging `../../../foundry/...` in `kit-build/anvil-kit-engine` would be
    // flagging the fix.
    //
    // They ARE scanned for `[patch.crates-io]`, by
    // `no_workspace_anywhere_declares_a_patch_block` below. The two rules are
    // not in tension: a declared path dep in an excluded workspace is the
    // SANCTIONED way to depend on a private sibling, whereas a patch block is a
    // global redirect of crates.io resolution and has no sanctioned use here.
    let scanned: Vec<(String, PathBuf)> =
        std::iter::once(("Cargo.toml".to_string(), root.join("Cargo.toml")))
            .chain(DEFAULT_WORKSPACE_MEMBERS.iter().map(|member| {
                (
                    format!("{member}/Cargo.toml"),
                    root.join(member).join("Cargo.toml"),
                )
            }))
            .collect();

    for (label, manifest) in &scanned {
        assert!(
            manifest.is_file(),
            "`{label}` does not exist at {} — this guard would have silently \
             checked nothing",
            manifest.display()
        );
        manifests_checked += 1;
        path_values_seen +=
            path_values(&std::fs::read_to_string(manifest).expect("manifest readable")).len();

        for (line_no, finding) in escaping_declarations(manifest, &root) {
            if ALLOWED_ESCAPES
                .iter()
                .any(|allowed| finding.contains(allowed))
            {
                continue;
            }
            problems.push(format!("{label}:{line_no}: {finding}"));
        }
    }

    // A guard over a hand-written list silently misses everything added after
    // it is written, so assert the surface it actually covered: the root plus
    // every member.
    assert_eq!(
        manifests_checked,
        DEFAULT_WORKSPACE_MEMBERS.len() + 1,
        "checked {manifests_checked} manifests but the default workspace has {} \
         members plus the root manifest",
        DEFAULT_WORKSPACE_MEMBERS.len()
    );

    // A PARSER THAT FINDS NOTHING FLAGS NOTHING, and reports a confident pass
    // while doing it. These manifests are wired together with intra-repo path
    // deps (`../anvil-core` and friends), so the scan MUST be seeing several —
    // if it ever sees zero, the extractor has broken and every escape is
    // invisible.
    assert!(
        path_values_seen >= DEFAULT_WORKSPACE_MEMBERS.len(),
        "the scan found only {path_values_seen} `path = \"…\"` values across \
         {manifests_checked} manifests. These crates depend on each other by \
         path, so that is a broken extractor, not a clean tree — the guard \
         would pass vacuously."
    );

    assert!(
        problems.is_empty(),
        "the default workspace reaches outside this repository, so it will not \
         build from a bare clone:\n  - {}\n\nIf the dependency is genuinely \
         optional and private, it still must not be reachable from a default \
         `cargo build` — move the crate that needs it into its own excluded \
         workspace, as `brine-tests/` and `kit-build/` do.",
        problems.join("\n  - ")
    );
}

/// NO workspace root in this repository may declare `[patch.crates-io]` —
/// including the ones that legitimately reach a private sibling.
///
/// The escaping-path rule above deliberately exempts `brine-tests/` and
/// `kit-build/`: their whole purpose is to hold the path deps a bare clone must
/// never load, and they are unreachable from a default `cargo build`. A patch
/// block is a different mechanism with a different blast radius — it globally
/// redirects crates.io resolution for its workspace, it is what the private
/// siblings were wired through before this track, and
/// `scripts/anvil-public-staging-EXPORT.sh` refuses to export ANY manifest that
/// still carries one. So the exemption is scoped to path deps only, and this
/// rule covers every root.
///
/// # Why two tiers of root, and why the count is asserted
///
/// This test used to assert that EVERY root in its list exists, on the correct
/// reasoning that a missing manifest means the guard silently checked nothing.
/// The reasoning was right and the implementation shipped a broken artifact:
/// `brine-tests/` and `kit-build/` are not in the public mirror's INCLUDE list
/// (`scripts/anvil-public-staging-EXPORT.sh`), so the FIRST thing a stranger
/// ran after cloning the mirror was a panic naming a directory that is not in
/// the repository they cloned.
///
/// The fix is not "skip if missing" — that is the silent no-op the original
/// assertion existed to prevent, and it would let the whole test degrade to
/// zero roots without a word. It is two tiers plus a floor:
///
/// * REQUIRED roots are present in every tree this test can compile in. The
///   root manifest and the four default members are, by construction: cargo
///   built this test out of that workspace.
/// * OPTIONAL roots are the excluded workspaces, which the mirror omits. Absent
///   ⇒ skipped, and the skip is REPORTED, not swallowed.
/// * The number of roots actually checked is asserted non-zero and printed.
///   A no-op version of this test cannot pass in either repository.
#[test]
fn no_workspace_anywhere_declares_a_patch_block() {
    let root = repo_root();

    // (label, path, required)
    let mut manifests: Vec<(String, PathBuf, bool)> =
        vec![("Cargo.toml".to_string(), root.join("Cargo.toml"), true)];
    for member in DEFAULT_WORKSPACE_MEMBERS {
        manifests.push((
            format!("{member}/Cargo.toml"),
            root.join(member).join("Cargo.toml"),
            true,
        ));
    }
    // The excluded workspace ROOTS — the only place a `[patch]` table is
    // meaningful for them. Absent from the public mirror.
    for secondary in ["brine-tests", "kit-build"] {
        manifests.push((
            format!("{secondary}/Cargo.toml"),
            root.join(secondary).join("Cargo.toml"),
            false,
        ));
    }

    let mut problems = Vec::new();
    let mut checked: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for (label, manifest, required) in &manifests {
        if !manifest.is_file() {
            assert!(
                !required,
                "`{label}` does not exist at {} — this guard would have silently \
                 checked nothing. This root is REQUIRED: it is present in every \
                 tree this test can compile in, including the public export, so \
                 its absence is a broken checkout rather than a trimmed one.",
                manifest.display()
            );
            skipped.push(label);
            continue;
        }
        checked.push(label);
        let text = std::fs::read_to_string(manifest).expect("manifest readable");
        for (line_no, finding) in patch_block_lines(&text) {
            problems.push(format!("{label}:{line_no}: {finding}"));
        }
    }

    // THE FLOOR. Printed on every run, pass or fail, so "it passed" is always
    // accompanied by "over what". A guard whose coverage is invisible is a
    // guard that can reach zero coverage without anyone noticing.
    println!(
        "patch-block guard: checked {} root(s) {checked:?}; skipped {} absent optional root(s) {skipped:?}",
        checked.len(),
        skipped.len()
    );
    assert!(
        !checked.is_empty(),
        "checked ZERO workspace roots, so this test proved nothing. It found none \
         of {:?} under {}. Passing here would be a silent no-op, which is the one \
         outcome this guard must never produce.",
        manifests.iter().map(|(l, _, _)| l).collect::<Vec<_>>(),
        root.display()
    );

    assert!(
        problems.is_empty(),
        "a manifest declares a [patch] table:\n  - {}\n\nThat table is how the \
         private siblings were wired in before, and a bare clone has nothing to \
         resolve it against. Depend on a private sibling from an EXCLUDED \
         workspace by path instead.",
        problems.join("\n  - ")
    );
}

/// The resolution rule is the load-bearing part of this guard, and it is the
/// part that was wrong before (a substring match on `"../../` that a single
/// `../` walked straight past). Pin it directly, so a future simplification of
/// `normalize` cannot quietly restore the hole while the guard above stays
/// green on a clean tree.
#[test]
fn the_escape_rule_distinguishes_intra_repo_paths_from_escapes() {
    let root = PathBuf::from("/repo");
    let member = root.join("anvil-engine");

    // Legitimate intra-repo wiring — must NOT be treated as an escape.
    for inside in ["../anvil-core", "../anvil-core-hearth", "./sub", "sub/dir"] {
        let resolved = normalize(&member, inside);
        assert!(
            resolved.starts_with(&root),
            "`{inside}` resolved to {} and would be wrongly flagged as an escape",
            resolved.display()
        );
    }

    // Escapes — including the SINGLE-`../` shape the old predicate could not
    // match, which is the one the root `[patch.crates-io]` block used.
    for outside in [
        "../../foundry/crates/foundry-kit-broker-client",
        "../../../elsewhere/thing",
        "/absolute/elsewhere",
    ] {
        let resolved = normalize(&member, outside);
        assert!(
            !resolved.starts_with(&root),
            "`{outside}` resolved to {} and would be MISSED",
            resolved.display()
        );
    }
    // From the ROOT manifest's own directory, a single `../` already escapes.
    let from_root = normalize(&root, "../foundry/crates/foundry-engine-addressing");
    assert!(
        !from_root.starts_with(&root),
        "a single-`../` path from the root manifest resolved to {} and would be MISSED",
        from_root.display()
    );
}

/// The member list above is hand-written; if the real workspace grows a member,
/// this guard would quietly stop covering the repository.
#[test]
fn the_member_list_matches_the_actual_workspace() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");

    let members_block = text
        .split_once("members = [")
        .expect("root Cargo.toml has no `members = [`")
        .1
        .split_once(']')
        .expect("unterminated members list")
        .0;

    let actual: Vec<String> = members_block
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('"'))
        .map(|l| l.trim_matches(|c| c == '"' || c == ',').to_string())
        .collect();

    let expected: Vec<String> = DEFAULT_WORKSPACE_MEMBERS
        .iter()
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        actual, expected,
        "the default workspace's members changed; update DEFAULT_WORKSPACE_MEMBERS \
         in this test so the guard keeps covering every member"
    );
}

/// The four spellings that were MEASURED to walk past this guard on
/// `efce85ed`, pinned so a future simplification cannot reopen them.
///
/// Each of these was confirmed GREEN against the compiled guard binary while
/// cargo either exited 101 on a bare clone or silently accepted the escape.
/// Pinning them at the extractor rather than through a whole manifest keeps the
/// check honest: `cargo test`'s exit code cannot distinguish "the guard fired"
/// from "cargo failed to load the mutated manifest and never ran the guard",
/// because both are 101. These assertions observe the guard's own output.
#[test]
fn the_spellings_that_used_to_bypass_this_guard_are_read_correctly() {
    let root = PathBuf::from("/repo");
    let member = root.join("anvil-engine");
    let escape = "../../foundry/crates/foundry-kit-broker-client";

    // Every form TOML allows for the SAME declaration must yield the SAME value.
    let same = [
        format!("x = {{ path = \"{escape}\" }}"),
        format!("x = {{ path='{escape}' }}"),
        format!("x = {{ path = \"\"\"{escape}\"\"\" }}"),
        format!("x = {{ path = '''{escape}''' }}"),
    ];
    for text in &same {
        let found = path_values(text);
        assert_eq!(
            found.len(),
            1,
            "the extractor found {} declarations in `{text}`, not 1",
            found.len()
        );
        assert_eq!(
            found[0].1,
            Extracted::Value(escape.to_string()),
            "`{text}` was read as {:?}, so the escape is invisible",
            found[0].1
        );
        let resolved = normalize(&member, escape);
        assert!(
            !resolved.starts_with(&root),
            "`{text}` resolved inside the repo and would be MISSED"
        );
    }

    // A multi-line string whose opening delimiter is followed by a newline —
    // TOML drops that newline, and so must this.
    let across = format!("x = {{ path = \"\"\"\n{escape}\"\"\" }}");
    assert_eq!(
        path_values(&across)[0].1,
        Extracted::Value(escape.to_string())
    );

    // A `\u` escape inside a basic string is a path the old byte-literal
    // reader could not see through. `/` is `/`.
    let escaped = "x = { path = \"..\\u002f..\\u002ffoundry\" }";
    assert_eq!(
        path_values(escaped)[0].1,
        Extracted::Value("../../foundry".to_string())
    );

    // The EMPTY value the old reader produced for `"""` is refused outright,
    // so extractor confusion of that shape can never read as clean again.
    let empty = "x = { path = \"\" }";
    assert_eq!(path_values(empty)[0].1, Extracted::Value(String::new()));

    // An unterminated value is a FINDING, never a skip.
    let unterminated = "x = { path = \"\"\"../../foundry";
    assert!(
        matches!(path_values(unterminated)[0].1, Extracted::Unreadable(_)),
        "an unreadable `path =` must be reported, not skipped: {:?}",
        path_values(unterminated)[0].1
    );

    // ...and a key that merely ENDS in `path` still must not be matched.
    assert!(
        path_values("hearth_path = \"../../elsewhere\"").is_empty(),
        "`hearth_path` is a different key; matching it produces phantom findings"
    );
}

/// `[patch]` is matched on a normalised key, so the quoted spelling cannot
/// walk past. `[patch."crates-io"]` was MEASURED green on `efce85ed` while
/// cargo accepted it (exit 0) — the worst of the four bypasses, because
/// nothing downstream failed either.
#[test]
fn every_spelling_of_a_patch_table_is_caught() {
    for spelling in [
        "[patch.crates-io]",
        "[patch.\"crates-io\"]",
        "[patch.'crates-io']",
        "  [patch.\"crates-io\"]  ",
        "[patch.\"https://github.com/example-org/private-sibling\"]",
        "patch.crates-io.serde = { path = \"../../foundry/serde\" }",
    ] {
        let found = patch_block_lines(spelling);
        assert_eq!(
            found.len(),
            1,
            "`{spelling}` was not caught as a [patch] table — that is the \
             quoted-key bypass reopening"
        );
    }

    // Negative controls: these must NOT be findings, or the guard becomes noise
    // and gets deleted.
    for innocent in [
        "# [patch.crates-io]",
        "[dependencies]",
        "[package]",
        "[[bin]]",
        "dispatch = { path = \"../anvil-core\" }",
        "[workspace.dependencies]",
    ] {
        assert!(
            patch_block_lines(innocent).is_empty(),
            "`{innocent}` was wrongly flagged as a [patch] table"
        );
    }
}

/// The symlink rule, exercised for real rather than reasoned about.
///
/// Lexical normalisation is blind to this by construction, so if this test
/// stops failing when `escapes_via_symlink` is removed, the rule is decoration.
#[test]
fn a_symlink_out_of_the_repository_is_caught() {
    #[cfg(unix)]
    {
        let tmp = std::env::temp_dir().join(format!(
            "anvil-symlink-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = tmp.join("iso").join("anvil");
        let outside = tmp.join("foundry").join("crates").join("broker");
        std::fs::create_dir_all(repo.join("vendor")).expect("mkdir repo");
        std::fs::create_dir_all(&outside).expect("mkdir outside");
        let link = repo.join("vendor").join("broker");
        std::os::unix::fs::symlink("../../../foundry/crates/broker", &link).expect("symlink");

        let resolved = normalize(&repo, "vendor/broker");
        assert!(
            resolved.starts_with(&repo),
            "the lexical rule is supposed to be blind here — if it is not, this \
             test is no longer testing the symlink rule"
        );
        let caught = escapes_via_symlink(&resolved, &repo);
        assert!(
            caught.is_some(),
            "a symlink at {} pointing out of the repository was NOT caught",
            link.display()
        );

        // THE BARE-CLONE SHAPE, and the one a canonicalize-based rule silently
        // misses: the same symlink with its target ABSENT. That is the state of
        // every machine this guard exists to protect.
        std::fs::remove_dir_all(tmp.join("foundry")).expect("remove the link target");
        assert!(
            std::fs::canonicalize(&link).is_err(),
            "the link must now DANGLE, or this is not testing the bare-clone case"
        );
        assert!(
            escapes_via_symlink(&normalize(&repo, "vendor/broker"), &repo).is_some(),
            "a DANGLING symlink out of the repository was not caught — this is \
             the exact case canonicalize cannot see, and the case that matters"
        );

        // Negative control: an ordinary intra-repo directory is not flagged.
        std::fs::create_dir_all(repo.join("anvil-core")).expect("mkdir member");
        assert!(
            escapes_via_symlink(&normalize(&repo, "anvil-core"), &repo).is_none(),
            "an ordinary intra-repo path was wrongly flagged as a symlink escape"
        );

        // Negative control: a path that does NOT exist is left to the lexical
        // rule, which is the whole reason canonicalize cannot be primary.
        assert!(
            escapes_via_symlink(&normalize(&repo, "not-here/at-all"), &repo).is_none(),
            "a non-existent path must be left to the lexical rule"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
