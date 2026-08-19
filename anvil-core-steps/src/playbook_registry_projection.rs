//! Step definitions for `anvil-core/features/playbook_registry_projection.feature`.
//!
//! C9's seam. Every scenario builds its own `TempDir` hearth — **the live hearth
//! is never touched** — and drives the production functions in
//! `anvil_core::domain::playbook::registry_projection` plus the production
//! `HearthPlaybookRegistry` loader. There is no reimplementation here of any rule
//! under test; a hand-rolled "list the directory" helper would test this file.
//!
//! ## The oracle, and why it changed
//!
//! The projection used to be asserted against `loaded_set(&hearth)` — a helper
//! in THIS file that called the same production loader the projection is built
//! from. Expected and actual moved together, so the check could never disagree
//! about which ids loaded. An independent review made the production loader
//! `continue` past every definition whose basename contains `playbook` and the
//! feature stayed **7/7 green**; a second mutation discarding the basename
//! entirely also stayed green.
//!
//! The expected ids are now **literals in the Gherkin**, and the assertions are
//! `lists exactly` over the projection's ACTIVE section rather than
//! `body.contains(..)` over the whole document — a dropped definition is demoted
//! into the "present on disk but not loaded" section, where a whole-body
//! substring test still matched it. An oracle a mutation can move with is not an
//! oracle.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anvil_test_support::ScratchDir;

use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry_projection::{self as proj};

const HEARTH_KEY: &str = "prp_hearth";
const HANDLE_KEY: &str = "prp_handle";
const PREIMAGE_KEY: &str = "prp_preimage";
const PROJECTION_KEY: &str = "prp_projection";
const OUTCOME_KEY: &str = "prp_outcome";

const CARRIED: &[(&str, &str)] = &[(HEARTH_KEY, "string"), (HANDLE_KEY, "handle")];
const CARRIED_PRE: &[(&str, &str)] = &[
    (HEARTH_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (PREIMAGE_KEY, "string"),
];
const CARRIED_PRE_OUT: &[(&str, &str)] = &[
    (HEARTH_KEY, "string"),
    (HANDLE_KEY, "handle"),
    (PREIMAGE_KEY, "string"),
    (OUTCOME_KEY, "string"),
];

/// A machine the production loader actually accepts, derived from a REAL one.
///
/// The first version of this was hand-synthesized and the loader resolved NONE
/// of it — the scenario's own anti-vacuity guard caught that ("the loader
/// resolved NO definition; a projection over an empty loaded set is vacuously
/// correct and proves nothing"). A fixture the production loader rejects would
/// have made every projection assertion below pass over an empty set.
///
/// So the shape is taken from `playbooks/spark_lifecycle/machine.yaml`, with
/// only the `kind`/`directory`/`registry` swapped per fixture — real shape,
/// per `green-suites-blind-to-runtime`.
fn machine_yaml(kind: &str) -> String {
    const REAL: &str = r#"kind: spark
register: free
projection_only: true
route:
  triggers: []
directory: sparks
registry: sparks.md
parent_kind: ~
description: "Projection-only capture of an out-of-band idea."
required_fields:
  - name: name
    field_type: string
    description: Spark body or idea text
roles:
  - doer
states:
  - name: captured
    role_filters: [doer_actionable]
    registry_section: captured
    projection_targets: [sparks.md]
    is_review_gate: false
    # Spark is fire-and-forget: `captured` is its single, final resting state
    # with no follow-on transition, so it is terminal (the contiguity gate
    # rejects a non-terminal state that has no outgoing transition).
    is_terminal: true
    hooks_by_role:
      doer: capture.md
    measurement_by_role:
      doer:
        intent: Capture the spark as an out-of-band idea without creating a lifecycle artifact.
        expected_output: An appended spark event in sparks/sparks.md and an updated sparks projection.
        success_criteria: A single spark entry is appended to `sparks/sparks.md` carrying the idea text verbatim, with zero lifecycle artifact (no proposal, track, or decision) created and zero edits to any other artifact's files — capture stays fire-and-forget.
transitions: []
outcome_predicate:
  terminal_state: captured
  check: the spark reached captured as an appended sparks.md entry with no lifecycle artifact spawned
"#;
    REAL.replace("kind: spark\n", &format!("kind: {kind}\n"))
        .replace("directory: sparks", &format!("directory: {kind}s"))
        .replace("registry: sparks.md", &format!("registry: {kind}s.md"))
}

/// Each seeded definition gets a distinct governed kind derived from its id, so
/// a projection that confuses id with kind is observable.
fn kind_for(id: &str) -> String {
    format!(
        "c9_{}",
        id.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    )
}

fn seed_definition(dir: &Path, id: &str) -> Result<(), String> {
    let d = dir.join(id);
    std::fs::create_dir_all(&d).map_err(|e| format!("create {}: {e}", d.display()))?;
    std::fs::write(d.join("machine.yaml"), machine_yaml(&kind_for(id)))
        .map_err(|e| format!("write machine: {e}"))?;
    std::fs::write(
        d.join("status.yaml"),
        "version: 1\nkind: playbook\nstate: active\n",
    )
    .map_err(|e| format!("write status: {e}"))?;
    // The real machine references `capture.md` from `hooks_by_role`, and the
    // loader REJECTS a machine whose hook file is missing
    // (`playbook_unknown_hook_reference`). Seeding it is what makes the fixture
    // loadable — and finding that out was the anti-vacuity guard's doing, not a
    // guess: it reported the loader's own error rather than passing over an
    // empty set.
    let hooks = d.join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|e| format!("create hooks: {e}"))?;
    std::fs::write(hooks.join("capture.md"), "# capture\n")
        .map_err(|e| format!("write hook: {e}"))?;
    Ok(())
}

/// Recursive byte census of a subtree: relative path -> bytes.
fn census(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(base: &Path, cur: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(cur) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(b) = std::fs::read(&p) {
                let rel = p.strip_prefix(base).unwrap_or(&p).display().to_string();
                out.insert(rel, b);
            }
        }
    }
    walk(root, root, &mut out);
    out
}

fn carry(ctx: &mut Context) -> Result<(Context, PathBuf), String> {
    let h = ctx.require::<String>(HEARTH_KEY)?.clone();
    let handle = ctx.take::<Arc<ScratchDir>>(HANDLE_KEY);
    let mut out = Context::new();
    out.set(HEARTH_KEY, h.clone());
    if let Some(x) = handle {
        out.set(HANDLE_KEY, x);
    }
    Ok((out, PathBuf::from(h)))
}

fn carry_pre(ctx: &mut Context) -> Result<(Context, PathBuf), String> {
    let pre = ctx.require::<String>(PREIMAGE_KEY)?.clone();
    let (mut out, hearth) = carry(ctx)?;
    out.set(PREIMAGE_KEY, pre);
    Ok((out, hearth))
}

fn hearth_of(ctx: &Context) -> Result<PathBuf, String> {
    Ok(PathBuf::from(
        ctx.get::<String>(HEARTH_KEY).ok_or("no hearth")?,
    ))
}

fn declared_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// The projection's two sections, split on the exclusions header.
///
/// SCOPED DELIBERATELY. The old assertions ran `body.contains(id)` over the
/// whole document. A definition the loader dropped is demoted into the
/// exclusions section — where a whole-body substring still matched it — so
/// "the basename survived into the projection" was satisfied by the definition
/// being EXCLUDED. Every assertion below names which section it means.
const EXCLUSIONS_HEADER: &str = "## present on disk but not loaded";

fn sections(body: &str) -> (&str, &str) {
    match body.split_once(EXCLUSIONS_HEADER) {
        Some((active, excluded)) => (active, excluded),
        None => (body, ""),
    }
}

/// Ids listed in a rendered section, read back from the rendered lines rather
/// than from any value the projection builder was handed.
fn ids_in_active(active: &str) -> Vec<String> {
    active
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("- [")?;
            let (id, _) = rest.split_once(']')?;
            Some(id.to_string())
        })
        .collect()
}

fn ids_in_excluded(excluded: &str) -> Vec<String> {
    excluded
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("- `")?;
            let (id, _) = rest.split_once('`')?;
            Some(id.to_string())
        })
        .collect()
}

fn assert_exactly(actual: &[String], declared: &[String], what: &str) -> Result<(), String> {
    let mut a = actual.to_vec();
    a.sort();
    let mut d = declared.to_vec();
    d.sort();
    if a == d {
        return Ok(());
    }
    let missing: Vec<&String> = d.iter().filter(|x| !a.contains(x)).collect();
    let extra: Vec<&String> = a.iter().filter(|x| !d.contains(x)).collect();
    Err(format!(
        "the projection's {what} section does not list exactly what this feature declares.\n  \
         declared: {d:?}\n  rendered: {a:?}\n  missing (the loader did not resolve, or the \
         projection renamed, something this feature says it must list): {missing:?}\n  \
         unexpected: {extra:?}"
    ))
}

/// A partially-migrated hearth whose legacy definitions have NO canonical
/// namesake — the state a refusal keyed to NAME COLLISION cannot see.
///
/// [`seed_both_roots`] always seeds `20260303T0000_shared_identity` under both
/// roots, so every both-present scenario written against it carries a name
/// collision and none of them can tell "refuse on collision" apart from "refuse
/// on shadowing". This one has zero overlap: Foundry wrote new definitions into
/// `playbooks/` while older ones sit under `workflows/` under different ids.
/// Those are exactly as lost as a colliding pair, and losing them is what the
/// refusal exists to announce.
fn seed_shadow_only_roots(hearth: &Path) -> Result<(), String> {
    let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
    let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
    std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
    seed_definition(&legacy, "20260528T2321_workflow_generation")?;
    seed_definition(&legacy, "20260505T0000_legacy_only_beta")?;
    seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
    Ok(())
}

/// A legacy root alongside canonical that holds NOTHING THE LOADER COULD HAVE
/// RESOLVED: an empty leftover subdirectory, a hidden directory, and a stray
/// file. Zero definitions are at risk, so by the declared rule — the refusal is
/// keyed to what is LOST — this must not refuse.
fn seed_legacy_root_without_definitions(hearth: &Path) -> Result<(), String> {
    let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
    let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
    std::fs::create_dir_all(legacy.join("20260707T0000_empty_shell"))
        .map_err(|e| format!("mkdir: {e}"))?;
    std::fs::create_dir_all(legacy.join(".cache")).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(legacy.join("README.md"), "left over\n").map_err(|e| format!("write: {e}"))?;
    std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
    seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
    Ok(())
}

fn seed_both_roots(hearth: &Path) -> Result<(), String> {
    let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
    let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
    std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
    // Legacy-only, canonical-only, and one identity under BOTH.
    seed_definition(&legacy, "20260528T2321_workflow_generation")?;
    seed_definition(&legacy, "20260303T0000_shared_identity")?;
    seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
    seed_definition(&canonical, "20260303T0000_shared_identity")?;
    Ok(())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── project ──────────────────────────────────────────────────────
        step_def(
            "a temporary hearth seeded with definitions {string} and unloadable directories {string}",
            &[],
            CARRIED,
            |_ctx, params| {
                let loadable = declared_list(params.get_string(0).ok_or("expected definitions")?);
                let unloadable = declared_list(params.get_string(1).ok_or("expected directories")?);
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let pb = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&pb).map_err(|e| format!("mkdir: {e}"))?;
                for id in &loadable {
                    seed_definition(&pb, id)?;
                }
                for id in &unloadable {
                    // Present on disk, NOT loadable: no machine.yaml.
                    std::fs::create_dir_all(pb.join(id)).map_err(|e| format!("mkdir: {e}"))?;
                }
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                Ok(out)
            },
        ),
        step_def(
            "the definition {string} is made unloadable",
            CARRIED,
            CARRIED,
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let (out, hearth) = carry(&mut ctx)?;
                let machine = hearth
                    .join(proj::CANONICAL_HEARTH_DIR)
                    .join(&id)
                    .join("machine.yaml");
                if !machine.is_file() {
                    return Err(format!("{id:?} was not seeded as a loadable definition"));
                }
                std::fs::write(&machine, "this: is: not: valid: yaml: [\n")
                    .map_err(|e| format!("corrupt machine: {e}"))?;
                Ok(out)
            },
        ),
        step_def(
            "the production registry renders the playbooks projection",
            CARRIED,
            &[
                (HEARTH_KEY, "string"),
                (HANDLE_KEY, "handle"),
                (PROJECTION_KEY, "string"),
            ],
            |mut ctx, _params| {
                let (mut out, hearth) = carry(&mut ctx)?;
                // The PRODUCTION registry renders it. `build_projection` had no
                // production caller at all — it was reachable only from this
                // file, so scenarios proved a library worked and not that the
                // system did.
                let reg = HearthPlaybookRegistry::new(hearth.clone());
                let body = reg.projection();
                if reg.loaded_definitions().is_empty() {
                    let why: Vec<String> =
                        reg.invalid_artifacts().iter().map(|e| e.to_string()).collect();
                    return Err(format!(
                        "the loader resolved NO definition; a projection over an empty loaded \
                         set is vacuously correct and proves nothing. Loader errors: {why:?}"
                    ));
                }
                out.set(PROJECTION_KEY, body);
                Ok(out)
            },
        ),
        check_def(
            "the projection's active section lists exactly {string}",
            &[(PROJECTION_KEY, "string")],
            |ctx, params| {
                let declared = declared_list(params.get_string(0).ok_or("expected ids")?);
                let body = ctx.get::<String>(PROJECTION_KEY).ok_or("no projection")?;
                let (active, _) = sections(body);
                assert_exactly(&ids_in_active(active), &declared, "active")
            },
        ),
        check_def(
            "the projection's exclusions section lists exactly {string}",
            &[(PROJECTION_KEY, "string")],
            |ctx, params| {
                let declared = declared_list(params.get_string(0).ok_or("expected ids")?);
                let body = ctx.get::<String>(PROJECTION_KEY).ok_or("no projection")?;
                let (_, excluded) = sections(body);
                assert_exactly(&ids_in_excluded(excluded), &declared, "exclusions")
            },
        ),
        check_def(
            "the projection states the exclusion reason for {string}",
            &[(PROJECTION_KEY, "string")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let body = ctx.get::<String>(PROJECTION_KEY).ok_or("no projection")?;
                let (_, excluded) = sections(body);
                let line = excluded
                    .lines()
                    .find(|l| l.contains(&id))
                    .ok_or_else(|| {
                        format!(
                            "{id:?} is not named in the exclusions section at all; a silent \
                             omission is how an operator concludes it was never there"
                        )
                    })?;
                let (_, reason) = line.split_once("— excluded: ").ok_or_else(|| {
                    format!("the projection names {id:?} but states no exclusion reason: {line:?}")
                })?;
                if reason.trim().is_empty() {
                    return Err(format!("the exclusion reason for {id:?} is blank"));
                }
                Ok(())
            },
        ),
        check_def(
            "the projection's active section carries a governed kind for every id it lists",
            &[(HEARTH_KEY, "string"), (PROJECTION_KEY, "string")],
            |ctx, _params| {
                let body = ctx.get::<String>(PROJECTION_KEY).ok_or("no projection")?;
                let (active, _) = sections(body);
                for id in ids_in_active(active) {
                    // FIXTURE-DERIVED, not independent — and the earlier comment
                    // claiming independence was the overreach, not the check.
                    // Deriving the expectation from the id is exactly what makes
                    // it non-independent of any implementation that also derives
                    // kind from id. What it does catch, and all it claims to
                    // catch: a projection that prints SOME kind somewhere rather
                    // than pairing each basename with the kind that basename's
                    // own machine declares.
                    let want = kind_for(&id);
                    let line = active
                        .lines()
                        .find(|l| l.contains(&format!("- [{id}]")))
                        .ok_or_else(|| format!("no active line for {id:?}"))?;
                    if !line.contains(&format!("governed kind `{want}`")) {
                        return Err(format!(
                            "the active line for {id:?} does not carry its governed kind \
                             {want:?}: {line:?}"
                        ));
                    }
                }
                Ok(())
            },
        ),
        // ── retire ───────────────────────────────────────────────────────
        step_def(
            "a temporary hearth carrying a legacy workflows registry file",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let body = "# Playbooks\n\n## active\n\n- [alpha](playbooks/alpha/)\n";
                std::fs::write(hearth.join(proj::LEGACY_REGISTRY_FILE), body)
                    .map_err(|e| format!("write legacy registry: {e}"))?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, body.to_string());
                Ok(out)
            },
        ),
        step_def(
            "retirement is attempted with the receipt entry {string}",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, params| {
                let entry = params.get_string(0).ok_or("expected receipt entry")?.to_string();
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let receipt = vec![entry];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        step_def(
            "retirement is attempted with a receipt entry naming ANOTHER hearth's file of the same name",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                // A REAL second hearth, with a real file of the same name in it,
                // so the receipt is genuine evidence — about somebody else's
                // file. Basename equality accepted this and deleted THIS hearth's
                // registry; the gate that exists to prevent deletion-on-mismatched
                // -evidence was doing exactly that, one directory level up.
                let other = ScratchDir::new()?;
                let other_file = other.path().join(proj::LEGACY_REGISTRY_FILE);
                std::fs::write(&other_file, "# a DIFFERENT hearth's registry\n")
                    .map_err(|e| format!("write other: {e}"))?;
                let receipt = vec![format!("retired: {}", other_file.display())];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                if !other_file.exists() {
                    return Err(
                        "the retirement deleted the OTHER hearth's file — the gate acted outside \
                         the hearth it was asked about"
                            .to_string(),
                    );
                }
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        step_def(
            "retirement is attempted with a receipt entry naming the file by its full path",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let receipt = vec![format!(
                    "retired: {}",
                    hearth.join(proj::LEGACY_REGISTRY_FILE).display()
                )];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        check_def(
            "the retirement is refused",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("retirement_not_receipted") {
                    return Err(format!(
                        "expected a typed retirement_not_receipted refusal, got {o:?}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names what the receipt actually said",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("backup_of_workflows.md.bak") {
                    return Err(format!(
                        "the refusal does not say what the receipt named, so an operator cannot \
                         tell a near miss from a missing receipt: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy registry file is byte-identical to its preimage",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                let now = std::fs::read_to_string(hearth.join(proj::LEGACY_REGISTRY_FILE))
                    .map_err(|e| format!("the refused retirement removed the file anyway: {e}"))?;
                if &now != pre {
                    return Err("the refused retirement modified the file".to_string());
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy registry file is gone",
            &[(HEARTH_KEY, "string"), (OUTCOME_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o != "ok" {
                    return Err(format!("the receipted retirement was refused: {o}"));
                }
                if hearth.join(proj::LEGACY_REGISTRY_FILE).exists() {
                    return Err("the receipted retirement left the file in place".to_string());
                }
                Ok(())
            },
        ),
        // ── generations ──────────────────────────────────────────────────
        step_def(
            "a temporary hearth whose generations live only under the legacy directory",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let legacy = hearth.join(proj::LEGACY_GENERATIONS_DIR);
                for id in ["20260528T2321_workflow_generation", "20260601T0000_gen_beta"] {
                    std::fs::create_dir_all(legacy.join(id))
                        .map_err(|e| format!("mkdir: {e}"))?;
                    std::fs::write(legacy.join(id).join("status.yaml"), "version: 1\n")
                        .map_err(|e| format!("write: {e}"))?;
                }
                let pre = format!("{:?}", census(&legacy));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth with one identity present under both generations directories",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let id = "20260528T2321_workflow_generation";
                for root in [proj::LEGACY_GENERATIONS_DIR, proj::CANONICAL_GENERATIONS_DIR] {
                    let d = hearth.join(root).join(id);
                    std::fs::create_dir_all(&d).map_err(|e| format!("mkdir: {e}"))?;
                    std::fs::write(d.join("status.yaml"), format!("version: 1\nfrom: {root}\n"))
                        .map_err(|e| format!("write: {e}"))?;
                }
                let pre = format!("{:?}", census(&hearth));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "the generations are resolved",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let outcome = match proj::resolve_generations(&hearth) {
                    Ok(map) => format!("ok:{}", map.keys().cloned().collect::<Vec<_>>().join(",")),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        check_def(
            "every legacy generation is resolved",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let Some(ids) = o.strip_prefix("ok:") else {
                    return Err(format!("resolution failed: {o}"));
                };
                for want in ["20260528T2321_workflow_generation", "20260601T0000_gen_beta"] {
                    if !ids.split(',').any(|i| i == want) {
                        return Err(format!(
                            "dual read did not resolve legacy generation {want:?}; resolved: {ids}"
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy generations directory is byte-identical to its preimage",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                let now = format!("{:?}", census(&hearth.join(proj::LEGACY_GENERATIONS_DIR)));
                if &now != pre {
                    return Err(
                        "the legacy generations directory changed during a READ — dual-read must \
                         not migrate anything"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        check_def(
            "a new generation is written under the canonical generations directory",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let target = proj::canonical_generations_dir(&hearth).join("20260701T0000_gen_new");
                std::fs::create_dir_all(&target).map_err(|e| format!("write new: {e}"))?;
                std::fs::write(target.join("status.yaml"), "version: 1\n")
                    .map_err(|e| format!("write new: {e}"))?;
                if !target.starts_with(hearth.join(proj::CANONICAL_GENERATIONS_DIR)) {
                    return Err("a new generation was not written to the canonical dir".to_string());
                }
                // And the dual read must now see BOTH generations.
                let map = proj::resolve_generations(&hearth)
                    .map_err(|e| format!("resolution after canonical write failed: {e}"))?;
                if !map.contains_key("20260701T0000_gen_new")
                    || !map.contains_key("20260601T0000_gen_beta")
                {
                    return Err(format!(
                        "after a canonical write the dual read lost a side: {:?}",
                        map.keys().collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the resolution is refused naming both paths",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("generation_identity_collision") {
                    return Err(format!("expected a typed collision refusal, got {o:?}"));
                }
                if !o.contains(proj::LEGACY_GENERATIONS_DIR)
                    || !o.contains(proj::CANONICAL_GENERATIONS_DIR)
                {
                    return Err(format!("the refusal does not name BOTH paths: {o}"));
                }
                Ok(())
            },
        ),
        check_def(
            "an explicit merge decision is required",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("explicit merge decision") {
                    return Err(format!("the refusal does not require a merge decision: {o}"));
                }
                Ok(())
            },
        ),
        check_def(
            "both generations directories are byte-identical to their preimages",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                if &format!("{:?}", census(&hearth)) != pre {
                    return Err(
                        "the refused collision still changed bytes — no merge, no rename, no write"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        // The PRODUCTION lookup path, not the library function. `resolve_generations`
        // had no production caller; the snapshot adapter picked whichever root its
        // constant list named first and never saw a collision at all.
        check_def(
            "the production artifact lookup resolves {string}",
            &[(HEARTH_KEY, "string")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let found = anvil_core_hearth::fs_snapshot_adapter::locate_artifact_dir_for_test(
                    &hearth, &id,
                )
                .map_err(|e| format!("the production lookup could not inspect {id:?}: {e}"))?
                .ok_or_else(|| {
                    format!("the production lookup did not resolve {id:?} across both roots")
                })?;
                if !found.is_dir() {
                    return Err(format!("resolved {id:?} to a non-directory: {found:?}"));
                }
                Ok(())
            },
        ),
        step_def(
            "that hearth also holds an unrelated track {string}",
            CARRIED_PRE,
            CARRIED_PRE,
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let (out, hearth) = carry_pre(&mut ctx)?;
                let d = hearth.join("tracks").join(&id);
                std::fs::create_dir_all(&d).map_err(|e| format!("mkdir: {e}"))?;
                std::fs::write(d.join("status.yaml"), "version: 1\nkind: track\n")
                    .map_err(|e| format!("write: {e}"))?;
                Ok(out)
            },
        ),
        check_def(
            "the production artifact lookup still resolves {string} while that collision stands",
            &[(HEARTH_KEY, "string")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                // ANTI-VACUITY: the control means nothing unless the collision is
                // still standing when it runs. Re-establish it from disk rather
                // than trusting the previous step's ordering.
                let colliding = "20260601T0000_gen_beta";
                let legacy = hearth.join(proj::LEGACY_GENERATIONS_DIR).join(colliding);
                let canonical = hearth.join(proj::CANONICAL_GENERATIONS_DIR).join(colliding);
                if !legacy.is_dir() || !canonical.is_dir() {
                    return Err(format!(
                        "the collision this control is scoped against is not standing: legacy={} \
                         canonical={}",
                        legacy.is_dir(),
                        canonical.is_dir()
                    ));
                }
                let found = anvil_core_hearth::fs_snapshot_adapter::locate_artifact_dir_for_test(
                    &hearth, &id,
                )
                .map_err(|e| format!("the production lookup could not inspect {id:?}: {e}"))?;
                if found.is_none() {
                    return Err(format!(
                        "ONE colliding generation identity took down the bare-id lookup for {id:?}, \
                         an artifact that is not a generation and has nothing to do with either \
                         generations root. The collision is a property of those two directories; \
                         answering `None` for every other artifact in the hearth converts a local \
                         refusal into a hearth-wide outage — and the state that triggers it is the \
                         one this track's own cutover creates."
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "a canonical generation {string} is created",
            &[(HEARTH_KEY, "string")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                let d = proj::canonical_generations_dir(&hearth).join(&id);
                std::fs::create_dir_all(&d).map_err(|e| format!("mkdir: {e}"))?;
                std::fs::write(d.join("status.yaml"), "version: 1\n")
                    .map_err(|e| format!("write: {e}"))?;
                Ok(())
            },
        ),
        check_def(
            "the production artifact lookup refuses {string} once it exists under both roots",
            &[(HEARTH_KEY, "string")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("expected id")?.to_string();
                let hearth = hearth_of(&ctx)?;
                // Create the SAME identity under canonical too.
                let d = proj::canonical_generations_dir(&hearth).join(&id);
                std::fs::create_dir_all(&d).map_err(|e| format!("mkdir: {e}"))?;
                std::fs::write(d.join("status.yaml"), "version: 1\nfrom: canonical\n")
                    .map_err(|e| format!("write: {e}"))?;
                // The refusal is now a TYPED refusal rather than a bare
                // `None`: a caller cannot confuse "these two roots disagree
                // about this identity" with "this artifact is not here".
                let found = anvil_core_hearth::fs_snapshot_adapter::locate_artifact_dir_for_test(
                    &hearth, &id,
                );
                match found {
                    Ok(Some(p)) => Err(format!(
                        "the production lookup silently PICKED a side for an identity present \
                         under both generation roots: {p:?}. A first-match pick resolves, looks \
                         fine, and makes half the history invisible."
                    )),
                    Ok(None) => Err(
                        "the production lookup answered 'not found' for an identity present under \
                         BOTH generation roots. That is the wrong refusal: it is indistinguishable \
                         from an artifact that is not in this hearth, and it hides the fact that \
                         two roots disagree."
                            .to_string(),
                    ),
                    Err(e) => {
                        let text = e.to_string();
                        if !text.contains("generation_identity_collision") {
                            return Err(format!(
                                "the lookup refused, but not as a collision: {text}"
                            ));
                        }
                        Ok(())
                    }
                }
            },
        ),
        // ── move ─────────────────────────────────────────────────────────
        step_def(
            "a temporary hearth whose definitions live under the pre-migration definitions directory",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
                std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&legacy, "20260101T0000_alpha_lifecycle")?;
                seed_definition(&legacy, "20260528T2321_workflow_generation")?;
                let pre = format!("{:?}", census(&legacy));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth carrying definitions under BOTH the legacy and canonical directories",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed_both_roots(&hearth)?;
                let pre = format!("{:?}", census(&hearth));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "the legacy hearth directory is migrated",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let outcome = match proj::migrate_legacy_hearth_dir(&hearth) {
                    Ok(moved) => format!("ok:{moved}"),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        check_def(
            "the canonical directory holds every definition under its exact source basename",
            &[(HEARTH_KEY, "string"), (OUTCOME_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o != "ok:true" {
                    return Err(format!("the move did not complete: {o}"));
                }
                for id in ["20260101T0000_alpha_lifecycle", "20260528T2321_workflow_generation"] {
                    if !hearth
                        .join(proj::CANONICAL_HEARTH_DIR)
                        .join(id)
                        .join("machine.yaml")
                        .is_file()
                    {
                        return Err(format!(
                            "{id:?} did not survive the move under its exact basename — a basename \
                             is not renamed for aesthetics"
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy hearth directory is gone",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                if hearth.join(proj::LEGACY_HEARTH_DIR).exists() {
                    return Err("the legacy hearth directory survived a completed move".to_string());
                }
                Ok(())
            },
        ),
        step_def(
            "the hearth move is set to fail",
            CARRIED_PRE,
            CARRIED_PRE,
            |mut ctx, _params| {
                let (out, _hearth) = carry_pre(&mut ctx)?;
                // The sanctioned injector: it forces the syscall's RESULT at a
                // production boundary. The move still runs through the real
                // function; nothing is replaced.
                std::env::set_var(proj::MOVE_FAILURE_INJECTION_ENV, "c9-seam");
                Ok(out)
            },
        ),
        step_def(
            "the production loader scans that hearth",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let reg = HearthPlaybookRegistry::new(hearth.clone());
                let errs: Vec<String> =
                    reg.invalid_artifacts().iter().map(|e| e.to_string()).collect();
                let blocked = reg
                    .registration_blocked()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                let loaded: Vec<String> =
                    reg.loaded_definitions().into_iter().map(|d| d.id).collect();
                let attribution: Vec<String> = reg
                    .invalid_artifacts()
                    .iter()
                    .map(|e| format!("{}=>{:?}", e.code(), e.artifact_ids()))
                    .collect();
                std::env::remove_var(proj::MOVE_FAILURE_INJECTION_ENV);
                out.set(
                    OUTCOME_KEY,
                    format!(
                        "ERRORS[{}]BLOCKED[{}]LOADED[{}]ATTRIB[{}]",
                        errs.join(" | "),
                        blocked,
                        loaded.join(","),
                        attribution.join(" | ")
                    ),
                );
                Ok(out)
            },
        ),
        check_def(
            "the loader reports a hearth directory move failure naming both paths",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("hearth_directory_move_failed") {
                    return Err(format!(
                        "the loader did NOT report the failed move — this is the warn-and-continue \
                         fallback: it logged and scanned canonical only. Reported: {o:?}"
                    ));
                }
                if !o.contains(proj::LEGACY_HEARTH_DIR) || !o.contains(proj::CANONICAL_HEARTH_DIR) {
                    return Err(format!("the failure does not name both paths: {o}"));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports registration as blocked",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let blocked = o
                    .split_once("BLOCKED[")
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(b, _)| b)
                    .unwrap_or("");
                if blocked.is_empty() {
                    return Err(format!(
                        "the scenario is NAMED for blocking registration and nothing blocks: \
                         `registration_blocked()` returned None. Reporting a failure is not \
                         blocking a writer. Outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no definition was written into the canonical directory",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                if canonical.exists() && census(&canonical).keys().next().is_some() {
                    return Err(
                        "definitions were written into the canonical directory after the move \
                         failed"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy directory still holds every definition under its exact basename",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                if &format!("{:?}", census(&hearth.join(proj::LEGACY_HEARTH_DIR))) != pre {
                    return Err(
                        "the failed move still changed the legacy directory's bytes".to_string()
                    );
                }
                Ok(())
            },
        ),
        // ── both present ─────────────────────────────────────────────────
        check_def(
            "the migration is refused naming both roots",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("hearth_directory_collision") {
                    return Err(format!(
                        "both directories are present and the migration reported {o:?}. Returning \
                         `Ok(false)` here is not 'nothing to do': the scan reads canonical only, \
                         so every legacy definition vanishes with no diagnostic."
                    ));
                }
                if !o.contains(proj::LEGACY_HEARTH_DIR) || !o.contains(proj::CANONICAL_HEARTH_DIR) {
                    return Err(format!("the refusal does not name BOTH roots: {o}"));
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names every definition the legacy root would have shadowed",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                for id in ["20260528T2321_workflow_generation", "20260303T0000_shared_identity"] {
                    if !o.contains(id) {
                        return Err(format!(
                            "the refusal does not name the shadowed definition {id:?}; an operator \
                             cannot act on a count. Refusal: {o}"
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names the identity present under both roots",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let colliding = o
                    .split_once("also exist under the canonical root")
                    .map(|(before, _)| before)
                    .unwrap_or("");
                // The colliding list is rendered immediately before that phrase.
                let tail = colliding.rsplit_once("Of those, ").map(|(_, t)| t).unwrap_or("");
                if !tail.contains("20260303T0000_shared_identity") {
                    return Err(format!(
                        "the refusal does not distinguish the identity present under BOTH roots \
                         from the merely-shadowed ones; that is the subset a merge decision is \
                         actually about. Refusal: {o}"
                    ));
                }
                if tail.contains("20260528T2321_workflow_generation") {
                    return Err(format!(
                        "the refusal lists a legacy-ONLY definition as colliding: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "both hearth directories are byte-identical to their preimages",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                if &format!("{:?}", census(&hearth)) != pre {
                    return Err(
                        "the refused collision still changed bytes — no merge, no rename, no move"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports a hearth directory collision naming both roots",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("hearth_directory_collision") {
                    return Err(format!(
                        "the loader did not report the collision. This is the SILENT DROP: it \
                         scans canonical only and says nothing about the legacy root. \
                         Reported: {o}"
                    ));
                }
                if !o.contains(proj::LEGACY_HEARTH_DIR) || !o.contains(proj::CANONICAL_HEARTH_DIR) {
                    return Err(format!("the collision does not name both roots: {o}"));
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy-only definition is absent from the loaded set",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let loaded = o
                    .split_once("LOADED[")
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(l, _)| l)
                    .unwrap_or("");
                // This is the LOSS the refusal exists to announce, asserted so the
                // refusal cannot be traded for a quiet successful load: the legacy
                // definition really is unreachable, which is exactly why silence
                // was unacceptable.
                if loaded.split(',').any(|id| id == "20260528T2321_workflow_generation") {
                    return Err(format!(
                        "the legacy-only definition loaded after all, so the collision refusal is \
                         describing a loss that did not happen: {o}"
                    ));
                }
                if !loaded.split(',').any(|id| id == "20260101T0000_alpha_lifecycle") {
                    return Err(format!(
                        "the canonical definition did not load, so this scenario is not observing \
                         the shadowing it claims to: {o}"
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "a temporary hearth whose legacy definitions have NO canonical namesake",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed_shadow_only_roots(&hearth)?;
                let pre = format!("{:?}", census(&hearth));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth whose legacy root holds no definition at all",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                seed_legacy_root_without_definitions(&hearth)?;
                let pre = format!("{:?}", census(&hearth));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth whose only fault is a malformed definition",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                // ONE bad definition, and NOTHING wrong at the hearth level: no
                // legacy root, so no move and no collision.
                let bad = canonical.join("20260404T0000_malformed_control");
                std::fs::create_dir_all(&bad).map_err(|e| format!("mkdir: {e}"))?;
                std::fs::write(bad.join("machine.yaml"), "kind: [unclosed\n")
                    .map_err(|e| format!("write: {e}"))?;
                let pre = format!("{:?}", census(&hearth));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        check_def(
            "the refusal names every legacy-only definition it would have shadowed",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                for id in ["20260528T2321_workflow_generation", "20260505T0000_legacy_only_beta"] {
                    if !o.contains(id) {
                        return Err(format!(
                            "the refusal does not name the shadowed definition {id:?}. Neither has \
                             a canonical namesake, so a refusal keyed to a NAME COLLISION never \
                             fires here and both vanish exactly as they did before this rule \
                             existed. Refusal: {o}"
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names no identity as present under both roots",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let before = o
                    .split_once("also exist under the canonical root")
                    .map(|(b, _)| b)
                    .ok_or_else(|| format!("the refusal does not render a colliding set: {o}"))?;
                let tail = before.rsplit_once("Of those, ").map(|(_, t)| t).unwrap_or("");
                if tail.trim() != "[]" {
                    return Err(format!(
                        "this hearth has NO identity under both roots, and the refusal claims \
                         {tail:?} does. Shadowing and collision are different facts and the \
                         diagnostic must not conflate them: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports no hearth directory collision",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o.contains("hearth_directory_collision") {
                    return Err(format!(
                        "the legacy root holds NO definition the loader could have resolved, so \
                         nothing is shadowed and nothing is lost — and the hearth is refused \
                         anyway. That blocks an already-migrated hearth over a leftover empty \
                         directory, which is the outcome the declared boundary exists to prevent: \
                         {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports registration as not blocked",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let blocked = o
                    .split_once("BLOCKED[")
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(b, _)| b)
                    .unwrap_or("");
                if !blocked.is_empty() {
                    return Err(format!(
                        "registration is reported BLOCKED on a hearth with no hearth-level fault: \
                         {blocked:?}. `registration_blocked()` returns only the two hearth-level \
                         variants — a hearth that reports itself unwritable over one bad \
                         machine.yaml, or over a leftover empty directory, blocks every registrar \
                         against a hearth that is fine. Outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the canonical definition still loaded",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let loaded = o
                    .split_once("LOADED[")
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(l, _)| l)
                    .unwrap_or("");
                if !loaded.split(',').any(|id| id == "20260101T0000_alpha_lifecycle") {
                    return Err(format!(
                        "the canonical definition did not load, so 'no collision was reported' is \
                         vacuous — this scenario is not observing a working hearth: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports the malformed definition as invalid",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                // ANTI-VACUITY for the assertion that follows it: "registration is
                // not blocked" is trivially true on a hearth with no errors at all.
                // This one insists the fault is really there and really reported.
                if !o.contains("20260404T0000_malformed_control") {
                    return Err(format!(
                        "the malformed definition was not reported as invalid, so the hearth this \
                         scenario claims to be testing — one whose ONLY fault is a bad \
                         machine.yaml — has no fault at all: {o}"
                    ));
                }
                Ok(())
            },
        ),
        // ── error attribution ────────────────────────────────────────────
        check_def(
            "every hearth directory error attributes to no artifact id",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let attrib = o
                    .split_once("ATTRIB[")
                    .and_then(|(_, rest)| rest.rsplit_once(']'))
                    .map(|(a, _)| a)
                    .unwrap_or("");
                let mut seen = false;
                for entry in attrib.split(" | ") {
                    let Some((code, ids)) = entry.split_once("=>") else { continue };
                    if code.starts_with("hearth_directory_") {
                        seen = true;
                        if ids.trim() != "[]" {
                            return Err(format!(
                                "{code} attributes to {ids} — a hearth DIRECTORY failure belongs to \
                                 no definition; blaming one is how a directory's problem gets read \
                                 as an artifact's."
                            ));
                        }
                    }
                }
                if !seen {
                    return Err(format!(
                        "no hearth-directory error was produced at all, so this assertion is \
                         vacuous: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "a malformed definition error still attributes to its own artifact id",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                // A CONTROL for the assertion above: if `artifact_ids()` returned
                // an empty vec for everything, "attributes to no artifact id"
                // would be unfailable. Seed one malformed definition under the
                // canonical root and require it to be attributed.
                let hearth = hearth_of(&ctx)?;
                let d = hearth
                    .join(proj::CANONICAL_HEARTH_DIR)
                    .join("20260404T0000_malformed_control");
                std::fs::create_dir_all(&d).map_err(|e| format!("mkdir: {e}"))?;
                std::fs::write(d.join("machine.yaml"), "kind: [unclosed\n")
                    .map_err(|e| format!("write: {e}"))?;
                let reg = HearthPlaybookRegistry::new(hearth.clone());
                let attributed = reg.invalid_artifacts().iter().any(|e| {
                    e.artifact_ids()
                        .iter()
                        .any(|id| id == "20260404T0000_malformed_control")
                });
                if !attributed {
                    return Err(
                        "a malformed definition attributed to no artifact id, so the \
                         'attributes to no artifact id' assertion above is unfailable"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 4: the receipt gate's bare-basename residual ──────
        step_def(
            "retirement is attempted with a receipt entry naming the basename beside ANOTHER hearth's path",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                // A REAL second hearth, named in the same entry. The entry names
                // THIS file by bare basename and THAT place by path — which
                // `workflows.md` is it a receipt for? The gate DELETES, so the
                // answer cannot be "assume the local one".
                let other = ScratchDir::new()?;
                let other_file = other.path().join(proj::LEGACY_REGISTRY_FILE);
                std::fs::write(&other_file, "# a DIFFERENT hearth's registry\n")
                    .map_err(|e| format!("write other: {e}"))?;
                let receipt = vec![format!(
                    "retired workflows.md from {}",
                    other.path().display()
                )];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                if !other_file.exists() {
                    return Err(
                        "the retirement deleted the OTHER hearth's file".to_string(),
                    );
                }
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        // ── C-d.1 round 5, M-3: locations that carry no `/` ───────────────
        step_def(
            "retirement is attempted with a receipt entry naming another location as {string}",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, params| {
                let shape = params.get_string(0).ok_or("expected shape")?.to_string();
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                // Each shape names a REAL other place holding a REAL
                // `workflows.md`, exactly as the round-4 scenario does — the
                // only thing that varies is the convention it is written in.
                let other = ScratchDir::new()?;
                let other_file = other.path().join(proj::LEGACY_REGISTRY_FILE);
                std::fs::write(&other_file, "# a DIFFERENT hearth's registry\n")
                    .map_err(|e| format!("write other: {e}"))?;
                let token = match shape.as_str() {
                    // A Windows path and a UNC path are unambiguously
                    // path-shaped tokens naming another hearth, and on Unix
                    // neither contains `MAIN_SEPARATOR`.
                    "windows_path" => r"C:\other\hearth\workflows.md".to_string(),
                    "unc_path" => r"\\server\share\workflows.md".to_string(),
                    // A bare relative directory name, made REAL as a sibling of
                    // this hearth so it names something an operator could have
                    // meant. Both hearths live under one parent for this shape.
                    "sibling_dirname" => {
                        let parent = hearth
                            .parent()
                            .ok_or("the hearth has no parent to hold a sibling")?;
                        let sibling = parent.join("other_hearth_dirname");
                        std::fs::create_dir_all(&sibling)
                            .map_err(|e| format!("mkdir sibling: {e}"))?;
                        std::fs::write(
                            sibling.join(proj::LEGACY_REGISTRY_FILE),
                            "# a DIFFERENT hearth's registry\n",
                        )
                        .map_err(|e| format!("write sibling: {e}"))?;
                        "other_hearth_dirname".to_string()
                    }
                    // C-d.1 round 6, L-1. A percent-encoded path is a real
                    // convention for writing a path, and it carries NONE of the
                    // four literal markers the shape test looks for — no `/`,
                    // no `\`, no `~` prefix, no `://` — and it does not resolve
                    // to a real directory beside this hearth either. So it was
                    // invisible to the ambiguity test and this DELETING gate
                    // accepted the bare basename standing beside it. The
                    // encoded target is the OTHER hearth's real file.
                    "percent_encoded" => other_file
                        .display()
                        .to_string()
                        .replace('/', "%2F")
                        .replace('\\', "%5C"),
                    other => return Err(format!("unknown location shape {other:?}")),
                };
                let receipt = vec![format!("retired: workflows.md from {token}")];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                if !other_file.exists() {
                    return Err("the retirement deleted the OTHER hearth's file".to_string());
                }
                // The sibling lives beside the hearth's own `ScratchDir`, so it
                // is the one thing in this module not cleaned up by a handle.
                // Removed here rather than left for a sweeper.
                if shape == "sibling_dirname" {
                    if let Some(parent) = hearth.parent() {
                        let sibling = parent.join("other_hearth_dirname");
                        let survived = sibling.join(proj::LEGACY_REGISTRY_FILE).exists();
                        let _ = std::fs::remove_dir_all(&sibling);
                        if !survived {
                            return Err(
                                "the retirement deleted the SIBLING hearth's file".to_string()
                            );
                        }
                    }
                }
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        // ── C-d.1 round 5, L-3: two safe-direction false negatives ────────
        step_def(
            "retirement is attempted with a markdown link to this hearth's own file",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let receipt = vec![format!(
                    "retired [workflows.md]({}/{})",
                    hearth.display(),
                    proj::LEGACY_REGISTRY_FILE
                )];
                let outcome = match proj::retire_legacy_registry(&hearth, &receipt) {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        // ── C-d.1 round 4: roots that cannot be ENUMERATED ────────────────
        step_def(
            "a temporary hearth whose legacy root holds a definition and cannot be enumerated",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
                std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
                // A REAL definition under the legacy root — one the loader would
                // have resolved, so something genuinely IS lost.
                seed_definition(&legacy, "20260505T0000_legacy_only_beta")?;
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                // Traversable, NOT readable: `exists()` and `is_dir()` still
                // answer true, `read_dir` fails. This is the cheapest way to
                // construct the state; the general trigger is any read_dir
                // failure (protected locations, stale mounts, fd exhaustion).
                set_mode(&legacy, 0o300)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, "unreadable-legacy-root".to_string());
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth whose canonical root cannot be enumerated",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                set_mode(&canonical, 0o300)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, "unreadable-canonical-root".to_string());
                Ok(out)
            },
        ),
        check_def(
            "the migration is refused because a root could not be enumerated",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("hearth_root_unreadable") {
                    return Err(format!(
                        "an unenumerable legacy root holding a REAL definition did not refuse. \
                         `read_dir` failing was swallowed into an empty listing, so the guard \
                         concluded nothing would be lost and the definition vanished with no \
                         rename, no error and no log line — the silent fallback inside the guard \
                         whose stated purpose is to fail loud. Outcome: {o:?}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names the unreadable root",
            &[(OUTCOME_KEY, "string"), (HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let named = o.contains(
                    &hearth.join(proj::LEGACY_HEARTH_DIR).display().to_string(),
                );
                if !named {
                    return Err(format!(
                        "the refusal does not name the root it could not enumerate, so an operator \
                         cannot tell WHICH root to fix: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader reports an unreadable hearth root",
            &[(OUTCOME_KEY, "string"), (HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let reported = o.contains("hearth_root_unreadable")
                    // The refusal must name the root an operator has to fix, not
                    // just its code.
                    && o.contains(&hearth.join(proj::CANONICAL_HEARTH_DIR).display().to_string());
                if !reported {
                    return Err(format!(
                        "a canonical root that EXISTS and cannot be enumerated produced an EMPTY \
                         REGISTRY with no diagnostic — indistinguishable from a hearth that \
                         genuinely holds nothing, and a writer was cleared to write into it. \
                         Outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the legacy hearth directory still exists",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let present = hearth.join(proj::LEGACY_HEARTH_DIR).exists();
                if !present {
                    return Err(
                        "the refused operation moved or removed the legacy root anyway".to_string(),
                    );
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 4: self-shadow is not shadowing ───────────────────
        step_def(
            "a temporary hearth whose legacy root is a symlink to the canonical root",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                // The compatibility shim an operator leaves behind after a
                // migration: `workflows` -> `playbooks`. Both "exist"; there is
                // exactly ONE directory, so nothing can be shadowed.
                #[cfg(unix)]
                std::os::unix::fs::symlink(&canonical, hearth.join(proj::LEGACY_HEARTH_DIR))
                    .map_err(|e| format!("symlink: {e}"))?;
                let pre = format!("{:?}", census(&canonical));
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, pre);
                Ok(out)
            },
        ),
        check_def(
            "the migration reports nothing to move",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o != "ok:false" {
                    return Err(format!(
                        "two paths to ONE directory were treated as two roots: the refusal names \
                         the canonical root's OWN definitions as shadowed and — since the refusal \
                         is now wired to the persist WRITER — hard-refuses every write into a \
                         perfectly healthy hearth. Outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the canonical hearth directory still holds its definition",
            &[(HEARTH_KEY, "string"), (PREIMAGE_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let pre = ctx.get::<String>(PREIMAGE_KEY).ok_or("no preimage")?;
                // ANTI-VACUITY FIRST (L-1). "unchanged" over an EMPTY census is
                // trivially true, so this asserts the census is non-empty before
                // it asserts it is unchanged — the state it goes red in is a
                // fixture that seeded nothing, which is the one way an
                // unchanged-bytes claim can be worthless.
                let post = census(&hearth.join(proj::CANONICAL_HEARTH_DIR));
                if post.is_empty() {
                    return Err(
                        "the canonical root holds NO files, so 'still holds its definition' is \
                         vacuously true and this scenario proves nothing about self-shadowing"
                            .to_string(),
                    );
                }
                if &format!("{post:?}") != pre {
                    return Err(
                        "the canonical root's bytes changed — the 'nothing to move' answer was not \
                         actually a no-op"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 4: a root that is not a directory ─────────────────
        step_def(
            "a temporary hearth whose legacy root is a plain file",
            &[],
            CARRIED_PRE,
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                std::fs::write(hearth.join(proj::LEGACY_HEARTH_DIR), "not a directory\n")
                    .map_err(|e| format!("write: {e}"))?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, "legacy-root-is-a-file".to_string());
                Ok(out)
            },
        ),
        check_def(
            "the migration is refused because a root is not a directory",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains("hearth_root_not_a_directory") {
                    return Err(format!(
                        "a plain FILE at the legacy root was reported as a completed migration: it \
                         was RENAMED to the canonical root, leaving the scan a file where a \
                         directory belongs — whose read_dir failure was then swallowed into an \
                         empty registry. Outcome: {o:?}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the canonical hearth directory was not created",
            &[(HEARTH_KEY, "string")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                if hearth.join(proj::CANONICAL_HEARTH_DIR).exists() {
                    return Err(
                        "the refused operation created the canonical root anyway — a guard that \
                         mutates while refusing is not a guard"
                            .to_string(),
                    );
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 4: the guard is pure ──────────────────────────────
        step_def(
            "the hearth root guard is asked",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let outcome = match proj::hearth_root_diagnosis(&hearth) {
                    Ok(proj::HearthRootState::NothingToMove) => "nothing_to_move".to_string(),
                    Ok(proj::HearthRootState::MovePending { .. }) => "move_pending".to_string(),
                    Err(e) => e.to_string(),
                };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        check_def(
            "the guard reports a pending move",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o != "move_pending" {
                    return Err(format!(
                        "the guard did not identify the pending move, so this scenario's \
                         'and nothing moved' assertions are vacuous: {o}"
                    ));
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 5, HIGH-1: the per-ENTRY metadata swallow ─────────
        //
        // Round 4 closed `read_dir`. `Path::is_dir()` / `is_file()` / `exists()`
        // return `bool` and map EVERY error to `false`, and three of them sat
        // BEHIND the fixed `read_dir`. The three fixtures below are the
        // reviewer's own reproductions, each parameterised on the mode so a
        // single failure mode can no longer stand in for the class — and each
        // carrying its own readable CONTROL mode in the Examples table, so a
        // fixture that seeds nothing is caught rather than passing vacuously.
        step_def(
            "a temporary hearth whose canonical root holds two definitions at mode {string}",
            &[],
            CARRIED_PRE,
            |_ctx, params| {
                let mode = parse_mode(params.get_string(0).ok_or("expected mode")?)?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                // TWO real definitions the loader resolves at the control modes,
                // so "loaded nothing" is a LOSS and not an empty hearth.
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                seed_definition(&canonical, "20260505T0000_legacy_only_beta")?;
                // 0600 / 0400: READABLE, NOT TRAVERSABLE. `read_dir` SUCCEEDS
                // and hands back both entries; every `stat` on those entries
                // then fails with EACCES. This is also the shape a stale network
                // mount takes — `readdir` answers from cache and `stat` returns
                // ESTALE/EIO — which is why the entry level, not the directory
                // level, is where this matters in production.
                set_mode(&canonical, mode)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, format!("canonical-root-mode-{mode:04o}"));
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth whose legacy root holds a shadowed definition at mode {string}",
            &[],
            CARRIED_PRE,
            |_ctx, params| {
                let mode = parse_mode(params.get_string(0).ok_or("expected mode")?)?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
                std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
                // A REAL shadowed definition: the scan reads canonical only, so
                // at the control modes this MUST refuse as a collision. If the
                // legacy root reads as empty, it does not.
                seed_definition(&legacy, "20260505T0000_legacy_only_beta")?;
                set_mode(&legacy, mode)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, format!("legacy-root-mode-{mode:04o}"));
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth whose legacy definition directory is at mode {string}",
            &[],
            CARRIED_PRE,
            |_ctx, params| {
                let mode = parse_mode(params.get_string(0).ok_or("expected mode")?)?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                let legacy = hearth.join(proj::LEGACY_HEARTH_DIR);
                std::fs::create_dir_all(&legacy).map_err(|e| format!("mkdir: {e}"))?;
                seed_definition(&legacy, "20260505T0000_legacy_only_beta")?;
                // ROOT PERFECTLY READABLE, ENTRY UNREADABLE — literally row 3 of
                // round 4's own declared table, and the row the code did not
                // implement. `read_dir` succeeds, the entry `stat`s as a
                // directory, and `{entry}/machine.yaml` is the stat that fails.
                set_mode(&legacy.join("20260505T0000_legacy_only_beta"), mode)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, format!("legacy-definition-dir-mode-{mode:04o}"));
                Ok(out)
            },
        ),
        check_def(
            "the migration answer carries {string}",
            &[(OUTCOME_KEY, "string")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected code")?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if !o.contains(expected) {
                    return Err(format!(
                        "the guard answered {o:?} instead of {expected:?}. On the READABLE modes \
                         this hearth genuinely shadows a definition and MUST refuse as a \
                         collision — that is the control that proves the fixture is real. On the \
                         unreadable modes an error was mapped to `false` by a bool-returning \
                         predicate, the guard concluded nothing would be lost, and the definition \
                         vanished with no rename, no error and no log line."
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader answer carries {string}",
            &[(OUTCOME_KEY, "string")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected code")?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let blocked = o
                    .split_once("BLOCKED[")
                    .and_then(|(_, t)| t.split_once(']'))
                    .map(|(b, _)| b.to_string())
                    .ok_or_else(|| format!("the loader outcome has no BLOCKED section: {o}"))?;
                // An EMPTY declared code is "not blocked", asserted as EMPTINESS
                // rather than as `contains("")` — which is true of every string
                // and would make every control row of the Examples table an
                // assertion that cannot fail.
                if expected.is_empty() {
                    if !blocked.is_empty() {
                        return Err(format!(
                            "this hearth is readable and holds two real definitions, and \
                             registration is blocked anyway with {blocked:?}. The control rows of \
                             the table are what stop the refusing rows from passing over an empty \
                             registry. Full outcome: {o}"
                        ));
                    }
                    return Ok(());
                }
                if !blocked.contains(expected) {
                    return Err(format!(
                        "`registration_blocked()` answered {blocked:?}, not {expected:?}. An \
                         unreadable root that reads as an EMPTY registry is indistinguishable \
                         from a hearth that genuinely holds nothing, and it CLEARS A WRITER. \
                         Full outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the loader resolved the definitions {string}",
            &[(OUTCOME_KEY, "string")],
            |ctx, params| {
                let expected = declared_list(params.get_string(0).ok_or("expected ids")?);
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                let loaded_raw = o
                    .split_once("LOADED[")
                    .and_then(|(_, t)| t.split_once(']'))
                    .map(|(b, _)| b.to_string())
                    .ok_or_else(|| format!("the loader outcome has no LOADED section: {o}"))?;
                let mut loaded = declared_list(&loaded_raw);
                loaded.sort();
                let mut want = expected;
                want.sort();
                if loaded != want {
                    return Err(format!(
                        "the loader resolved {loaded:?}, the scenario declares {want:?}. The \
                         expected ids are LITERALS IN THE GHERKIN so this cannot move with the \
                         implementation. Full outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 6, M-1: the guard path is WIDER than the scan ─────
        //
        // `loader::list_hook_filenames` is called from the SCANNED
        // `hearth_registry::hook_file_names` and is itself unscanned. On
        // unmutated round-5 HEAD it carried the round-3 signature (`let Ok(..)
        // = read_dir(..) else { Vec::new() }`) and the round-4 signature (a
        // per-entry `if !path.is_file() { return None }`) verbatim.
        step_def(
            "a temporary hearth whose definition carries a hooks directory at mode {string}",
            &[],
            CARRIED_PRE,
            |_ctx, params| {
                let mode = parse_mode(params.get_string(0).ok_or("expected mode")?)?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let canonical = hearth.join(proj::CANONICAL_HEARTH_DIR);
                std::fs::create_dir_all(&canonical).map_err(|e| format!("mkdir: {e}"))?;
                // A REAL definition whose machine REFERENCES `capture.md` from
                // `hooks_by_role`, with a REAL `capture.md` on disk beside it.
                // The hook is present in EVERY row — only the ability to LOOK
                // at it varies — which is what makes the refusing rows
                // non-vacuous and the false `playbook_unknown_hook_reference`
                // provably false.
                seed_definition(&canonical, "20260101T0000_alpha_lifecycle")?;
                set_mode(
                    &canonical
                        .join("20260101T0000_alpha_lifecycle")
                        .join("hooks"),
                    mode,
                )?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, format!("hooks-dir-mode-{mode:04o}"));
                Ok(out)
            },
        ),
        check_def(
            "no hook file on disk is reported as an unknown reference",
            &[(OUTCOME_KEY, "string")],
            |ctx, _params| {
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                if o.contains("playbook_unknown_hook_reference") || o.contains("unknown hook") {
                    return Err(format!(
                        "the loader reported a hook file that IS ON DISK as an unknown \
                         reference. The hook listing swallowed a read failure into an EMPTY \
                         listing, so the machine was rejected for naming a hook it names \
                         correctly, the definition was dropped, and `registration_blocked()` \
                         answered None. The operator is sent to fix a machine.yaml that is \
                         right. Full outcome: {o}"
                    ));
                }
                Ok(())
            },
        ),
        // ── C-d.1 round 6, M-1 second reach: the artifact LOOKUP ──────────
        //
        // `fs_snapshot_adapter::locate_artifact_dir` brackets round 5's fixed
        // `resolve_generation_identity` with `literal.exists()` and
        // `candidate.exists()`, both unscanned. All thirteen call sites map the
        // resulting `None` onto `SnapshotError::NotFound` => gRPC NOT_FOUND.
        step_def(
            "a temporary hearth holding a track under a per-kind directory at mode {string}",
            &[],
            CARRIED_PRE,
            |_ctx, params| {
                let mode = parse_mode(params.get_string(0).ok_or("expected mode")?)?;
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                let tracks = hearth.join("tracks");
                let track = tracks.join(PROBE_TRACK_ID);
                std::fs::create_dir_all(&track).map_err(|e| format!("mkdir: {e}"))?;
                // A REAL artifact, on disk, in every row. The controls resolve
                // it; the refusing rows differ only in whether the process may
                // LOOK at the directory holding it.
                std::fs::write(
                    track.join("status.yaml"),
                    "version: 1\nkind: track\nstate: active\n",
                )
                .map_err(|e| format!("write status: {e}"))?;
                set_mode(&tracks, mode)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(PREIMAGE_KEY, format!("per-kind-dir-mode-{mode:04o}"));
                Ok(out)
            },
        ),
        step_def(
            "the production artifact lookup is asked for that track by its full path",
            CARRIED_PRE,
            CARRIED_PRE_OUT,
            |mut ctx, _params| {
                let (mut out, hearth) = carry_pre(&mut ctx)?;
                let asked = format!("tracks/{PROBE_TRACK_ID}");
                let outcome =
                    match anvil_core_hearth::fs_snapshot_adapter::locate_artifact_dir_for_test(
                        &hearth, &asked,
                    ) {
                        Ok(Some(_)) => "resolved".to_string(),
                        Ok(None) => "not_found".to_string(),
                        Err(e) => format!("uninspectable: {e}"),
                    };
                out.set(OUTCOME_KEY, outcome);
                Ok(out)
            },
        ),
        check_def(
            "the artifact lookup answers {string}",
            &[(OUTCOME_KEY, "string")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected answer")?;
                let o = ctx.get::<String>(OUTCOME_KEY).ok_or("no outcome")?;
                match expected {
                    "resolved" => {
                        if o != "resolved" {
                            return Err(format!(
                                "the lookup answered {o:?} for a track that is ON DISK and \
                                 inspectable. This is the CONTROL row: without it the refusing \
                                 rows below could pass over a fixture that seeded nothing."
                            ));
                        }
                        Ok(())
                    }
                    "uninspectable" => {
                        if !o.starts_with("uninspectable") {
                            return Err(format!(
                                "the lookup answered {o:?}. The track's directory EXISTS and \
                                 could not be inspected, and `not_found` is a different fact \
                                 with a different operator action — every call site turns it \
                                 into gRPC NOT_FOUND, telling the operator an artifact that is \
                                 there is not there. 'I could not look' is not 'it is absent'."
                            ));
                        }
                        Ok(())
                    }
                    other => Err(format!(
                        "the scenario declares an answer this check does not know: {other:?}"
                    )),
                }
            },
        ),
    ]
}

/// The track the artifact-lookup fixtures seed. One id, used by the fixture and
/// by the step that asks for it, so the two cannot drift apart.
const PROBE_TRACK_ID: &str = "20260101T0000_probe_track";

/// Set a directory's mode, from a Gherkin-declared octal literal.
///
/// The fail-loud fixtures are PARAMETERISED on the mode, because a single mode
/// is what produced the HIGH finding twice: round 3's probe used `0300`
/// (traversable, not readable) where `read_dir` itself fails, and round 4 fixed
/// exactly that syscall — leaving `0600`/`0400` (readable, NOT traversable),
/// where `read_dir` SUCCEEDS and every subsequent `stat` fails, and `0000` on a
/// definition directory under a perfectly readable root, both wide open.
///
/// Nothing here restores the mode: [`anvil_test_support::ScratchDir`] does that from `Drop`,
/// which is the only place that runs on a RED as well as a green (M-4).
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Err("the fail-loud root fixtures require unix permissions".to_string())
}

/// Parse a Gherkin `"0600"` mode literal. A malformed literal is a REFUSAL, not
/// a default: a fixture that silently ran at mode 0755 would make every
/// unreadable-root scenario a vacuous pass.
fn parse_mode(raw: &str) -> Result<u32, String> {
    u32::from_str_radix(raw.trim().trim_start_matches("0o"), 8)
        .map_err(|e| format!("mode {raw:?} is not octal: {e}"))
}
