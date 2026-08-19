//! Step module for `generated_wire_boundary.feature` — B-i.2a's boundary check.
//!
//! WHAT IS ASSERTED, AND WHY A COUNT CANNOT SEE IT
//! ------------------------------------------------
//! B-i.2's identifier namespace is SHARED. Five names denote both an `anvil-core`
//! type/field and a prost-generated wire type/field:
//!
//! | name                                     | core                                    | wire                    |
//! |------------------------------------------|-----------------------------------------|-------------------------|
//! | `PlaybookActivityEntry`                  | `domain::artifact_activity` (pre-B-i.2b) | `proto/anvil.proto:772` |
//! | `PlaybookRunFidelity`               | `domain::playbook_run_fidelity` (pre-B-i.2b) | `proto/anvil.proto:1078`|
//! | `ResolvedHook::artifact_kind`            | `domain::hook_manifest:30`               | `proto/anvil.proto:892` |
//! | `ActorActivityEntry::artifact_kinds`     | `domain::actor_activity:38`              | `proto/anvil.proto:941` |
//! | `CatalogResponse::available_artifact_kinds` | (none)                                | `proto/anvil.proto:46`  |
//!
//! So a rename applied to the WHOLE namespace compiles perfectly well while
//! having moved the wire — which is exactly what B-i is forbidden from doing,
//! and exactly what round 4 caught B-i.1's first seam failing to detect for 15
//! of 17 codes. The failure mode is a rename that succeeds, not one that breaks.
//!
//! The check therefore has three independent legs, because each catches a case
//! the others cannot:
//!
//!   1. **Compile-pin.** Every ledgered prost type is named in a TYPE position
//!      and every ledgered wire field is written in a FIELD position, inside
//!      `mirrored_wire_surface()`. A rename that sweeps `proto/anvil.proto` and
//!      every use site together is still a compile error HERE, because this
//!      module is the one place the old names are pinned rather than swept.
//!   2. **Value-pin against the contract source.** `proto/anvil.proto` is parsed
//!      and each ledgered message/field is required to be declared with exactly
//!      its pinned name AND tag. Field tags are never reused (`spec.md:783`), so
//!      pinning the tag makes a renumber red too.
//!   3. **Population-pin.** The set of `Playbook`-named messages in the proto is
//!      required to be EXACTLY the ledgered eleven. A twelfth message, or a
//!      deleted one, reds — the round-5 MEDIUM-2 lesson (a pin list that a new
//!      member is not forced into is a pin list about a subset).
//!
//! A fourth leg guards the other direction: B-i.2b renames core-domain Rust
//! FIELDS whose persisted JSONL key is a hand-written string literal. Scenario 4
//! writes a real record through the real `FileSystemActivityLogAdapter` and reads
//! the byte on disk, so a rename that also moved the literal reds by value.

use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core::ports::activity_log_port::{ActivityLogRecord, ActivityLogWritePort};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const PROTO_KEY: &str = "gwb_proto_source";
const SINK_KEY: &str = "gwb_sink_hearth";
const SINK_HANDLE_KEY: &str = "gwb_sink_handle";

/// EVERY `Playbook*` message `proto/anvil.proto` declares, plus
/// `AvailableArtifactKind` — the one message this migration renamed OUT of the
/// playbook namespace, which would otherwise fall out of every pin below.
///
/// Pinned as `(rust type name, proto declaration line)`. `mirrored_wire_surface`
/// below names each of these in a type position, so this list cannot drift from
/// the generated code without a compile error.
///
/// THE LINE PIN CATCHES RENAMES, NOT APPENDS. Every rpc appended to the
/// `service AnvilService` block shifts every message below it, so a pure append
/// moves all twenty-two by the SAME delta. Re-record them together (T-RD-PBK-DEPTH's
/// `rpc RunDetail` moved them +1) and check the delta is uniform — a NON-uniform
/// shift means a message moved relative to its neighbours, which is what this pin
/// exists to catch.
///
/// T1 (`conversation_scoped_join`) is the first re-record whose delta is
/// DELIBERATELY NON-UNIFORM, and the reason is stated rather than smoothed over:
/// it appended `rpc JoinCoverage` to the service block (+1 to everything) AND
/// `string conversation_hash = 17;` with its comment INSIDE `RouteResponse`
/// (a further +8 to everything BELOW that message only). So
/// `AvailableArtifactKind`, which is declared above `RouteResponse`, moved +1
/// while the other twenty-one moved +9. Two contiguous blocks, each internally
/// uniform, and no ledgered message moved relative to a neighbour in its own
/// block — which is the property this pin is actually about. A shift that is
/// non-uniform WITHIN a block is still the defect.
const WIRE_MESSAGES: &[(&str, u32)] = &[
    ("AvailableArtifactKind", 60),
    ("PersistPlaybookRequest", 633),
    ("PlaybookHook", 651),
    ("PersistPlaybookResponse", 656),
    ("CandidatePlaybook", 662),
    ("IntakeCandidatePlaybookRequest", 737),
    ("IntakeCandidatePlaybookResponse", 752),
    ("PlaybookActivityRequest", 795),
    ("PlaybookActivityResponse", 806),
    ("PlaybookOwnerGroup", 819),
    ("PlaybookActivityEntry", 825),
    ("ArtifactKindPeriodCount", 878),
    ("PlaybookStepVolumeRequest", 890),
    ("PlaybookStepVolumeResponse", 898),
    ("PlaybookFidelityRequest", 1085),
    ("PlaybookFidelityResponse", 1092),
    ("PlaybookRunFidelity", 1131),
    ("PlaybookAtlasRequest", 1158),
    ("PlaybookAtlasResponse", 1162),
    ("PlaybookAtlasDetailRequest", 1218),
    ("PlaybookAtlasDetailResponse", 1223),
    ("BacklogPlaybookBinding", 1533),
];

/// EVERY `playbook`-named wire FIELD, as `(message, field, tag)` — NINE
/// declarations over SEVEN distinct names.
///
/// Round-6 MEDIUM-3: this list held three, which by Leg 3's own standard ("a pin
/// list a new member is not forced into is a pin list about a subset") made it a
/// subset pin. `plan.md`'s B-i.2 families cover only the `artifact_kind(s)`
/// three; the other six are equally wire and equally B-w.1's, and a `playbook_*`
/// field added to the proto used to red nothing.
///
/// `PLAYBOOK_FIELD_POPULATION` below closes it: the proto's `playbook`-named
/// field-NAME set must be EXACTLY the ledgered seven, so an eighth reds.
const WIRE_FIELDS: &[(&str, &str, u32)] = &[
    // The three whose names COLLIDE with `anvil-core` field names — the reason
    // B-i.2 needed a per-site classification at all.
    ("ResolvedHook", "artifact_kind", 1),
    ("ActorActivityEntry", "artifact_kinds", 4),
    ("CatalogResponse", "available_artifact_kinds", 5),
    // `execution_route` -> `execution_route` (spec.md:760). Three declarations.
    ("ActiveArtifact", "execution_route", 5),
    ("AvailableArtifactType", "execution_route", 4),
    ("ActionInfo", "execution_route", 3),
    // Measurement roll-ups keyed by artifact kind.
    ("TimeBucket", "per_artifact_kind", 3),
    ("ActivitySummaryResponse", "by_artifact_kind", 6),
    ("ActivitySummaryResponse", "playbook_step_turns", 12),
    // K8 backlog binding (a4/k8): `playbook`-named wire fields must be ledgered or
    // Leg 3's population pin reds — a pin list a new member is not forced into is a
    // pin list about a subset.
    ("BacklogPlaybookBinding", "playbook_definition_id", 1),
    ("BacklogPlaybookBinding", "no_playbook_definition", 2),
    ("BacklogExecutionBinding", "playbook_definition_id", 2),
];

/// The DISTINCT `playbook`-named field names the proto declares. Separate from
/// `WIRE_FIELDS` because `execution_route` is declared three times and
/// `ActivitySummaryResponse` carries two different names — the population is
/// about NAMES, the pin table is about DECLARATIONS.
const PLAYBOOK_FIELD_POPULATION: &[&str] = &[
    // `by_artifact_kind` / `per_artifact_kind` are NOT members: they are keyed
    // by the governed artifact kind, not by a playbook identity, so they carry
    // the artifact-kind name and fall outside a `playbook`-named population.
    "playbook_id",
    "playbook_name",
    "playbook_step_turns",
    "playbook_version",
    // K8 backlog binding: a backlog item names the playbook definition it binds
    // to, or declares the absence of one. Both are `playbook`-named fields and
    // so are members of this population by the same rule as the four above.
    "no_playbook_definition",
    "playbook_definition_id",
    // BacklogExecutionBinding: renamed off the retired `workflow_instance_id`
    // token, so it joins the playbook-named population by the same rule.
    "playbook_run_id",
];

/// LEG 1 — the compile-pin.
///
/// Every ledgered message is named in a TYPE position and every ledgered field
/// in a FIELD position. `std::any::type_name` is then compared against the pin
/// list by VALUE, so the two can never silently disagree.
///
/// This function's ONLY job is to fail to compile. Do not "simplify" it into a
/// loop over `WIRE_MESSAGES`: a loop over strings compiles no matter what the
/// generated code is called, which is precisely the defect round 4 found.
fn mirrored_wire_surface() -> Vec<(&'static str, &'static str)> {
    use anvil_engine::proto::{
        ActorActivityEntry, AvailableArtifactKind, BacklogPlaybookBinding, CandidatePlaybook, CatalogResponse,
        IntakeCandidatePlaybookRequest, IntakeCandidatePlaybookResponse, PersistPlaybookRequest,
        PersistPlaybookResponse, PlaybookActivityEntry, PlaybookActivityRequest,
        PlaybookActivityResponse, PlaybookAtlasDetailRequest, PlaybookAtlasDetailResponse,
        PlaybookAtlasRequest, PlaybookAtlasResponse, PlaybookFidelityRequest,
        PlaybookFidelityResponse, PlaybookHook, PlaybookOwnerGroup, ArtifactKindPeriodCount,
        PlaybookRunFidelity, PlaybookStepVolumeRequest, PlaybookStepVolumeResponse, ResolvedHook,
    };

    fn last(path: &'static str) -> &'static str {
        match path.rsplit_once("::") {
            Some((_, tail)) => tail,
            None => path,
        }
    }

    // FIELD position: each of the three ledgered wire fields is WRITTEN here.
    // Renaming `ResolvedHook.artifact_kind` in the proto breaks this line.
    let _resolved_hook = ResolvedHook {
        artifact_kind: String::new(),
        state: String::new(),
        role: String::new(),
        body: String::new(),
        gate: String::new(),
    };
    let _actor_entry = ActorActivityEntry {
        actor: String::new(),
        begin_count: 0,
        last_active: String::new(),
        artifact_kinds: Vec::new(),
    };
    let _catalog = CatalogResponse {
        available_artifact_kinds: Vec::new(),
        ..Default::default()
    };
    // Read them back by name too, so a field that is renamed AND re-initialised
    // through `..Default::default()` still reds.
    let _: &String = &_resolved_hook.artifact_kind;
    let _: &Vec<String> = &_actor_entry.artifact_kinds;
    let _: &Vec<AvailableArtifactKind> = &_catalog.available_artifact_kinds;

    // TYPE position: one entry per ledgered message, in `WIRE_MESSAGES` order.
    vec![
        ("AvailableArtifactKind", last(std::any::type_name::<AvailableArtifactKind>())),
        ("PersistPlaybookRequest", last(std::any::type_name::<PersistPlaybookRequest>())),
        ("PlaybookHook", last(std::any::type_name::<PlaybookHook>())),
        ("PersistPlaybookResponse", last(std::any::type_name::<PersistPlaybookResponse>())),
        ("CandidatePlaybook", last(std::any::type_name::<CandidatePlaybook>())),
        ("IntakeCandidatePlaybookRequest", last(std::any::type_name::<IntakeCandidatePlaybookRequest>())),
        ("IntakeCandidatePlaybookResponse", last(std::any::type_name::<IntakeCandidatePlaybookResponse>())),
        ("PlaybookActivityRequest", last(std::any::type_name::<PlaybookActivityRequest>())),
        ("PlaybookActivityResponse", last(std::any::type_name::<PlaybookActivityResponse>())),
        ("PlaybookOwnerGroup", last(std::any::type_name::<PlaybookOwnerGroup>())),
        ("PlaybookActivityEntry", last(std::any::type_name::<PlaybookActivityEntry>())),
        ("ArtifactKindPeriodCount", last(std::any::type_name::<ArtifactKindPeriodCount>())),
        ("PlaybookStepVolumeRequest", last(std::any::type_name::<PlaybookStepVolumeRequest>())),
        ("PlaybookStepVolumeResponse", last(std::any::type_name::<PlaybookStepVolumeResponse>())),
        ("PlaybookFidelityRequest", last(std::any::type_name::<PlaybookFidelityRequest>())),
        ("PlaybookFidelityResponse", last(std::any::type_name::<PlaybookFidelityResponse>())),
        ("PlaybookRunFidelity", last(std::any::type_name::<PlaybookRunFidelity>())),
        ("PlaybookAtlasRequest", last(std::any::type_name::<PlaybookAtlasRequest>())),
        ("PlaybookAtlasResponse", last(std::any::type_name::<PlaybookAtlasResponse>())),
        ("PlaybookAtlasDetailRequest", last(std::any::type_name::<PlaybookAtlasDetailRequest>())),
        ("PlaybookAtlasDetailResponse", last(std::any::type_name::<PlaybookAtlasDetailResponse>())),
        ("BacklogPlaybookBinding", last(std::any::type_name::<BacklogPlaybookBinding>())),
    ]
}

fn proto_path() -> PathBuf {
    // `anvil-test-support/` -> workspace root -> `proto/anvil.proto`.
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .expect("workspace root")
        .join("proto/anvil.proto")
}

/// Parse `proto/anvil.proto` into `(message name, Vec<(field name, tag)>)`.
/// Deliberately tiny: it only needs `message X {` and `<type> <name> = <tag>;`.
fn parse_proto(src: &str) -> Vec<(String, Vec<(String, u32)>)> {
    let mut out: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    let mut current: Option<(String, Vec<(String, u32)>)> = None;
    for raw in src.lines() {
        let line = raw.trim();
        if line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("message ") {
            if let Some(name) = rest.split_whitespace().next() {
                if let Some(done) = current.take() {
                    out.push(done);
                }
                current = Some((name.to_string(), Vec::new()));
                continue;
            }
        }
        if line == "}" {
            if let Some(done) = current.take() {
                out.push(done);
            }
            continue;
        }
        if let Some((decl, tail)) = line.split_once('=') {
            if let Some(fields) = current.as_mut() {
                let name = decl.split_whitespace().last().unwrap_or("");
                let tag: String = tail
                    .trim()
                    .trim_end_matches(';')
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    if let Ok(tag) = tag.parse::<u32>() {
                        fields.1.push((name.to_string(), tag));
                    }
                }
            }
        }
    }
    if let Some(done) = current.take() {
        out.push(done);
    }
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the anvil proto contract source",
            &[],
            &[(PROTO_KEY, "String")],
            |_ctx, _params| {
                let src = std::fs::read_to_string(proto_path())
                    .map_err(|e| format!("cannot read {}: {}", proto_path().display(), e))?;
                let mut out = Context::new();
                out.set(PROTO_KEY, src);
                Ok(out)
            },
        ),
        // ---- LEG 1: compile-pin + value-pin of the generated Rust names ------
        check_def(
            "every ledgered wire message is still generated under its pinned name",
            &[],
            |_ctx, _params| {
                let mirrored = mirrored_wire_surface();
                if mirrored.len() != WIRE_MESSAGES.len() {
                    return Err(format!(
                        "mirrored_wire_surface() names {} messages, WIRE_MESSAGES pins {}",
                        mirrored.len(),
                        WIRE_MESSAGES.len()
                    ));
                }
                let mut moved = Vec::new();
                for ((pinned, _line), (declared, actual)) in WIRE_MESSAGES.iter().zip(&mirrored) {
                    if pinned != declared || pinned != actual {
                        moved.push(format!(
                            "  pinned `{}`, mirrored as `{}`, generated as `{}`",
                            pinned, declared, actual
                        ));
                    }
                }
                if moved.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} generated wire message name(s) moved. Renaming a proto message is \
                         B-w.1's, under NG-PROTO-VNEXT — not B-i's:\n{}",
                        moved.len(),
                        moved.join("\n")
                    ))
                }
            },
        ),
        // ---- LEG 2: value-pin against the contract source --------------------
        check_def(
            "the proto declares each ledgered message at its pinned line",
            &[(PROTO_KEY, "String")],
            |ctx, _params| {
                let src = ctx.get::<String>(PROTO_KEY).ok_or("No proto source")?;
                let mut missing = Vec::new();
                for (name, line) in WIRE_MESSAGES {
                    let found = src
                        .lines()
                        .enumerate()
                        .find(|(_, l)| l.trim() == format!("message {} {{", name));
                    match found {
                        None => missing.push(format!("  `message {}` not declared at all", name)),
                        Some((idx, _)) if idx as u32 + 1 != *line => missing.push(format!(
                            "  `message {}` pinned at :{}, found at :{}",
                            name,
                            line,
                            idx + 1
                        )),
                        Some(_) => {}
                    }
                }
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} ledgered wire message(s) moved in proto/anvil.proto:\n{}",
                        missing.len(),
                        missing.join("\n")
                    ))
                }
            },
        ),
        check_def(
            "the proto declares each ledgered wire field with its pinned name and tag",
            &[(PROTO_KEY, "String")],
            |ctx, _params| {
                let src = ctx.get::<String>(PROTO_KEY).ok_or("No proto source")?;
                let parsed = parse_proto(src);
                let mut bad = Vec::new();
                for (msg, field, tag) in WIRE_FIELDS {
                    match parsed.iter().find(|(m, _)| m == msg) {
                        None => bad.push(format!("  message `{}` not found", msg)),
                        Some((_, fields)) => match fields.iter().find(|(f, _)| f == field) {
                            None => bad.push(format!(
                                "  `{}.{}` not declared; message carries [{}]",
                                msg,
                                field,
                                fields
                                    .iter()
                                    .map(|(f, t)| format!("{}={}", f, t))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )),
                            Some((_, actual)) if actual != tag => bad.push(format!(
                                "  `{}.{}` pinned tag {}, declared tag {} — field tags are never \
                                 reused (spec.md:783)",
                                msg, field, tag, actual
                            )),
                            Some(_) => {}
                        },
                    }
                }
                if bad.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} ledgered wire field(s) moved. Renaming a wire field is B-w.1's, under \
                         NG-PROTO-VNEXT — not B-i's:\n{}",
                        bad.len(),
                        bad.join("\n")
                    ))
                }
            },
        ),
        // ---- LEG 3: population-pin ------------------------------------------
        check_def(
            "the Playbook-named message set in the proto is exactly the ledgered population",
            &[(PROTO_KEY, "String")],
            |ctx, _params| {
                let src = ctx.get::<String>(PROTO_KEY).ok_or("No proto source")?;
                let mut declared: Vec<String> = parse_proto(src)
                    .into_iter()
                    .map(|(m, _)| m)
                    .filter(|m| m.contains("Playbook"))
                    .collect();
                declared.sort();
                let mut pinned: Vec<String> = WIRE_MESSAGES
                    .iter()
                    .map(|(n, _)| n.to_string())
                    .filter(|n| n.contains("Playbook"))
                    .collect();
                pinned.sort();
                if declared == pinned {
                    return Ok(());
                }
                let extra: Vec<&String> = declared.iter().filter(|d| !pinned.contains(d)).collect();
                let gone: Vec<&String> = pinned.iter().filter(|p| !declared.contains(p)).collect();
                Err(format!(
                    "the ledgered wire set is no longer the whole population: {} unledgered \
                     ({:?}), {} ledgered-but-gone ({:?}). B-w.1 inherits a NAMED set; an \
                     unledgered Playbook* message means the ledger is a sample.",
                    extra.len(),
                    extra,
                    gone.len(),
                    gone
                ))
            },
        ),
        check_def(
            "the playbook-named field set in the proto is exactly the ledgered population",
            &[(PROTO_KEY, "String")],
            |ctx, _params| {
                let src = ctx.get::<String>(PROTO_KEY).ok_or("No proto source")?;
                let mut declared: Vec<String> = parse_proto(src)
                    .into_iter()
                    .flat_map(|(_m, fields)| fields.into_iter().map(|(f, _t)| f))
                    .filter(|f| f.contains("playbook"))
                    .collect();
                declared.sort();
                declared.dedup();
                let mut pinned: Vec<String> =
                    PLAYBOOK_FIELD_POPULATION.iter().map(|s| s.to_string()).collect();
                pinned.sort();
                if declared == pinned {
                    return Ok(());
                }
                let extra: Vec<&String> = declared.iter().filter(|d| !pinned.contains(d)).collect();
                let gone: Vec<&String> = pinned.iter().filter(|p| !declared.contains(p)).collect();
                Err(format!(
                    "the ledgered wire FIELD set is no longer the whole population: {} unledgered \
                     ({:?}), {} ledgered-but-gone ({:?}). Leg 3's rule applies to fields too — a \
                     pin list a new member is not forced into is a pin list about a subset, and \
                     B-w.1 inherits this set.",
                    extra.len(),
                    extra,
                    gone.len(),
                    gone
                ))
            },
        ),
        // ---- LEG 3b: zero residue ---------------------------------------------
        // The population pins above are about the CANONICAL namespace. This one
        // is about the retired one: after the rename there is no `Workflow`
        // message and no `workflow` field left to be a second name for
        // something. It is the check the whole migration exists to satisfy, and
        // it can be reddened by adding a single field.
        check_def(
            "no message or field in the proto still carries the retired token",
            &[(PROTO_KEY, "String")],
            |ctx, _params| {
                let src = ctx.get::<String>(PROTO_KEY).ok_or("No proto source")?;
                let parsed = parse_proto(src);
                let mut residue: Vec<String> = Vec::new();
                for (message, fields) in &parsed {
                    if message.to_lowercase().contains("workflow") {
                        residue.push(format!("  message `{}`", message));
                    }
                    for (field, _tag) in fields {
                        if field.to_lowercase().contains("workflow") {
                            residue.push(format!("  field `{}.{}`", message, field));
                        }
                    }
                }
                if residue.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} wire name(s) still carry the retired token — the rename left a \
                         second name for something:\n{}",
                        residue.len(),
                        residue.join("\n")
                    ))
                }
            },
        ),
        // ---- LEG 4: the persisted-key direction ------------------------------
        step_def(
            "an activity-log sink at a fresh hearth",
            &[],
            &[
                (SINK_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp = tempfile::TempDir::new()
                    .map_err(|e| format!("failed to create temp hearth: {e}"))?;
                let root = temp.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp)));
                let mut out = Context::new();
                out.set(SINK_KEY, root);
                out.set(SINK_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a command turn for artifact kind {string} is recorded",
            &[
                (SINK_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (SINK_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let dir = ctx.get::<PathBuf>(SINK_KEY).ok_or("No sink hearth")?.clone();
                let adapter = FileSystemActivityLogAdapter::new(&dir);
                adapter
                    .append_activity_log(&ActivityLogRecord {
                        command: "begin".to_string(),
                        outcome: "ok".to_string(),
                        artifact_kind: kind,
                        from_state: String::new(),
                        to_state: "drafting".to_string(),
                        actor_hash: None,
                        at: "2026-01-01T00:00:00Z".to_string(),
                        source: "brine".to_string(),
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        call_state: None,
                    })
                    .map_err(|e| format!("append failed: {}", e))?;
                // Carry the TempDir handle forward: dropping it deletes the
                // hearth, and a check that reads an already-deleted directory
                // would fail for the wrong reason.
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(SINK_HANDLE_KEY)
                    .ok_or("No sink handle")?
                    .clone();
                let mut out = Context::new();
                out.set(SINK_KEY, dir);
                out.set(SINK_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the persisted line still carries the key {string} with value {string}",
            &[(SINK_KEY, "PathBuf")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let value = params.get_string(1).ok_or("Expected value")?;
                let dir = ctx.get::<PathBuf>(SINK_KEY).ok_or("No sink hearth")?;
                let mut found: Option<String> = None;
                for entry in walk(dir) {
                    if let Ok(text) = std::fs::read_to_string(&entry) {
                        for line in text.lines() {
                            if line.contains("\"command\":\"begin\"") {
                                found = Some(line.to_string());
                            }
                        }
                    }
                }
                let line = found.ok_or_else(|| {
                    format!("no activity-log line written under {}", dir.display())
                })?;
                let needle = format!("\"{}\":\"{}\"", key, value);
                if line.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "the persisted JSONL key moved. The Rust FIELD is B-i.2b's; the persisted \
                         KEY is C-d.2's, and this record is hand-serialised so the two are \
                         independent. Expected `{}` in:\n  {}",
                        needle, line
                    ))
                }
            },
        ),
    ]
}

fn walk(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
