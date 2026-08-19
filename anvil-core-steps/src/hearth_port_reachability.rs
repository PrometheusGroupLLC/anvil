//! Step definitions for `anvil-core/features/hearth_port_reachability.feature`.
//!
//! # Why this module is a MATRIX and not another module list
//!
//! **C-d.1 round 7, H-1.** Six rounds each closed a hand-written set of modules
//! and each was then defeated by a site nobody had named. `fs_query_adapter.rs`
//! is the purest case: the `QueryPort` the engine constructs eleven times,
//! carrying all four historical spellings of the swallow, named ZERO times in
//! the entire implementation record — while its sibling `fs_snapshot_adapter.rs`
//! was being fixed one seam over. And inside that sibling, `read_artifact_kind`
//! answered NOT_FOUND for an artifact on disk while `read_artifact_state`, in
//! the same file, at the same mode, for the same artifact, answered `IoError`.
//!
//! A module list cannot see that. The criterion here is behavioural instead:
//!
//! > **every port method × every address form it accepts × every failure mode,
//! > and the closure is the table being green.**
//!
//! The methods are enumerated from the PORT TRAITS (`QueryPort`, `SnapshotPort`)
//! rather than from the adapters, so a method that resolves an artifact through
//! a route nobody wrote down still has a row. The address forms are the two the
//! adapters actually accept — a bare artifact id and a hearth-relative path —
//! because they take different routes through the lookup and round 7 found a
//! cell where they DISAGREE (`read_artifact_kind` by path never touches the
//! filesystem: it maps the directory prefix, so it is correct at every mode,
//! and that cell is declared `resolved` rather than quietly omitted).
//!
//! ## What makes a row falsifiable
//!
//! Every row builds the SAME fixture with the SAME two artifacts on disk. Only
//! the mode of one named directory or file varies. So:
//!
//! * `resolved` rows are CONTROLS, and they assert the method's **full correct
//!   value** (declared per method in [`expected_value`]), not merely `Ok`. A
//!   fixture that seeded nothing reds them. `list_artifacts` resolving to a
//!   SHORT list reds them — which is the H-1 consequence itself.
//! * `uninspectable` rows assert the answer is the I/O class. `NotFound`,
//!   `Ok(empty)` and `Ok(short)` all red them, and those three are precisely
//!   what the defect produced.
//! * **`0300` is a control that is not `0755`.** At `0300` a directory is
//!   traversable and not listable, so `stat` succeeds and `read_dir` fails: the
//!   per-artifact lookups must RESOLVE there and the enumerations must REFUSE.
//!   Without it, "every mode that is not 0755 refuses" would pass the table, and
//!   that is a check that has stopped discriminating.

use anvil_test_support::ScratchDir;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::ports::query_port::QueryPort;
use anvil_core::ports::snapshot_port::SnapshotPort;

const HEARTH_KEY: &str = "hpr_hearth";
const HANDLE_KEY: &str = "hpr_handle";
const SEEDED_KEY: &str = "hpr_seeded";
const OUTCOME_KEY: &str = "hpr_outcome";
const METHOD_KEY: &str = "hpr_method";

const CARRIED: &[(&str, &str)] = &[
    (HEARTH_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (SEEDED_KEY, "string"),
];
const CARRIED_OUT: &[(&str, &str)] = &[
    (HEARTH_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (SEEDED_KEY, "string"),
    (OUTCOME_KEY, "string"),
    (METHOD_KEY, "string"),
];

/// The track every row seeds, by bare id. `TRACK_PATH` is the same artifact by
/// hearth-relative path — the two address forms of ONE artifact, which is what
/// makes the address axis a comparison rather than two unrelated probes.
pub const TRACK_ID: &str = "20260101T0000_alpha_track";
pub const KNOWLEDGE_ID: &str = "20260101T0000_beta_knowledge";

/// The per-file transition event store — the node round 7's fixture never
/// created, found by the syscall-coverage instrument rather than by anyone
/// naming it.
const TRANSITIONS_SUBDIR: &str = "transitions";
const EVENT_ONE: &str = "2026-01-01T00:01:00.000000000Z_Author-000001_aaaa.yaml";
const EVENT_TWO: &str = "2026-01-01T00:02:00.000000000Z_Author-000001_bbbb.yaml";

pub fn track_path() -> String {
    format!("tracks/{TRACK_ID}")
}

fn set_mode(p: &Path, mode: u32) -> Result<(), String> {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {} to {mode:04o}: {e}", p.display()))
}

/// The pseudo-mode the ELOOP outline carries, since a symlink loop is not a
/// mode. Spelled once, here, because the syscall-coverage instrument parses it
/// out of the feature and must agree with the step that applies it.
pub const LOOP_MODE: &str = "symlink loop";

/// Replace `p` with a self-referential symlink: `stat` on it answers `ELOOP`
/// deterministically, with no chmod anywhere in the fixture.
fn make_loop(p: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(p)
        .or_else(|_| std::fs::remove_file(p))
        .map_err(|e| format!("remove {}: {e}", p.display()))?;
    std::os::unix::fs::symlink(p, p).map_err(|e| format!("symlink loop at {}: {e}", p.display()))
}

/// Apply ONE row's degradation to a freshly seeded hearth.
///
/// Factored out of the two `Given` steps so the syscall-coverage instrument can
/// rebuild every row's fixture in its probe child (C-d.1 round 9, M-2) without a
/// second, drifting copy of what "at mode 0600" and "replaced by a symlink loop"
/// mean. A second copy is the fixture bound in a new place.
pub fn degrade(hearth: &Path, token: &str, mode: &str) -> Result<(), String> {
    let Some(p) = target(hearth, token)? else {
        return Ok(());
    };
    if mode == LOOP_MODE {
        return make_loop(&p);
    }
    let m = u32::from_str_radix(mode.trim(), 8)
        .map_err(|e| format!("mode {mode:?} is not octal: {e}"))?;
    set_mode(&p, m)
}

/// A hearth holding two REAL artifacts of two different kinds, a registry, a
/// projection, an op log, a hook file and a context file — enough state for
/// every method in the table to have a correct answer that is not empty.
pub fn seed(hearth: &Path) -> Result<(), String> {
    let mk = |p: &Path| std::fs::create_dir_all(p).map_err(|e| format!("mkdir: {e}"));
    let wr = |p: &Path, body: &str| std::fs::write(p, body).map_err(|e| format!("write: {e}"));

    let track = hearth.join("tracks").join(TRACK_ID);
    mk(&track)?;
    wr(
        &track.join("status.yaml"),
        "version: 1\nkind: track\nstate: implementing\norigin_turn: turn-1\ntransitions:\n  - to: implementing\n    at: 2026-01-01T00:00:00Z\n    actor: Author-000001\n    role: plan\nactors:\n  Author-000001:\n    type: agent\n    configurations: []\nactivity:\n  - kind: begin\n    actor: Author-000001\n    state: implementing\n    at: 2026-01-01T00:00:00Z\n",
    )?;
    wr(&track.join("spec.md"), "# Alpha Track\n\nBody.\n")?;
    wr(
        &track.join("spec.amendments.yaml"),
        "- op_id: op-1\n  accepted_at: \"2026-01-01T00:00:00Z\"\n  seq: 0\n  op:\n    target_id: e1\n    kind: retire\n",
    )?;
    wr(&track.join("carry-forward.md"), "CARRY\n")?;

    // ── C-d.1 round 8: the per-file TRANSITION EVENT STORE ───────────────
    //
    // The node the syscall-coverage instrument found, that no axis could have
    // reached. `read_artifact_state`, `read_transitions`,
    // `read_transitions_strict`, `list_artifacts` and
    // `find_artifact_by_kind_origin_turn` — on BOTH ports — all fold
    // `domain::transition_log::read_event_files(artifact_dir)`, which
    // `read_dir`s this directory. Round 7's fixture wrote the history as the
    // legacy `transitions:` array inside status.yaml and never created the
    // directory, so the reader's `Err(_) => Vec::new()` arm executed in all 249
    // cells, on a `NotFound` where emptying is the CORRECT answer, and passed
    // 249 times. Gutting both event readers left the matrix 249/249 GREEN.
    //
    // The two events ADVANCE the state past the legacy array's, so an event
    // that cannot be read is not merely a shorter history — it is a WRONG
    // ANSWER. `Ok(implementing)` for an artifact that is `reviewing` is the
    // sharpest cell on this track: not a refusal, not an emptying, a confidently
    // wrong state that everything gating on state then gates on.
    mk(&track.join(TRANSITIONS_SUBDIR))?;
    wr(
        &track.join(TRANSITIONS_SUBDIR).join(EVENT_ONE),
        "to: planning\nat: 2026-01-01T00:01:00Z\nactor: Author-000001\nrole: plan\n",
    )?;
    wr(
        &track.join(TRANSITIONS_SUBDIR).join(EVENT_TWO),
        "to: reviewing\nat: 2026-01-01T00:02:00Z\nactor: Author-000001\nrole: review\n",
    )?;

    let knowledge = hearth.join("knowledge").join(KNOWLEDGE_ID);
    mk(&knowledge)?;
    wr(
        &knowledge.join("status.yaml"),
        "version: 1\nkind: knowledge_lifecycle\nstate: ingesting\n",
    )?;
    // The non-legacy artifact gets an event store too: the third and fourth
    // address forms reach the fold through a different lookup, and a node that
    // only one address form creates is the fixture bound one address form down.
    mk(&knowledge.join(TRANSITIONS_SUBDIR))?;
    wr(
        &knowledge.join(TRANSITIONS_SUBDIR).join(EVENT_ONE),
        "to: ingesting\nat: 2026-01-01T00:01:00Z\nactor: Author-000001\nrole: plan\n",
    )?;

    wr(
        &hearth.join("tracks.md"),
        &format!("# Tracks\n\n## implementing\n\n- [Alpha Track](tracks/{TRACK_ID}/)\n"),
    )?;
    mk(&hearth.join("projections"))?;
    wr(
        &hearth.join("projections/execution.md"),
        &format!("---\nincremental_count: 0\n---\n\n# Execution\n\n## Implementing (1)\n\n- [Alpha Track](tracks/{TRACK_ID}/)\n\n## Complete (0)\n"),
    )?;
    mk(&hearth.join("context"))?;
    wr(&hearth.join("context/product.md"), "# Product\n")?;
    mk(&hearth.join("playbooks/pb_one/hooks"))?;
    wr(&hearth.join("playbooks/pb_one/hooks/capture.md"), "HOOK\n")?;
    mk(&hearth.join("sparks"))?;
    wr(&hearth.join("sparks/sparks.md"), "# Sparks\n\n## open\n\n- [s1](sparks/s1/)\n")?;
    wr(&hearth.join("decisions.md"), "# Decisions\n\n## tension\n\n- [d1](decisions/d1/)\n")?;
    // The two projection FILES the rebuilds read before they write. The
    // instrument found these the same way it found `transitions/`: a read of a
    // node the fixture never creates, so the "there is already a projection
    // here" branch was never once exercised by 249 cells over a method whose
    // whole risk is reading a projection as empty and writing the empty result
    // over the real one.
    wr(
        &hearth.join("projections/decisions.md"),
        "---\nincremental_count: 0\n---\n\n# Decisions\n\n## Tension (1)\n\n- [d1](decisions/d1/)\n",
    )?;
    wr(
        &hearth.join("projections/sparks.md"),
        "---\nincremental_count: 0\n---\n\n# Sparks\n\n## Open (1)\n\n- [s1](sparks/s1/)\n",
    )?;
    Ok(())
}

/// Resolve the Gherkin `<unreadable>` token to a path inside the hearth.
///
/// The token is the thing the row makes unreadable, spelled the way an operator
/// would name it. `none` is the all-readable control shape.
pub fn target(hearth: &Path, token: &str) -> Result<Option<std::path::PathBuf>, String> {
    Ok(match token {
        "none" => None,
        "the hearth root" => Some(hearth.to_path_buf()),
        "the per-kind directory" => Some(hearth.join("tracks")),
        "the artifact directory" => Some(hearth.join("tracks").join(TRACK_ID)),
        // The per-ENTRY failure mode. Every directory stays readable, so the
        // enumeration edge SUCCEEDS and the read beneath it is what fails —
        // which is the only fixture that can tell a swallow at the per-entry
        // read from a refusal at the listing above it. Without it, MUT-Q4
        // (restore `status_path.exists()` + the `continue`) leaves the table
        // GREEN, because the edge refuses first and masks the site.
        "the artifact status file" => Some(hearth.join("tracks").join(TRACK_ID).join("status.yaml")),
        // ── C-d.1 round 8: the subjects the SYSCALL-COVERAGE instrument found ──
        //
        // Not one of these was named by a person. `hearth_port_syscall_coverage`
        // recorded every path the ports `stat`/`open`/`opendir` during the green
        // control and failed on the ones no row varies. Four of them are reads
        // of nodes round 7's fixture never created, which is the class of hole
        // no additional COLUMN could ever have reached.
        "the transitions directory" => {
            Some(hearth.join("tracks").join(TRACK_ID).join(TRANSITIONS_SUBDIR))
        }
        // The NEWEST event — the one that carries the current state. Unreadable,
        // the fold resolves to the previous event's `to`, which is a wrong
        // answer rather than a short history.
        "the newest transition event file" => Some(
            hearth
                .join("tracks")
                .join(TRACK_ID)
                .join(TRANSITIONS_SUBDIR)
                .join(EVENT_TWO),
        ),
        "the non-legacy transitions directory" => Some(
            hearth
                .join("knowledge")
                .join(KNOWLEDGE_ID)
                .join(TRANSITIONS_SUBDIR),
        ),
        "the non-legacy transition event file" => Some(
            hearth
                .join("knowledge")
                .join(KNOWLEDGE_ID)
                .join(TRANSITIONS_SUBDIR)
                .join(EVENT_ONE),
        ),
        "the non-legacy artifact directory" => Some(hearth.join("knowledge").join(KNOWLEDGE_ID)),
        "the artifact text file" => Some(hearth.join("tracks").join(TRACK_ID).join("spec.md")),
        "the carry-forward file" => {
            Some(hearth.join("tracks").join(TRACK_ID).join("carry-forward.md"))
        }
        "the op log file" => Some(
            hearth
                .join("tracks")
                .join(TRACK_ID)
                .join("spec.amendments.yaml"),
        ),
        "the playbooks directory" => Some(hearth.join("playbooks")),
        "the playbook directory" => Some(hearth.join("playbooks/pb_one")),
        "the hook file" => Some(hearth.join("playbooks/pb_one/hooks/capture.md")),
        "the context file" => Some(hearth.join("context/product.md")),
        "the projections directory" => Some(hearth.join("projections")),
        "the execution projection" => Some(hearth.join("projections/execution.md")),
        "the decisions projection" => Some(hearth.join("projections/decisions.md")),
        "the sparks projection" => Some(hearth.join("projections/sparks.md")),
        "the sparks directory" => Some(hearth.join("sparks")),
        "the non-legacy kind directory" => Some(hearth.join("knowledge")),
        "the non-legacy status file" => Some(hearth.join("knowledge").join(KNOWLEDGE_ID).join("status.yaml")),
        "the hooks directory" => Some(hearth.join("playbooks/pb_one/hooks")),
        "the context directory" => Some(hearth.join("context")),
        "the registry file" => Some(hearth.join("tracks.md")),
        "the sparks source" => Some(hearth.join("sparks/sparks.md")),
        "the decisions registry" => Some(hearth.join("decisions.md")),
        "the projection file" => Some(hearth.join("projections/execution.md")),
        other => return Err(format!("the scenario names a target this module does not seed: {other:?}")),
    })
}

/// The full correct answer for each method, DECLARED here rather than derived
/// from a run.
///
/// This is the anti-vacuity spine of the control rows: `resolved` compares
/// against these literals, so a control cannot pass over a fixture that seeded
/// nothing, and an enumeration that comes back SHORT fails its own control.
fn expected_value(method: &str, address: &str) -> &'static str {
    if address == "bare id, non-legacy kind" || address == "relative path, non-legacy kind" {
        return match method {
            "read_artifact_kind" => "knowledge_lifecycle",
            "read_artifact_state" => "ingesting",
            "read_artifact_status" => "kind=knowledge_lifecycle;state=ingesting",
            "read_artifact_actor_names" => "",
            "read_activity_entries" => "0 entries",
            "read_transitions" => "1 transitions",
            _ => "<undeclared for the non-legacy address>",
        };
    }
    match method {
        "list_artifacts" => "20260101T0000_alpha_track=track,20260101T0000_beta_knowledge=knowledge_lifecycle",
        "find_artifact_by_kind_origin_turn" => "tracks/20260101T0000_alpha_track@reviewing",
        "read_artifact_kind" => "track",
        // C-d.1 round 8: the fixture now carries a per-file event store whose
        // newest event advances the artifact past the legacy array's state, so
        // the CORRECT answer is the folded one. An event that cannot be read
        // resolves the artifact to `implementing` — a confidently wrong state,
        // which is what these literals now red on.
        "read_artifact_state" => "reviewing",
        // `read_artifact_status` reports the STATUS FILE, not the fold: it is
        // the raw `status.yaml` (see `domain::status`, whose own doc says the
        // authoritative current state is `resolve_state_with_events`). So it
        // stays `implementing` while the folded state is `reviewing`, and the
        // event store's mode does not reach it. Declared, measured, and left
        // alone — a declaration corrected rather than a method changed.
        "read_artifact_status" => "kind=track;state=implementing",
        "read_artifact_actor_names" => "Author-000001",
        "read_activity_entries" => "1 entries",
        "read_transitions" => "3 transitions",
        "read_transitions_strict" => "3 transitions",
        "read_op_log" => "1 ops",
        "read_carry_forward_if_present" => "CARRY",
        "read_artifact_text" => "# Alpha Track",
        "read_context_file" => "# Product",
        "read_playbook_hook_body" => "HOOK",
        "registry_entry_exists" => "true",
        "resolve_display_name_via_move_execution_row" => "moved",
        "rebuild_decisions_projection" => "rebuilt",
        "rebuild_sparks_projection" => "rebuilt",
        _ => "<undeclared>",
    }
}

fn one_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Run one port method against one address form, and reduce its answer to a
/// single token: either the method's full value, or `ERR:<variant>`.
pub fn invoke(hearth: &Path, port: &str, method: &str, address: &str) -> Result<String, String> {
    // THE THIRD ADDRESS FORM, added by round 7's own unfailability audit. A bare
    // id under a kind directory that is NOT in `KIND_DIRS` is the ONLY route
    // that reaches `scan_subdirs_for_artifact` — the function carrying the
    // round-3, round-4 and round-5 spellings in eleven lines. With only the
    // track addressed, restoring all three of them left the table GREEN,
    // because the per-kind loop above answered first.
    let id: String = match address {
        "bare id" => TRACK_ID.to_string(),
        "relative path" => track_path(),
        "bare id, non-legacy kind" => KNOWLEDGE_ID.to_string(),
        // The FOURTH address form, added by the second pass of the audit. A
        // non-legacy kind by RELATIVE PATH is the only address that reaches
        // `fs_snapshot_adapter::kind_from_status_yaml`'s error arm: the six
        // legacy kinds are answered by `kind_from_path` from the directory name
        // and never open a status.yaml at all. Restoring that arm's swallow
        // (`locate_artifact_dir(..).unwrap_or(None)`) left every earlier version
        // of this table GREEN.
        "relative path, non-legacy kind" => format!("knowledge/{KNOWLEDGE_ID}"),
        other => return Err(format!("unknown address form {other:?}")),
    };
    let q = FileSystemQueryAdapter::new(hearth.to_path_buf());
    let s = FileSystemSnapshotAdapter::new(hearth.to_path_buf());
    // `ERR:` carries the error's VARIANT, not its text, so a row can distinguish
    // "I could not look" (IoError) from "it is not there" (NotFound) — which is
    // the distinction the whole finding is about.
    fn qe<T>(r: Result<T, anvil_core::ports::query_port::QueryError>, ok: impl Fn(T) -> String) -> String {
        use anvil_core::ports::query_port::QueryError as E;
        match r {
            Ok(v) => ok(v),
            Err(E::IoError { .. }) => "ERR:IoError".into(),
            Err(E::NotFound { .. }) => "ERR:NotFound".into(),
            Err(E::MalformedStatus { .. }) => "ERR:MalformedStatus".into(),
            // C-d.1 round 8, H-2: the adoption path's OWN refusal variant. It is
            // discriminated here rather than falling into the debug-formatted
            // catch-all, because a token carrying the error's FIELDS could never
            // equal a literal in a row and so could never be asserted.
            Err(E::AdoptionEvidenceUnreadable { .. }) => "ERR:AdoptionEvidenceUnreadable".into(),
            Err(other) => format!("ERR:{other:?}"),
        }
    }
    fn se<T>(r: Result<T, anvil_core::domain::snapshot::SnapshotError>, ok: impl Fn(T) -> String) -> String {
        use anvil_core::domain::snapshot::SnapshotError as E;
        match r {
            Ok(v) => ok(v),
            Err(E::IoError { .. }) => "ERR:IoError".into(),
            Err(E::NotFound { .. }) => "ERR:NotFound".into(),
            Err(E::MalformedStatus { .. }) => "ERR:MalformedStatus".into(),
            Err(other) => format!("ERR:{other:?}"),
        }
    }

    Ok(match (port, method) {
        ("query", "list_artifacts") => qe(q.list_artifacts(), |v| {
            v.iter()
                .map(|(i, k)| format!("{i}={k}"))
                .collect::<Vec<_>>()
                .join(",")
        }),
        ("query", "find_artifact_by_kind_origin_turn") => {
            qe(q.find_artifact_by_kind_origin_turn("track", "turn-1"), |v| match v {
                Some(a) => format!("{}@{}", a.artifact_path, a.state),
                None => "Ok:NONE-FOR-THIS-TURN".into(),
            })
        }
        ("query", "read_artifact_kind") => qe(q.read_artifact_kind(&id), |v| v),
        ("query", "read_artifact_state") => qe(q.read_artifact_state(&id), |v| v),
        ("query", "read_artifact_status") => qe(q.read_artifact_status(&id), |v| {
            format!(
                "kind={};state={}",
                v.kind.unwrap_or_default(),
                v.state.unwrap_or_default()
            )
        }),
        ("query", "read_activity_entries") => {
            qe(q.read_activity_entries(&id), |v| format!("{} entries", v.len()))
        }
        ("query", "read_transitions") => {
            qe(q.read_transitions(&id), |v| format!("{} transitions", v.len()))
        }
        ("query", "read_transitions_strict") => {
            qe(q.read_transitions_strict(&id), |v| format!("{} transitions", v.len()))
        }
        ("query", "read_op_log") => qe(q.read_op_log(&track_path(), "spec"), |v| {
            format!("{} ops", v.entries().len())
        }),
        ("query", "read_carry_forward_if_present") => {
            qe(q.read_carry_forward_if_present(&track_path()), |v| match v {
                Some(body) => one_line(&body),
                None => "Ok:NO-CARRY-FORWARD".into(),
            })
        }
        ("query", "read_artifact_text") => {
            qe(q.read_artifact_text(&track_path(), "spec.md"), |v| one_line(&v))
        }
        ("query", "read_context_file") => qe(q.read_context_file("product.md"), |v| one_line(&v)),
        ("query", "read_playbook_hook_body") => {
            // The canonical/legacy CHOICE is the swallow here: `full_path.exists()`
            // answering `false` for an unreadable canonical hooks directory sends
            // the read to the LEGACY path, and the operator gets "No such file"
            // for a hook that is on disk. Both spellings end in an IoError, so a
            // check on the variant alone cannot tell them apart — the outcome
            // carries WHICH path the error is about.
            let r = q.read_playbook_hook_body("pb_one", "capture.md");
            match r {
                Ok(v) => one_line(&v),
                Err(e) => {
                    let msg = format!("{e}");
                    if msg.contains("No such file") {
                        "ERR:IoError-NAMING-AN-ABSENT-FILE".to_string()
                    } else {
                        qe::<String>(Err(e), |v| v)
                    }
                }
            }
        }
        ("snapshot", "read_artifact_kind") => se(s.read_artifact_kind(&id), |v| v),
        ("snapshot", "read_artifact_state") => se(s.read_artifact_state(&id), |v| v),
        ("snapshot", "read_artifact_actor_names") => {
            se(s.read_artifact_actor_names(&id), |v| v.join(","))
        }
        ("snapshot", "read_activity_entries") => {
            se(s.read_activity_entries(&id), |v| format!("{} entries", v.len()))
        }
        ("snapshot", "read_transitions") => {
            se(s.read_transitions(&id), |v| format!("{} transitions", v.len()))
        }
        ("snapshot", "registry_entry_exists") => {
            se(s.registry_entry_exists("tracks.md", TRACK_ID), |v| v.to_string())
        }
        ("snapshot", "resolve_display_name_via_move_execution_row") => {
            // `resolve_display_name` is private; `move_execution_row` is the
            // public method that consumes it, and it is the one whose answer an
            // operator sees. Asserting through the PORT is deliberate — L-1 of
            // the round-6 review was that the previous coverage asserted against
            // an internal helper, which is why the sibling defect in the same
            // file went unasked-about.
            se(s.move_execution_row(TRACK_ID, "Complete"), |_| "moved".into())
        }
        ("snapshot", "rebuild_decisions_projection") => {
            se(s.rebuild_decisions_projection(), |_| "rebuilt".into())
        }
        ("snapshot", "rebuild_sparks_projection") => {
            se(s.rebuild_sparks_projection(), |_| "rebuilt".into())
        }
        (p, m) => return Err(format!("no such port method in the matrix: {p}.{m}")),
    })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a hearth holding a track and a knowledge artifact with {string} at mode {string}",
            &[],
            CARRIED,
            |_ctx, params| {
                let token = params.get_string(0).ok_or("expected target")?;
                let mode_s = params.get_string(1).ok_or("expected mode")?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed(&hearth)?;
                // Recorded BEFORE the chmod, from the filesystem, so the
                // scenario can prove its own fixture is non-empty even on the
                // rows where the port is about to refuse to look.
                let seeded = format!(
                    "{}+{}",
                    hearth.join("tracks").join(TRACK_ID).join("status.yaml").is_file(),
                    hearth.join("knowledge").join(KNOWLEDGE_ID).join("status.yaml").is_file()
                );
                degrade(&hearth, token, mode_s)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(SEEDED_KEY, seeded);
                Ok(out)
            },
        ),
        // ── C-d.1 round 8, M-2: ELOOP, and why it needed its own step ──
        //
        // Round 7 declared MUT-Q1, MUT-Q7 and MUT-S1 MASKED, arguing that on a
        // POSIX filesystem `stat` of `<hearth>/<kind>/<id>` can only fail when
        // `<kind>` itself is unreadable, and that same mode makes the scan below
        // refuse. **The argument is scoped to MODE as the only failure lever,
        // and the round-7 reviewer falsified it: `ELOOP` is not a mode.**
        //
        // A mode-parameterised outline cannot express a symlink loop, so the
        // masked verdicts were unfalsifiable BY THE SHAPE OF THE FIXTURE — the
        // same disease as the missing node, in the axis rather than the subject.
        // This step is the axis, so MUT-Q1's red is a CELL now and not a
        // one-off measurement in a review.
        step_def(
            "a hearth holding a track and a knowledge artifact with {string} replaced by a symlink loop",
            &[],
            CARRIED,
            |_ctx, params| {
                let token = params.get_string(0).ok_or("expected target")?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed(&hearth)?;
                let seeded = format!(
                    "{}+{}",
                    hearth.join("tracks").join(TRACK_ID).join("status.yaml").is_file(),
                    hearth.join("knowledge").join(KNOWLEDGE_ID).join("status.yaml").is_file()
                );
                if target(&hearth, token)?.is_none() {
                    return Err("the loop outline needs a named node".to_string());
                }
                degrade(&hearth, token, LOOP_MODE)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(SEEDED_KEY, seeded);
                Ok(out)
            },
        ),
        step_def(
            "the {string} port method {string} is asked for the artifact by {string}",
            CARRIED,
            CARRIED_OUT,
            |mut ctx, params| {
                let port = params.get_string(0).ok_or("expected port")?.to_string();
                let method = params.get_string(1).ok_or("expected method")?.to_string();
                let address = params.get_string(2).ok_or("expected address form")?.to_string();
                let hearth = ctx.get::<String>(HEARTH_KEY).ok_or("no hearth")?.clone();
                let seeded = ctx.get::<String>(SEEDED_KEY).ok_or("no seeded")?.clone();
                let outcome = invoke(Path::new(&hearth), &port, &method, &address)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                if let Some(h) = ctx.take::<Arc<ScratchDir>>(HANDLE_KEY) {
                    out.set(HANDLE_KEY, h);
                }
                out.set(SEEDED_KEY, seeded);
                out.set(OUTCOME_KEY, outcome);
                out.set(METHOD_KEY, format!("{port}.{method}|{address}"));
                Ok(out)
            },
        ),
        check_def(
            "that port method answers {string}",
            &[
                (OUTCOME_KEY, "string"),
                (METHOD_KEY, "string"),
                (SEEDED_KEY, "string"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected answer")?;
                let outcome = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let qualified = ctx.get::<String>(METHOD_KEY).ok_or("no method")?;
                let seeded = ctx.get::<String>(SEEDED_KEY).ok_or("no seeded")?;
                if seeded != "true+true" {
                    return Err(format!(
                        "the FIXTURE did not seed both artifacts ({seeded}). Every row of this \
                         matrix asserts against two artifacts that are on disk; without them a \
                         refusing row is refusing over an empty hearth and proves nothing."
                    ));
                }
                let tail = qualified.split_once('.').map(|(_, t)| t).unwrap_or("");
                let (method, address) = tail.split_once('|').unwrap_or((tail, "bare id"));
                match expected {
                    "resolved" => {
                        let want = expected_value(method, address);
                        if outcome == want {
                            return Ok(());
                        }
                        Err(format!(
                            "{qualified} answered {outcome:?}; the row declares this a CONTROL and \
                             the whole correct answer is {want:?}. Both artifacts are on disk and \
                             this mode can inspect them. A SHORT enumeration fails here on \
                             purpose — an unreadable input read as a smaller one is the finding."
                        ))
                    }
                    "uninspectable" => {
                        if outcome == "ERR:IoError" {
                            return Ok(());
                        }
                        Err(format!(
                            "{qualified} answered {outcome:?}. The artifact is ON DISK and could \
                             not be inspected. `NotFound` is a different fact with a different \
                             operator action (gRPC NOT_FOUND vs INTERNAL); `Ok` with an empty or \
                             short value is worse still, because nothing downstream can tell it \
                             from a hearth that is genuinely that size. 'I could not look' is not \
                             'it is not there'."
                        ))
                    }
                    "kind from the directory" => {
                        // `read_artifact_kind` resolves the six legacy kinds from
                        // the DIRECTORY the artifact sits in, without opening
                        // status.yaml at all. So an unreadable status.yaml does
                        // not stop it, and `resolved` is the honest cell —
                        // declared with its own token so nobody reads it as an
                        // accident. The directory-level rows in table A are where
                        // this method's swallow lived.
                        let want = expected_value(method, address);
                        if outcome == want {
                            return Ok(());
                        }
                        Err(format!(
                            "{qualified} answered {outcome:?}; the kind is carried by the \
                             DIRECTORY here, so the whole correct answer is {want:?} even with \
                             the status file unreadable."
                        ))
                    }
                    "adoption evidence unreadable" => {
                        // C-d.1 round 8, H-2. `read_transitions_strict` is the
                        // FAIL-CLOSED adoption reader: an adoption reset must see
                        // the COMPLETE governance history or refuse, because a
                        // dropped event is exactly the one that proves the
                        // artifact is governed. Its refusal has its OWN variant
                        // (`AdoptionEvidenceUnreadable`), which `begin` maps to a
                        // different operator action than `IoError`, so this token
                        // asserts the variant and not merely "an error".
                        //
                        // It failed OPEN at exactly 0600 — `Ok(0 transitions)`
                        // over two events on disk, while refusing at 0300 and
                        // 0000 — through `if !path.is_file() { continue; }`, the
                        // round-4 lexeme, in the one function written to be safe
                        // from it. `begin.rs`'s comment above the call names the
                        // outcome the code produced.
                        if outcome == "ERR:AdoptionEvidenceUnreadable" {
                            return Ok(());
                        }
                        Err(format!(
                            "{qualified} answered {outcome:?}. This is the FAIL-CLOSED adoption \
                             evidence read. `Ok` here — of ANY length — is `begin`'s \
                             `transitions.is_empty()` reading 'never governed' over an artifact \
                             whose governance evidence merely could not be read, and the reset \
                             proceeds. A plain `IoError` is not right either: the adoption path \
                             has its own variant so the operator is told which decision is \
                             blocked."
                        ))
                    }
                    "address not accepted" => {
                        // A DECLARED ASYMMETRY, not a pass. `SnapshotPort`'s
                        // lookup has no subdirectory scan, so a bare id under a
                        // kind directory outside `ARTIFACT_DIRS` resolves nowhere
                        // and answers NotFound at EVERY mode — including the
                        // readable control. That is not the class (it never
                        // looked, so it swallowed nothing), and it is not
                        // nothing either: `QueryPort` accepts the same address.
                        // The control row is what keeps this honest — if the
                        // scan is ever added to the snapshot adapter, the
                        // readable row reds and this declaration must be redone.
                        if outcome == "ERR:NotFound" {
                            return Ok(());
                        }
                        Err(format!(
                            "{qualified} answered {outcome:?}. This row declares that this port \
                             does not accept this address form and says NotFound at every mode. \
                             It no longer does — re-derive the row rather than widening this \
                             declaration."
                        ))
                    }
                    other => Err(format!(
                        "the scenario declares an answer this check does not know: {other:?}"
                    )),
                }
            },
        ),
    ]
}
