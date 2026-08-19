//! Step definitions for `anvil-core/features/hearth_write_refusal.feature`.
//!
//! # Why the WRITE half needed its own table
//!
//! **C-d.1 round 9, H-1 of `review-cd1-fixround-r8.md`.** Round 8 fixed eight
//! read-then-write sites and measured two of them by hand, writing the
//! measurements into doc comments. The round-8 reviewer then reverted the
//! highest-blast-radius fix verbatim and ran the whole workspace:
//!
//! ```text
//! MUT-INSTALLER — `register_mcp`'s `read_to_string(&path).unwrap_or_default()`
//!                 restored verbatim
//!   anvil-core     232 features / 1765 scenarios / 1765 PASSED / 0 failed
//!   anvil-engine   142 features /  533 scenarios /  529 passed / 4 failed (baseline)
//! ```
//!
//! **Zero red.** A user's `kiln` and `lore` MCP registrations and every
//! unrelated key in their `.claude.json`, destroyed on their own machine with
//! `written` reported, could be reintroduced by anyone and the suite would not
//! notice. The read half got 122 new cells; the write half got none — and this
//! track's entire subject is assertions that cannot fail.
//!
//! ## What these two tables hold constant, and what makes them falsifiable
//!
//! The same shape as `hearth_port_reachability`: one fixture per row, only the
//! MODE of one named node varies, and **every table carries live controls that
//! prove the write really happens.**
//!
//! * `install_one` at `0644` must report `written`, must leave `kiln`, `lore`
//!   and `theme` in place, AND must have added anvil's own entry. Without that
//!   last clause a `register_mcp` that never wrote anything would satisfy every
//!   refusing row.
//! * `append_transition_event` at `0755`, `0500` and `0300` must FILE the event
//!   into the artifact's real `transitions/`. `0300` is the discriminating
//!   control: `tracks/` is traversable and not listable there, so `stat`
//!   succeeds and a blanket "any non-0755 mode refuses" would fail it.
//!
//! The refusing rows assert the REFUSAL TOKEN, not merely an error. At `0000`
//! the pre-fix code already failed — but it failed at the WRITE, having first
//! decided on a phantom location. Asserting `mcp_config_uninspectable` /
//! `artifact_location_uninspectable` is what distinguishes "refused because it
//! could not read its input" from "happened to fail later anyway".
//!
//! ## Scope: two sites, declared
//!
//! Round 8 converted eight sites. These are the two whose blast radius is OTHER
//! PEOPLE'S DATA — a foreign tool's config on the user's own machine, and an
//! artifact's governance history. The other six are declared `EIO`-class-only in
//! `implementation-c.md` §41.9(4): their swallowing arm is reachable only by an
//! error class no local POSIX fixture can produce, because the mode that defeats
//! the read also defeats the temp-sibling write beneath it.

use anvil_test_support::ScratchDir;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anvil_core::domain::hooks::installer::{self, HarnessOutcome};
use anvil_core::domain::hooks::{Harness, InstallSpec};
use anvil_core_hearth::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::transition_event_write_port::{
    TransitionEventWritePort, TransitionRecord, TRANSITIONS_DIR,
};

const DIR_KEY: &str = "hwr_dir";
const HANDLE_KEY: &str = "hwr_handle";
const SEEDED_KEY: &str = "hwr_seeded";
const OUTCOME_KEY: &str = "hwr_outcome";
const DETAIL_KEY: &str = "hwr_detail";

const CARRIED: &[(&str, &str)] = &[
    (DIR_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (SEEDED_KEY, "string"),
];
const CARRIED_OUT: &[(&str, &str)] = &[
    (DIR_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (SEEDED_KEY, "string"),
    (OUTCOME_KEY, "string"),
    (DETAIL_KEY, "string"),
];

/// The artifact every transition-event row files against, by BARE ID.
///
/// The bare id is the address that reaches the per-kind fallback loop — the
/// route that manufactured a phantom location. A relative path resolves
/// literally on the first probe and never reaches it.
const TRACK_ID: &str = "20260101T0000_write_track";

const MCP_CONFIG: &str = ".claude.json";

fn set_mode(p: &Path, mode: u32) -> Result<(), String> {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {} to {mode:04o}: {e}", p.display()))
}

// ── site 1: installer::install_one → register_mcp ───────────────────────────

/// A `.claude.json` carrying TWO foreign MCP servers and one unrelated
/// top-level key — the exact fixture §41.4 measured the destruction on.
///
/// `otherUserSettings` matters as much as the servers: the swallow read the
/// whole document as `{}`, so what `write_config` wrote back was a document
/// holding anvil's entry ALONE. Everything in the file went, not only the MCP
/// section.
const FOREIGN_MCP_CONFIG: &str = r#"{
  "mcpServers": {
    "kiln": { "command": "/opt/kiln/mcp", "type": "stdio" },
    "lore": { "command": "/opt/lore/mcp", "type": "stdio" }
  },
  "otherUserSettings": { "theme": "dark" }
}
"#;

/// The user's MCP config as it stands, or a LOUD failure.
///
/// The three inspectors in this module (`mcp_body`, `real_events`, `phantom`)
/// are the assertions' eyes, and an assertion that cannot see is an assertion
/// that cannot fail. **None of them may swallow.** `unwrap_or_default()` here
/// would report "the file no longer holds kiln" for a file it merely could not
/// read — the exact substitution this whole track exists to end, on the test
/// side of the seam, where nobody would look for it.
fn mcp_body(dir: &Path) -> Result<String, String> {
    let p = dir.join(MCP_CONFIG);
    match std::fs::read_to_string(&p) {
        Ok(body) => Ok(body),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!(
            "the assertion could not READ {} ({e}), so it cannot say what survived. This is the \
             instrument's own version of the defect and it fails loud rather than reporting the \
             file as empty.",
            p.display()
        )),
    }
}

// ── site 2: fs_transition_event_adapter::append_transition_event ────────────

fn seed_track(hearth: &Path) -> Result<(), String> {
    let dir = hearth.join("tracks").join(TRACK_ID);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    std::fs::write(
        dir.join("status.yaml"),
        "version: 1\nkind: track\nstate: implementing\nactors: {}\n",
    )
    .map_err(|e| format!("write status.yaml: {e}"))?;
    Ok(())
}

/// Event files that reached the artifact's REAL transitions directory, or a
/// LOUD failure. An ABSENT directory genuinely holds zero events; anything else
/// means the assertion could not look, and "could not look" is not "zero".
fn real_events(hearth: &Path) -> Result<usize, String> {
    let dir = hearth.join("tracks").join(TRACK_ID).join(TRANSITIONS_DIR);
    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            let mut n = 0usize;
            for e in entries {
                let e = e.map_err(|e| format!("entry under {}: {e}", dir.display()))?;
                if e.file_name().to_string_lossy().ends_with(".yaml") {
                    n += 1;
                }
            }
            Ok(n)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(format!(
            "the assertion could not LIST {} ({e}), so it cannot say how many events landed. \
             Counting that as zero would be this track's defect, in the check.",
            dir.display()
        )),
    }
}

/// Whether a PHANTOM artifact directory exists at the hearth ROOT.
///
/// This is the manufactured location: `hearth.join(<bare id>)`, invented by the
/// caller's `unwrap_or_else` when the lookup could not tell "not here" from "I
/// could not look", and made real by the `create_dir_all` on the next line.
///
/// **`.exists()` is deliberately NOT used.** It answers `false` for a node it
/// could not stat, which would make "no phantom manufactured" pass over a
/// hearth this check simply could not see into — a fail-OPEN assertion about a
/// fail-open defect. `symlink_metadata` separates the two facts.
fn phantom(hearth: &Path) -> Result<bool, String> {
    let p = hearth.join(TRACK_ID);
    match std::fs::symlink_metadata(&p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!(
            "the assertion could not stat {} ({e}), so it cannot say whether a phantom artifact \
             was manufactured. `.exists()` would have answered `false` here and passed.",
            p.display()
        )),
    }
}

fn record() -> TransitionRecord {
    TransitionRecord {
        to: "reviewing".to_string(),
        at: "2026-01-01T00:03:00Z".to_string(),
        actor: "Author-000001".to_string(),
        role: "review".to_string(),
        approver: None,
        note: None,
        satisfaction: None,
        event_type: None,
    }
}

fn carry(ctx: &mut Context, out: &mut Context) -> Result<(), String> {
    let dir = ctx.get::<String>(DIR_KEY).ok_or("no dir")?.clone();
    let seeded = ctx.get::<String>(SEEDED_KEY).ok_or("no seeded")?.clone();
    out.set(DIR_KEY, dir);
    out.set(SEEDED_KEY, seeded);
    if let Some(h) = ctx.take::<Arc<ScratchDir>>(HANDLE_KEY) {
        out.set(HANDLE_KEY, h);
    }
    Ok(())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a Claude Code config dir whose MCP config holds two foreign servers and an unrelated setting, at mode {string}",
            &[],
            CARRIED,
            |_ctx, params| {
                let mode_s = params.get_string(0).ok_or("expected mode")?;
                let mode = u32::from_str_radix(mode_s.trim(), 8)
                    .map_err(|e| format!("mode {mode_s:?} is not octal: {e}"))?;
                let t = ScratchDir::new()?;
                let dir = t.path().to_path_buf();
                std::fs::write(dir.join(MCP_CONFIG), FOREIGN_MCP_CONFIG)
                    .map_err(|e| format!("write {MCP_CONFIG}: {e}"))?;
                // The hook config is a DIFFERENT file (settings.json). It stays
                // readable on every row, so the only thing this table varies is
                // the MCP config's own mode — otherwise a row could refuse for
                // the wrong reason and still look like the fix.
                std::fs::write(dir.join(Harness::ClaudeCode.config_filename()), "{}\n")
                    .map_err(|e| format!("write settings.json: {e}"))?;
                // Recorded BEFORE the chmod, from the file, so a row can prove
                // its own fixture carried the foreign data even where the port
                // is about to refuse to look at it.
                let body = mcp_body(&dir)?;
                let seeded = format!(
                    "{}+{}+{}",
                    body.contains("\"kiln\""),
                    body.contains("\"lore\""),
                    body.contains("\"theme\"")
                );
                set_mode(&dir.join(MCP_CONFIG), mode)?;
                let mut out = Context::new();
                out.set(DIR_KEY, dir.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(SEEDED_KEY, seeded);
                Ok(out)
            },
        ),
        step_def(
            "the hook installer runs for Claude Code with MCP registration",
            CARRIED,
            CARRIED_OUT,
            |mut ctx, _params| {
                let dir = ctx.get::<String>(DIR_KEY).ok_or("no dir")?.clone();
                let dir = PathBuf::from(dir);
                let spec = InstallSpec {
                    mcp_command: "/kit/mcp/anvil-mcp".to_string(),
                    ..InstallSpec::default()
                };
                let report = installer::install_one(Harness::ClaudeCode, &dir, &spec, true);
                let outcome = report.outcome.as_str().to_string();
                let detail = match &report.outcome {
                    HarnessOutcome::Failed(e) => e.clone(),
                    _ => String::new(),
                };
                // Restore read access so the assertions can inspect what the
                // installer left behind. The measurement is already taken.
                let _ = set_mode(&dir.join(MCP_CONFIG), 0o644);
                let mut out = Context::new();
                carry(&mut ctx, &mut out)?;
                out.set(OUTCOME_KEY, outcome);
                out.set(DETAIL_KEY, detail);
                Ok(out)
            },
        ),
        check_def(
            "the install reports {string} and the MCP config still holds {string}",
            &[
                (DIR_KEY, "string"),
                (SEEDED_KEY, "string"),
                (OUTCOME_KEY, "string"),
                (DETAIL_KEY, "string"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected outcome")?;
                let survivors = params.get_string(1).ok_or("expected survivor list")?;
                let dir = PathBuf::from(ctx.get::<String>(DIR_KEY).ok_or("no dir")?);
                let seeded = ctx.get::<String>(SEEDED_KEY).ok_or("no seeded")?;
                let outcome = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let detail = ctx.get::<String>(DETAIL_KEY).ok_or("no detail")?;

                if seeded != "true+true+true" {
                    return Err(format!(
                        "the FIXTURE did not seed the foreign config ({seeded}). Every row of \
                         this table asserts over a `.claude.json` that really holds another \
                         tool's registrations; without them a 'still holds' assertion is true \
                         over a file that never held anything."
                    ));
                }
                // EVERY clause is reported, not the first that fails. The
                // outcome and the surviving data are two independent facts and
                // the interesting mutant breaks BOTH — a short-circuit would
                // show a reviewer the reported outcome and hide the destruction
                // underneath it, which is the more consequential half.
                let mut faults: Vec<String> = Vec::new();
                if outcome != expected {
                    faults.push(format!(
                        "install_one reported {outcome:?}, not {expected:?}. {detail}\n     \
                         `written` over an unreadable MCP config is the finding: the read was \
                         swallowed to an empty document, so the config written back held ANVIL'S \
                         ENTRY ALONE — and the call reported success while doing it."
                    ));
                }
                let body = mcp_body(&dir)?;
                for key in survivors.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                    if !body.contains(&format!("\"{key}\"")) {
                        faults.push(format!(
                            "{key:?} is GONE from the user's {MCP_CONFIG}. It belongs to another \
                             tool (or is an unrelated user setting) and anvil's installer \
                             destroyed it. The file now reads: {body:?}"
                        ));
                    }
                }
                if faults.is_empty() {
                    return Ok(());
                }
                Err(format!("  - {}", faults.join("\n  - ")))
            },
        ),
        check_def(
            "the install refusal names {string}",
            &[(OUTCOME_KEY, "string"), (DETAIL_KEY, "string")],
            |ctx, params| {
                let token = params.get_string(0).ok_or("expected token")?;
                let outcome = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let detail = ctx.get::<String>(DETAIL_KEY).ok_or("no detail")?;
                if outcome != "failed" {
                    return Err(format!(
                        "install_one reported {outcome:?}; a row that names a refusal token \
                         asserts the install REFUSED."
                    ));
                }
                if !detail.contains(token) {
                    return Err(format!(
                        "the refusal reads {detail:?} and does not name {token:?}. The token is \
                         the discriminator: at mode 0000 the pre-fix code ALSO failed — but it \
                         failed at the WRITE, having already decided to write a config it had \
                         read as empty. 'Refused because it could not read its input' and \
                         'happened to fail later anyway' are different facts."
                    ));
                }
                Ok(())
            },
        ),
        // ── site 2 ──────────────────────────────────────────────────────────
        step_def(
            "a hearth holding a track artifact with the per-kind directory at mode {string}",
            &[],
            CARRIED,
            |_ctx, params| {
                let mode_s = params.get_string(0).ok_or("expected mode")?;
                let mode = u32::from_str_radix(mode_s.trim(), 8)
                    .map_err(|e| format!("mode {mode_s:?} is not octal: {e}"))?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed_track(&hearth)?;
                let seeded = format!(
                    "{}+{}",
                    hearth
                        .join("tracks")
                        .join(TRACK_ID)
                        .join("status.yaml")
                        .is_file(),
                    !phantom(&hearth)?
                );
                set_mode(&hearth.join("tracks"), mode)?;
                let mut out = Context::new();
                out.set(DIR_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(SEEDED_KEY, seeded);
                Ok(out)
            },
        ),
        step_def(
            "a governance transition is filed for the artifact by bare id",
            CARRIED,
            CARRIED_OUT,
            |mut ctx, _params| {
                let hearth = ctx.get::<String>(DIR_KEY).ok_or("no hearth")?.clone();
                let hearth = PathBuf::from(hearth);
                let adapter = FileSystemTransitionEventAdapter::new(hearth.clone());
                let (outcome, detail) = match adapter.append_transition_event(TRACK_ID, &record()) {
                    Ok(()) => ("filed".to_string(), String::new()),
                    Err(e) => ("refused".to_string(), format!("{e}")),
                };
                // Restore search permission so the assertions can count what
                // actually landed. The measurement is already taken.
                let _ = set_mode(&hearth.join("tracks"), 0o755);
                let mut out = Context::new();
                carry(&mut ctx, &mut out)?;
                out.set(OUTCOME_KEY, outcome);
                out.set(DETAIL_KEY, detail);
                Ok(out)
            },
        ),
        check_def(
            "the transition write reports {string} with {string} event(s) in the artifact's own history and no phantom artifact",
            &[
                (DIR_KEY, "string"),
                (SEEDED_KEY, "string"),
                (OUTCOME_KEY, "string"),
                (DETAIL_KEY, "string"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected outcome")?;
                let want_events: usize = params
                    .get_string(1)
                    .ok_or("expected event count")?
                    .trim()
                    .parse()
                    .map_err(|e| format!("event count is not a number: {e}"))?;
                let hearth = PathBuf::from(ctx.get::<String>(DIR_KEY).ok_or("no hearth")?);
                let seeded = ctx.get::<String>(SEEDED_KEY).ok_or("no seeded")?;
                let outcome = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let detail = ctx.get::<String>(DETAIL_KEY).ok_or("no detail")?;

                if seeded != "true+true" {
                    return Err(format!(
                        "the FIXTURE is wrong ({seeded}): it must seed a REAL track artifact and \
                         must not already carry a phantom directory at the hearth root, or \
                         'no phantom' is true for a reason that has nothing to do with the port."
                    ));
                }
                // EVERY clause is reported, not the first that fails. `Ok`, an
                // empty real history and a manufactured directory are three
                // independent facts and the defect produces all three at once;
                // a short-circuit would show a reviewer only the reported
                // outcome and hide the phantom underneath it.
                let mut faults: Vec<String> = Vec::new();
                if outcome != expected {
                    faults.push(format!(
                        "append_transition_event reported {outcome:?}, not {expected:?}. {detail}"
                    ));
                }
                let got = real_events(&hearth)?;
                if got != want_events {
                    faults.push(format!(
                        "the artifact's own transitions/ holds {got} event(s), not \
                         {want_events}. `Ok` with the event NOT in the artifact's history is the \
                         finding: the engine is told the transition was persisted and the \
                         governance record never receives it."
                    ));
                }
                if phantom(&hearth)? {
                    faults.push(format!(
                        "a PHANTOM artifact directory exists at {}. The lookup could not tell \
                         'this artifact is not here' from 'I could not look', so the caller \
                         invented a location and `create_dir_all` made it real — a governance \
                         transition filed into an artifact that does not exist, reported as Ok.",
                        hearth.join(TRACK_ID).display()
                    ));
                }
                if faults.is_empty() {
                    return Ok(());
                }
                Err(format!("  - {}", faults.join("\n  - ")))
            },
        ),
        check_def(
            "the transition refusal names {string}",
            &[(OUTCOME_KEY, "string"), (DETAIL_KEY, "string")],
            |ctx, params| {
                let token = params.get_string(0).ok_or("expected token")?;
                let outcome = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let detail = ctx.get::<String>(DETAIL_KEY).ok_or("no detail")?;
                if outcome != "refused" {
                    return Err(format!(
                        "append_transition_event reported {outcome:?}; a row that names a refusal \
                         token asserts the write REFUSED."
                    ));
                }
                if !detail.contains(token) {
                    return Err(format!(
                        "the refusal reads {detail:?} and does not name {token:?}. \
                         `artifact_not_found` would be the WRONG refusal here — the artifact is \
                         on disk and the adapter merely could not look at the directory above \
                         it, which is this whole track's distinction."
                    ));
                }
                Ok(())
            },
        ),
    ]
}
