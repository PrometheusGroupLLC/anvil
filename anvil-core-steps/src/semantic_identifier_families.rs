//! Step module for `semantic_identifier_families.feature` — B-i.2's semantic seam.
//!
//! WHY THIS EXISTS, STATED BLUNTLY
//! -------------------------------
//! `plan.md`'s B-i.2 Verify named this file and named its mutation: pasting
//! `playbook_kind` over `artifact_kind` rather than `artifact_kind` must turn a
//! semantic assertion red. Fix round 7 landed the rename WITHOUT the seam. Round
//! 6 then ran the plan's mutation against the landed tree — 102 sites retargeted
//! to `playbook_kind`, identical length so no column drift — and every gate
//! stayed green: workspace build clean, anvil-core 1295/0, anvil-engine 521/0,
//! the wire seam 4/0, both verifiers PASS.
//!
//! It stayed green because nothing in the tree asserted WHICH target was right.
//! `bi2-apply-rename.py --verify` compares the tree to the artifact the
//! implementer wrote, so it is circular with respect to the choice; the persisted
//! JSONL key is a hand-written literal, so a Rust field rename is invisible to
//! every behavioural suite. `spec.md:767`'s "a count-only rename is forbidden"
//! was, for these four families, unenforced.
//!
//! THE DISTINCTION
//! ---------------
//! An ARTIFACT KIND is what an artifact IS: `track`, `decision`, `milestone`.
//! A PLAYBOOK IDENTITY is the definition that GOVERNS that kind:
//! `20260422T0000_track_lifecycle`. `PlaybookRegistry` maps kind -> identity via
//! `playbook_id_for`, and the two namespaces are disjoint. The renamed field
//! holds the FIRST. `playbook_kind` claims it holds the second — false of every
//! value the fold sees.
//!
//! THREE LEGS, because a rename changes no value
//! ---------------------------------------------
//! LEG A  (`renamed_family_members`) binds every renamed member by NAME in a
//!        pattern or type position. `artifact_kind` and `artifact_kinds` are bound
//!        by EXHAUSTIVE struct destructuring, so `playbook_kind` cannot compile.
//! LEG A' requires the pinned set to equal the classification's `rename` targets.
//!        Rust has no exhaustive construct for "every type this phase renamed" — a
//!        `vec![...]` of types compiles whatever the phase did — so the
//!        classification artifact is the forcing function. Without it the type pin
//!        would be a subset pin, which is round-5 MEDIUM-2's defect in a new place.
//! LEG B  (every other scenario) asserts the value domain that makes
//!        `artifact_kind` the RIGHT name rather than an arbitrary one, over real
//!        folds and the real seed registry — and is failable on its own terms:
//!        collapsing kind and identity reds it, and so does making fidelity
//!        measure the definition instead of the run.

use anvil_core::domain::activity_summary::artifact_kind_counts;
use anvil_core::domain::playbook::registry::{
    playbook_generation_aliases, PlaybookRegistry, SeedPlaybookRegistry,
};
use anvil_core::domain::artifact_activity::{
    ArtifactActivityEntry, ArtifactActivityQuery, ArtifactActivityResult, OwnerResolver,
};
use anvil_core::domain::playbook_run_fidelity::{
    fold_playbook_run_fidelity, PlaybookRunFidelityResult, PlaybookRunInstanceFidelity,
};
use anvil_core::ports::activity_log_port::ActivityLogRecord;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;
use std::path::PathBuf;

const REGISTRY_KEY: &str = "sif_registry";
const RECORDS_KEY: &str = "sif_records";
const COUNTS_KEY: &str = "sif_counts";
const ACTIVITY_KEY: &str = "sif_activity";
const FIDELITY_KEY: &str = "sif_fidelity";

/// LEG A — the compile-pin.
///
/// Every identifier B-i.2b renamed is bound HERE by name, in a position the
/// compiler checks. `artifact_kind` and `artifact_kinds` are bound by exhaustive
/// STRUCT DESTRUCTURING (no `..`), so renaming the field to `playbook_kind` — or
/// to anything else — is `error[E0026]/E0027` in this file.
///
/// Do NOT "simplify" this into a list of strings. A list of strings compiles no
/// matter what the fields are called, which is the exact defect round 6 found.
fn renamed_family_members() -> Vec<&'static str> {
    // FIELD position, exhaustive: adding, removing or renaming a field of
    // `ActivityLogRecord` fails to compile until this pattern is updated.
    let probe = ActivityLogRecord {
        command: String::new(),
        outcome: String::new(),
        artifact_kind: String::new(),
        from_state: String::new(),
        to_state: String::new(),
        actor_hash: None,
        at: String::new(),
        source: String::new(),
        conversation_hash: None,
        project_label: None,
        playbook_run_id: None,
        call_state: None,
    };
    let ActivityLogRecord {
        command: _,
        outcome: _,
        artifact_kind: _bound_by_name,
        from_state: _,
        to_state: _,
        actor_hash: _,
        at: _,
        source: _,
        conversation_hash: _,
        project_label: _,
        playbook_run_id: _,
        call_state: _,
    } = probe;

    // The plural, likewise bound by exhaustive destructuring.
    let entry = anvil_core::domain::actor_activity::ActorActivityEntry {
        actor: String::new(),
        begin_count: 0,
        last_active: String::new(),
        artifact_kinds: Vec::new(),
    };
    let anvil_core::domain::actor_activity::ActorActivityEntry {
        actor: _,
        begin_count: _,
        last_active: _,
        artifact_kinds: _also_bound_by_name,
    } = entry;

    fn last(path: &'static str) -> &'static str {
        path.rsplit_once("::").map(|(_, t)| t).unwrap_or(path)
    }

    // TYPE position for the five renamed types.
    let mut out = vec![
        last(std::any::type_name::<ArtifactActivityEntry>()),
        last(std::any::type_name::<ArtifactActivityQuery>()),
        last(std::any::type_name::<ArtifactActivityResult>()),
        last(std::any::type_name::<PlaybookRunFidelityResult>()),
        last(std::any::type_name::<PlaybookRunInstanceFidelity>()),
        "artifact_kind",
        "artifact_kinds",
    ];
    out.sort();
    out
}

struct FlatOwners;
impl OwnerResolver for FlatOwners {
    fn owner_for(&self, _kind: &str) -> Option<String> {
        Some("anvil".to_string())
    }
}

fn record(kind: &str, instance: Option<&str>, to_state: &str, at: &str) -> ActivityLogRecord {
    ActivityLogRecord {
        command: "begin".to_string(),
        outcome: "ok".to_string(),
        artifact_kind: kind.to_string(),
        from_state: String::new(),
        to_state: to_state.to_string(),
        actor_hash: None,
        at: at.to_string(),
        source: "brine".to_string(),
        conversation_hash: None,
        project_label: None,
        playbook_run_id: instance.map(str::to_string),
        call_state: None,
    }
}

/// The kinds the seed registry governs. Read from the registry, not hardcoded.
fn governed_kinds(reg: &SeedPlaybookRegistry) -> Vec<String> {
    reg.all_machines().iter().map(|m| m.kind.clone()).collect()
}

fn playbook_ids(reg: &SeedPlaybookRegistry) -> Vec<String> {
    governed_kinds(reg)
        .iter()
        .filter_map(|k| reg.playbook_id_for(k))
        .collect()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the real seed playbook registry",
            &[],
            &[(REGISTRY_KEY, "SeedPlaybookRegistry")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(REGISTRY_KEY, SeedPlaybookRegistry);
                Ok(out)
            },
        ),
        step_def(
            "an activity-log stream whose artifact kinds are {string}",
            &[(REGISTRY_KEY, "SeedPlaybookRegistry")],
            &[
                (REGISTRY_KEY, "SeedPlaybookRegistry"),
                (RECORDS_KEY, "Vec<ActivityLogRecord>"),
            ],
            |ctx, params| {
                let kinds = params.get_string(0).ok_or("Expected kinds")?;
                let records: Vec<ActivityLogRecord> = kinds
                    .split(',')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .enumerate()
                    .map(|(i, k)| {
                        record(k, Some(&format!("run-{i}")), "drafting", "2026-01-01T00:00:00Z")
                    })
                    .collect();
                let mut out = Context::new();
                out.set(REGISTRY_KEY, SeedPlaybookRegistry);
                out.set(RECORDS_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "two separate runs of artifact kind {string}",
            &[(REGISTRY_KEY, "SeedPlaybookRegistry")],
            &[
                (REGISTRY_KEY, "SeedPlaybookRegistry"),
                (RECORDS_KEY, "Vec<ActivityLogRecord>"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                // Two DISTINCT instance ids, same kind. If fidelity measured the
                // definition rather than the execution these would collapse to one.
                let records = vec![
                    record(kind, Some("run-alpha"), "drafting", "2026-01-01T00:00:00Z"),
                    record(kind, Some("run-alpha"), "review", "2026-01-01T00:01:00Z"),
                    record(kind, Some("run-beta"), "drafting", "2026-01-01T00:02:00Z"),
                    record(kind, Some("run-beta"), "review", "2026-01-01T00:03:00Z"),
                ];
                let mut out = Context::new();
                out.set(REGISTRY_KEY, SeedPlaybookRegistry);
                out.set(RECORDS_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "the per-kind artifact activity is folded",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(COUNTS_KEY, "BTreeMap<String, u64>")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .ok_or("No records")?;
                let mut out = Context::new();
                out.set(COUNTS_KEY, artifact_kind_counts(records));
                Ok(out)
            },
        ),
        step_def(
            "the artifact activity is folded",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(ACTIVITY_KEY, "ArtifactActivityResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .ok_or("No records")?;
                let counts = artifact_kind_counts(records);
                let result =
                    ArtifactActivityQuery::execute_with_counts(&SeedPlaybookRegistry, &FlatOwners, &counts);
                let mut out = Context::new();
                out.set(ACTIVITY_KEY, result);
                Ok(out)
            },
        ),
        step_def(
            "the playbook-run fidelity is folded",
            &[(RECORDS_KEY, "Vec<ActivityLogRecord>")],
            &[(FIDELITY_KEY, "PlaybookRunFidelityResult")],
            |ctx, _params| {
                let records = ctx
                    .get::<Vec<ActivityLogRecord>>(RECORDS_KEY)
                    .ok_or("No records")?;
                let mut out = Context::new();
                out.set(
                    FIDELITY_KEY,
                    fold_playbook_run_fidelity(records, &SeedPlaybookRegistry),
                );
                Ok(out)
            },
        ),
        // ---- LEG B: the value domain ----------------------------------------
        check_def(
            "every governed artifact kind resolves to a playbook identity that is not itself a kind",
            &[(REGISTRY_KEY, "SeedPlaybookRegistry")],
            |_ctx, _params| {
                let reg = SeedPlaybookRegistry;
                let kinds = governed_kinds(&reg);
                if kinds.is_empty() {
                    return Err("the seed registry governs no kinds".to_string());
                }
                let mut bad = Vec::new();
                for kind in &kinds {
                    match reg.playbook_id_for(kind) {
                        None => bad.push(format!("  `{kind}` resolves to no playbook identity")),
                        Some(id) if kinds.contains(&id) => bad.push(format!(
                            "  `{kind}` -> `{id}`, which is ITSELF a governed kind — the two \
                             namespaces have collapsed"
                        )),
                        Some(_) => {}
                    }
                }
                if bad.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} kind(s) break the kind/identity split that makes `artifact_kind` the \
                         right name:\n{}",
                        bad.len(),
                        bad.join("\n")
                    ))
                }
            },
        ),
        check_def(
            "no governed artifact kind is equal to any playbook identity",
            &[(REGISTRY_KEY, "SeedPlaybookRegistry")],
            |_ctx, _params| {
                let reg = SeedPlaybookRegistry;
                let kinds = governed_kinds(&reg);
                let ids = playbook_ids(&reg);
                let overlap: Vec<&String> = kinds.iter().filter(|k| ids.contains(k)).collect();
                if overlap.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "artifact kinds and playbook identities are NOT disjoint: {overlap:?}. \
                         `artifact_kind` names the first; `playbook_kind` would name the second. \
                         If they are the same set the rename target is undecidable."
                    ))
                }
            },
        ),
        check_def(
            "every folded key is a governed artifact kind",
            &[(COUNTS_KEY, "BTreeMap<String, u64>")],
            |ctx, _params| {
                let counts = ctx
                    .get::<BTreeMap<String, u64>>(COUNTS_KEY)
                    .ok_or("No counts")?;
                let kinds = governed_kinds(&SeedPlaybookRegistry);
                if counts.is_empty() {
                    return Err("the fold produced no keys".to_string());
                }
                let stray: Vec<&String> = counts.keys().filter(|k| !kinds.contains(k)).collect();
                if stray.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} folded key(s) are not governed artifact kinds: {:?}. The field holds \
                         what an artifact IS.",
                        stray.len(),
                        stray
                    ))
                }
            },
        ),
        check_def(
            "no folded key is a playbook identity",
            &[(COUNTS_KEY, "BTreeMap<String, u64>")],
            |ctx, _params| {
                let counts = ctx
                    .get::<BTreeMap<String, u64>>(COUNTS_KEY)
                    .ok_or("No counts")?;
                let ids = playbook_ids(&SeedPlaybookRegistry);
                let stray: Vec<&String> = counts.keys().filter(|k| ids.contains(k)).collect();
                if stray.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{:?} is a PLAYBOOK IDENTITY, not an artifact kind. A field holding these \
                         would be `playbook_kind`; this one is not.",
                        stray
                    ))
                }
            },
        ),
        check_def(
            "the entry for artifact kind {string} reports {int} calls",
            &[(ACTIVITY_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let want = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ArtifactActivityResult>(ACTIVITY_KEY)
                    .ok_or("No activity result")?;
                let entry = result
                    .entry_for(kind)
                    .ok_or_else(|| format!("no entry for artifact kind `{kind}`"))?;
                if entry.call_count == want {
                    Ok(())
                } else {
                    Err(format!(
                        "artifact kind `{kind}`: expected {want} calls, got {}",
                        entry.call_count
                    ))
                }
            },
        ),
        check_def(
            "every entry names a governed artifact kind, never a playbook identity",
            &[(ACTIVITY_KEY, "ArtifactActivityResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<ArtifactActivityResult>(ACTIVITY_KEY)
                    .ok_or("No activity result")?;
                let reg = SeedPlaybookRegistry;
                let kinds = governed_kinds(&reg);
                let ids = playbook_ids(&reg);
                let mut bad = Vec::new();
                let mut seen = 0;
                for group in &result.groups {
                    for entry in &group.entries {
                        seen += 1;
                        if ids.contains(&entry.kind) {
                            bad.push(format!("  `{}` is a playbook identity", entry.kind));
                        } else if !kinds.contains(&entry.kind) {
                            bad.push(format!("  `{}` is neither", entry.kind));
                        }
                    }
                }
                if seen == 0 {
                    return Err("the activity result carries no entries".to_string());
                }
                if bad.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{} bad entry name(s):\n{}", bad.len(), bad.join("\n")))
                }
            },
        ),
        check_def(
            "the fidelity result carries {int} run instances",
            &[(FIDELITY_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(FIDELITY_KEY)
                    .ok_or("No fidelity result")?;
                if result.instances.len() == want {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {want} RUN instances, got {}: {:?}. PlaybookRunFidelity measures \
                         EXECUTIONS — two runs of one kind are two rows. A per-DEFINITION measure \
                         would collapse them to one.",
                        result.instances.len(),
                        result
                            .instances
                            .iter()
                            .map(|i| i.instance_id.as_str())
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "both run instances report artifact kind {string}",
            &[(FIDELITY_KEY, "PlaybookRunFidelityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let result = ctx
                    .get::<PlaybookRunFidelityResult>(FIDELITY_KEY)
                    .ok_or("No fidelity result")?;
                let wrong: Vec<&str> = result
                    .instances
                    .iter()
                    .filter(|i| i.kind != kind)
                    .map(|i| i.kind.as_str())
                    .collect();
                if wrong.is_empty() {
                    Ok(())
                } else {
                    Err(format!("instances carry non-`{kind}` kinds: {wrong:?}"))
                }
            },
        ),
        check_def(
            "the kind {string} resolves the aliases {string}",
            &[],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let want: Vec<&str> = params
                    .get_string(1)
                    .ok_or("Expected aliases")?
                    .split(',')
                    .map(str::trim)
                    .collect();
                let got = playbook_generation_aliases(kind);
                if got == want.as_slice() {
                    Ok(())
                } else {
                    Err(format!(
                        "`{kind}` resolves {got:?}, expected {want:?}. The legacy read alias is the \
                         SEMANTIC content of the `workflow_generation` -> `playbook_generation` \
                         rename: both names must denote one playbook, in a stable order."
                    ))
                }
            },
        ),
        // ---- LEG A': the pin list is forced to GROW -------------------------
        //
        // Round-5's rule, applied to this seam: a pin list a new member is not
        // forced into is a pin list about a subset. The FIELDS above are forced
        // by exhaustive destructuring, but Rust gives no equivalent construct for
        // "every type this phase renamed" — a `vec![...]` of types compiles
        // whatever the phase did. So the forcing function is the classification
        // artifact: if B-i.2c or any later task renames a sixth type, the
        // artifact gains a `rename` target and this check reds until the pin list
        // is updated to match.
        check_def(
            "the pinned members are exactly the classification artifact's rename targets",
            &[],
            |_ctx, _params| {
                let path = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
                    .parent()
                    .expect("workspace root")
                    .join("scripts/bi2-site-classification.tsv");
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
                let mut targets: Vec<&str> = text
                    .lines()
                    // Skip the provenance header comments AND the column header.
                    // `.skip(1)` was right when the file began with its columns;
                    // round 7 added a `#` banner and this read the banner as data.
                    .filter(|l| !l.starts_with('#') && !l.starts_with("site\t"))
                    .filter_map(|l| {
                        let f: Vec<&str> = l.split('\t').collect();
                        (f.len() == 7 && f[5] == "rename").then_some(f[6])
                    })
                    .collect();
                targets.sort();
                targets.dedup();
                let pinned = renamed_family_members();
                if targets == pinned {
                    return Ok(());
                }
                let extra: Vec<&&str> = targets.iter().filter(|t| !pinned.contains(t)).collect();
                let gone: Vec<&&str> = pinned.iter().filter(|p| !targets.contains(p)).collect();
                Err(format!(
                    "the seam pins {pinned:?} but B-i.2's classification renames to {targets:?}: \
                     {} unpinned ({:?}), {} pinned-but-not-renamed ({:?}). A member this seam does \
                     not bind by name is a member the `playbook_kind` mutation could move freely.",
                    extra.len(),
                    extra,
                    gone.len(),
                    gone
                ))
            },
        ),
        // ---- LEG A: the compile-pin -----------------------------------------
        check_def(
            "the renamed family members are exactly {string}",
            &[],
            |_ctx, params| {
                let want: Vec<&str> = params
                    .get_string(0)
                    .ok_or("Expected members")?
                    .split(',')
                    .map(str::trim)
                    .collect();
                let got = renamed_family_members();
                if got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "renamed family members are {got:?}, pinned as {want:?}. Each member is \
                         bound by NAME in `renamed_family_members()` — a field by exhaustive \
                         destructuring, a type by `type_name` — so a retarget (e.g. \
                         `artifact_kind` -> `playbook_kind`) cannot compile past it."
                    ))
                }
            },
        ),
    ]
}
