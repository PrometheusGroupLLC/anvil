//! Loader for playbook `machine.yaml` files.
//!
//! `load_from_yaml` is a pure function — it takes YAML text and a list of
//! available hook filenames, returns a `PlaybookMachine` or a named error.
//! No I/O; the caller supplies the file contents and directory listing.
//!
//! Validation order per plan.md Phase 1:
//! 1. Parse YAML structure → `PlaybookYamlParseError` on failure.
//! 2. Required top-level keys present → `PlaybookMissingRequiredKey`.
//! 3. Type enforcement (handled by serde; `#[serde(deny_unknown_fields)]`
//!    rejects unknown keys as parse errors).
//! 4. Cross-reference checks (roles, states, hooks) → specific named errors.
//! 5. Review-gate invariant → `PlaybookReviewGateMissingSatisfaction`.
//!
//! `validate` extracts steps 4+5 so Phase 3 seed-validity features can call
//! it directly on compiled-in seeds (DRY + feature-testable).

use super::event_driven::EventDrivenMachine;
use super::load_error::PlaybookLoadError;
use super::types::PlaybookMachine;
// C-d.1 round 6, M-1. There is deliberately NO path import here. Moving
// `list_hook_filenames` out to `fs_probe` removed this module's last filesystem
// access, and with it the last reason to name a path type — so `loader` now has
// no `Path` in scope AT ALL. A bool-returning filesystem predicate is not banned
// here; it does not compile here, because there is nothing to call it on. That
// is what the module header has claimed since it was written.

/// Independently selectable loader-side authoring gates.
///
/// The pure loader reads no environment. Binary and registry boundaries choose
/// which checks are active and pass that decision explicitly, allowing the
/// measurement-definition and evidence-obligation rollout flags to remain
/// independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoaderEnforcement {
    /// Require success criteria on measured pairs and an outcome predicate.
    pub measurement_definition: bool,
    /// Require obligations on DRIVEN measured pairs and forbid them on FREE
    /// measured pairs.
    pub evidence_obligation: bool,
}

impl LoaderEnforcement {
    /// Preserve the default loader's pre-enforcement behavior.
    pub const OFF: Self = Self {
        measurement_definition: false,
        evidence_obligation: false,
    };

    /// Preserve the existing measurement-enforcing loader's exact behavior.
    pub const MEASUREMENT: Self = Self {
        measurement_definition: true,
        evidence_obligation: false,
    };
}

// C-d.1 round 6, M-1. `list_hook_filenames` USED TO LIVE HERE, and it was the
// only filesystem access in a module whose header declares itself pure. It
// carried the round-3 signature (`let Ok(..) = read_dir(..) else { Vec::new() }`)
// and the round-4 signature (a per-entry `if !path.is_file() { return None }`)
// verbatim, one frame below a module the lint scans and outside the lint's
// two-file subject list. An unreadable `hooks/` therefore answered "this
// artifact has no hook files", and the loader rejected the machine for naming a
// hook that IS on disk.
//
// It is now `fs_probe::list_hook_files`, which answers `io::Result<Vec<String>>`.
// The fix is that THE I/O LEFT THIS MODULE: `loader` performs no filesystem
// access at all now, which is what its header always claimed, so there is
// nothing here for a bool-returning predicate to be written on. That is a
// property of the module's contents, not of a list of banned spellings.
//
// It remains the SINGLE canonical hook-listing filter shared by every load site
// — registry construction (`HearthPlaybookRegistry::hook_file_names`), the
// persist-preflight duplicate re-load (`persist_playbook`), and the write-
// boundary race re-load (`fs_artifact_adapter::compare_existing_machine`) — and
// all three must agree byte-for-byte on which hooks a machine resolves against,
// because a divergence lets one site accept a machine another rejects.

/// Parse a `machine.yaml` file and validate its cross-references.
///
/// # Parameters
/// - `artifact_id`: The artifact's identity, carried on any error.
/// - `yaml_text`: Full text content of the `machine.yaml` file.
/// - `hook_file_names`: Filenames present under the artifact's `hooks/`
///   directory. Pass an empty slice when the directory doesn't exist or is
///   empty.
///
/// # Errors
/// Returns a `PlaybookLoadError` on the first validation failure encountered
/// for this artifact. One artifact's failure does not affect other artifacts.
pub fn load_from_yaml(
    artifact_id: &str,
    yaml_text: &str,
    hook_file_names: &[String],
) -> Result<PlaybookMachine, PlaybookLoadError> {
    load_from_yaml_with(
        artifact_id,
        yaml_text,
        hook_file_names,
        LoaderEnforcement::OFF,
    )
}

/// Measurement-enforcing variant of [`load_from_yaml`].
///
/// Identical parse + cross-reference pipeline, but additionally runs the
/// loader-side DEFINE block ([`validate_measurement_definition`]) after the
/// existing `validate_with_id` checks succeed: every state carrying a
/// `measurement_by_role` entry must have a non-empty `success_criteria`, and
/// the machine must declare a non-null `outcome_predicate` with a non-blank
/// `terminal_state`. States with NO `measurement_by_role` entries are exempt.
///
/// This is deliberately a SEPARATE entry point rather than a mode of
/// `load_from_yaml` — mirroring `generate`/`generate_enforcing`. The dark-gate
/// decision (default OFF) lives at the engine/binary boundary: this pure
/// library reads no environment. The ENGINE reads
/// `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` and calls this instead of
/// `load_from_yaml` at its registry-construction load site when the flag is
/// on. Flip on only after the hearth fleet is backfilled with per-step
/// `success_criteria` + `outcome_predicate`s (see
/// `loader_measurement_enforcement.feature`) — before backfill, enforcing here
/// would drop the whole unbackfilled corpus out of the registry.
pub fn load_from_yaml_enforcing(
    artifact_id: &str,
    yaml_text: &str,
    hook_file_names: &[String],
) -> Result<PlaybookMachine, PlaybookLoadError> {
    load_from_yaml_with(
        artifact_id,
        yaml_text,
        hook_file_names,
        LoaderEnforcement::MEASUREMENT,
    )
}

/// Parse and validate a playbook with explicitly selected authoring gates.
///
/// Base cross-reference validation always runs first. Measurement-definition
/// validation then runs when selected, followed by evidence-obligation
/// validation when selected. Existing public wrappers reduce to the two legacy
/// configurations exactly: [`LoaderEnforcement::OFF`] and
/// [`LoaderEnforcement::MEASUREMENT`].
pub fn load_from_yaml_with(
    artifact_id: &str,
    yaml_text: &str,
    hook_file_names: &[String],
    enforcement: LoaderEnforcement,
) -> Result<PlaybookMachine, PlaybookLoadError> {
    load_from_yaml_impl(artifact_id, yaml_text, hook_file_names, enforcement)
}

fn load_from_yaml_impl(
    artifact_id: &str,
    yaml_text: &str,
    hook_file_names: &[String],
    enforcement: LoaderEnforcement,
) -> Result<PlaybookMachine, PlaybookLoadError> {
    // Step 1: YAML parse.
    // serde(deny_unknown_fields) on all structs ensures unknown keys surface
    // as parse errors rather than being silently dropped (R7.3 enforcement).
    // serde_yaml surfaces missing required fields as a "missing field `X`"
    // parse error — we reclassify those as MissingRequiredKey so the named
    // error codes from spec R5.2 are accurate.
    let machine: PlaybookMachine = match serde_yaml::from_str::<PlaybookMachine>(yaml_text) {
        Ok(machine) => machine,
        Err(strict_err) => {
            // The standard transition-graph schema rejected this file. Some
            // legitimately-registered machines (#34) use the EVENT/STEP/QUEUE-
            // DRIVEN schema instead: top-level `anvil_kind`/`trigger`/`steps`,
            // per-state `on:` maps, `terminal:` flags, and no standard
            // `transitions:` graph. Those carry keys the strict struct's
            // `deny_unknown_fields` rejects. When the file declares the
            // event-driven schema, parse it through the lenient
            // `EventDrivenMachine` mirror (which does NOT deny unknown fields)
            // and convert it into a `PlaybookMachine`. This keeps the standard
            // schema's strictness fully intact (a non-event-driven file with a
            // genuinely-unknown key still fails) while letting the event-driven
            // variant load + register instead of becoming an invalid artifact.
            if is_event_driven_schema(yaml_text) {
                parse_event_driven(artifact_id, yaml_text)?
            } else {
                return Err(reclassify_parse_error(artifact_id, &strict_err));
            }
        }
    };

    // Step 2: Required top-level keys.
    // serde already enforces non-Option fields are present; the only field
    // that needs a separate check is `kind` being non-empty (serde gives it
    // a default of "" if somehow deserialized as empty — guard defensively).
    // The struct definition already makes missing required keys a parse error,
    // so this step is covered by serde itself for most keys.
    //
    // However `required_fields` has `#[serde(default)]` (allowed to be
    // absent from YAML; defaults to empty vec). Per plan, it IS required as
    // a list even if empty — so an absent `required_fields` key should be
    // accepted (empty list), not rejected. The plan's "required as a list
    // even if empty" means the field is serialized but may be `[]`.
    // We do NOT check for required_fields presence — the default handles it.

    // Steps 4+5 plus independently selected authoring gates.
    validate_with_id_with(&machine, artifact_id, hook_file_names, enforcement)?;

    Ok(machine)
}

/// Validate cross-references and review-gate invariants on a parsed machine.
///
/// This is a pure function over `&PlaybookMachine` so Phase 3 seed-validity
/// features can call it directly on `track_seed()` and `playbook_seed()`.
///
/// Uses `machine.kind` as the `artifact_id` carried on any returned error.
/// Phase 3 seed callers therefore receive errors whose `artifact_id` equals
/// the seed's kind field (e.g., `"track"` or `"playbook"`). If a different id
/// is needed, call `validate_with_id` directly.
///
/// `hook_file_names` is the listing under `hooks/`; pass `&[]` for seeds
/// that have no hook references.
pub fn validate(
    machine: &PlaybookMachine,
    hook_file_names: &[String],
) -> Result<(), PlaybookLoadError> {
    validate_with_id(machine, &machine.kind, hook_file_names)
}

/// Measurement-enforcing variant of [`validate_with_id`].
///
/// Runs the existing cross-reference + review-gate checks FIRST, then the
/// loader-side DEFINE block ([`validate_measurement_definition`]). Composable
/// entry point for callers that already hold a parsed `PlaybookMachine`
/// (mirrors [`load_from_yaml_enforcing`], which threads YAML text through the
/// same two checks). Dark-gated the same way: this pure library reads no
/// environment — the engine decides whether to call this or `validate_with_id`
/// based on `ANVIL_ENFORCE_MEASUREMENT_DEFINITION`.
pub fn validate_with_id_enforcing(
    machine: &PlaybookMachine,
    artifact_id: &str,
    hook_file_names: &[String],
) -> Result<(), PlaybookLoadError> {
    validate_with_id_with(
        machine,
        artifact_id,
        hook_file_names,
        LoaderEnforcement::MEASUREMENT,
    )
}

/// Validate an already-parsed machine with explicitly selected authoring gates.
///
/// This is the in-memory counterpart to [`load_from_yaml_with`]. The order is
/// stable and load-bearing: base validation, then measurement-definition, then
/// evidence-obligation.
pub fn validate_with_id_with(
    machine: &PlaybookMachine,
    artifact_id: &str,
    hook_file_names: &[String],
    enforcement: LoaderEnforcement,
) -> Result<(), PlaybookLoadError> {
    validate_with_id(machine, artifact_id, hook_file_names)?;
    if enforcement.measurement_definition {
        validate_measurement_definition(machine, artifact_id)?;
    }
    if enforcement.evidence_obligation {
        validate_evidence_obligation(machine, artifact_id)?;
    }
    Ok(())
}

pub fn validate_with_id(
    machine: &PlaybookMachine,
    artifact_id: &str,
    hook_file_names: &[String],
) -> Result<(), PlaybookLoadError> {
    let declared_states: std::collections::HashSet<&str> =
        machine.states.iter().map(|s| s.name.as_str()).collect();
    let declared_roles: std::collections::HashSet<&str> =
        machine.roles.iter().map(|r| r.as_str()).collect();

    // Check state hooks (role-agnostic + per-role).
    for state in &machine.states {
        if let Some(hook) = &state.hook {
            let context = format!("state:{}", state.name);
            validate_hook_file(artifact_id, &context, hook, hook_file_names)?;
        }

        // Check per-role hooks. Role key validation runs before filename checks
        // so an unknown-role error takes precedence over a filename error.
        for (role, hook) in &state.hooks_by_role {
            let role_context = format!("state:{}:role:{}", state.name, role);
            if !declared_roles.contains(role.as_str()) {
                return Err(PlaybookLoadError::UnknownRoleKeyReference {
                    artifact_id: artifact_id.to_string(),
                    context: role_context,
                    role: role.clone(),
                });
            }
            validate_hook_file(artifact_id, &role_context, hook, hook_file_names)?;
        }
    }

    // Check transitions.
    for transition in &machine.transitions {
        // Role reference check.
        if !declared_roles.contains(transition.required_role.as_str()) {
            return Err(PlaybookLoadError::UnknownRoleReference {
                artifact_id: artifact_id.to_string(),
                from_state: transition.from_state.clone(),
                to_state: transition.to_state.clone(),
                role: transition.required_role.clone(),
            });
        }

        // from_state reference check.
        if !declared_states.contains(transition.from_state.as_str()) {
            return Err(PlaybookLoadError::UnknownStateReference {
                artifact_id: artifact_id.to_string(),
                from_state: transition.from_state.clone(),
                to_state: transition.to_state.clone(),
                unknown_state: transition.from_state.clone(),
            });
        }

        // to_state reference check.
        if !declared_states.contains(transition.to_state.as_str()) {
            return Err(PlaybookLoadError::UnknownStateReference {
                artifact_id: artifact_id.to_string(),
                from_state: transition.from_state.clone(),
                to_state: transition.to_state.clone(),
                unknown_state: transition.to_state.clone(),
            });
        }

        // Transition hook reference check.
        if let Some(hook) = &transition.hook {
            let context = format!(
                "transition:{}→{}",
                transition.from_state, transition.to_state
            );
            validate_hook_file(artifact_id, &context, hook, hook_file_names)?;
        }
    }

    // Success-rubric validity: every declared dimension must be in the shared
    // quality vocabulary (so cross-playbook scores stay comparable) and carry a
    // well-formed (strictly-positive) weight. A machine without a rubric is
    // unaffected (the field is optional).
    if let Some(rubric) = &machine.success_rubric {
        for entry in &rubric.dimensions {
            if !super::types::is_valid_quality_dimension(&entry.dimension) {
                return Err(PlaybookLoadError::UnknownQualityDimension {
                    artifact_id: artifact_id.to_string(),
                    dimension: entry.dimension.clone(),
                });
            }
            if entry.weight == 0 {
                return Err(PlaybookLoadError::InvalidRubricWeight {
                    artifact_id: artifact_id.to_string(),
                    dimension: entry.dimension.clone(),
                });
            }
        }
    }

    // Outcome-predicate validity: the declared `terminal_state` must be one of
    // the machine's states (the predicate's checkable fact — "this state was
    // reached" — is only meaningful for a state the machine actually has). A
    // machine without an outcome_predicate is unaffected (the field is optional).
    if let Some(predicate) = &machine.outcome_predicate {
        if !declared_states.contains(predicate.terminal_state.as_str()) {
            return Err(PlaybookLoadError::OutcomePredicateUnknownState {
                artifact_id: artifact_id.to_string(),
                terminal_state: predicate.terminal_state.clone(),
            });
        }
    }

    // Review-gate invariant: every outgoing transition from an is_review_gate
    // state must have non-null required_satisfaction.
    for state in &machine.states {
        if !state.is_review_gate {
            continue;
        }
        for transition in &machine.transitions {
            if transition.from_state != state.name {
                continue;
            }
            if transition.required_satisfaction.is_none() {
                return Err(PlaybookLoadError::ReviewGateMissingSatisfaction {
                    artifact_id: artifact_id.to_string(),
                    state: state.name.clone(),
                    to_state: transition.to_state.clone(),
                });
            }
        }
    }

    Ok(())
}

/// The loader-side measurement DEFINE block — the anti-Goodhart gate applied
/// at LOAD time rather than at generation time. Every hearth machine, however
/// it was authored (generator, hand-written, legacy), passes through here when
/// enforcement is on.
///
/// Checks:
/// (a) Every state carrying a `measurement_by_role` entry must have a
///     non-empty `success_criteria` on that `MeasurementSpec`. A state with NO
///     `measurement_by_role` entries at all is EXEMPT — automated/routing
///     machines legitimately have no per-step measurement (there is no doer
///     step to hold a reviewer or judge accountable to a criterion).
/// (b) The machine must declare a non-null `outcome_predicate` with a
///     non-blank `terminal_state` — the checkable FACT ("did the world-change
///     happen") distinct from success_criteria's HOW WELL.
///
/// Deliberately SEPARATE from `validate_with_id` (called only by the
/// `_enforcing` entry points) so the dark-gate composes the same way
/// `generate`/`generate_enforcing` does: the default load path never runs
/// this, and turning it on is a single flag flip at the engine boundary, not a
/// code change here.
fn validate_measurement_definition(
    machine: &PlaybookMachine,
    artifact_id: &str,
) -> Result<(), PlaybookLoadError> {
    for state in &machine.states {
        for (role, spec) in &state.measurement_by_role {
            let criterion = spec.success_criteria.as_deref().unwrap_or("").trim();
            if criterion.is_empty() {
                return Err(PlaybookLoadError::MeasurementDefinitionMissing {
                    artifact_id: artifact_id.to_string(),
                    detail: format!(
                        "state '{}' role '{}' has no success_criteria",
                        state.name, role
                    ),
                });
            }
        }
    }

    let terminal_state = machine
        .outcome_predicate
        .as_ref()
        .map(|predicate| predicate.terminal_state.trim())
        .unwrap_or("");
    if terminal_state.is_empty() {
        return Err(PlaybookLoadError::MeasurementDefinitionMissing {
            artifact_id: artifact_id.to_string(),
            detail: "machine has no outcome_predicate with a non-empty terminal_state"
                .to_string(),
        });
    }

    Ok(())
}

/// The loader-side evidence-obligation authoring gate.
///
/// Every measured `(state, role)` pair in a DRIVEN machine must declare at
/// least one required evidence class. FREE machines have the inverse contract:
/// their measurements are downstream-only, so they must not declare an
/// obligation. States carrying no `measurement_by_role` entries never enter
/// the loop and are therefore exempt.
///
/// Public so the engine can run it directly on a freshly-generated machine at
/// the intake and generation-terminal-persist seams (T-EEC-1 P4), mirroring how
/// those seams also honor the measurement dark-gate. This pure function reads no
/// environment — the engine decides whether to call it from
/// `ANVIL_ENFORCE_EVIDENCE_OBLIGATION`.
pub fn validate_evidence_obligation(
    machine: &PlaybookMachine,
    artifact_id: &str,
) -> Result<(), PlaybookLoadError> {
    for state in &machine.states {
        for (role, spec) in &state.measurement_by_role {
            if machine.is_driven() && spec.evidence_obligation.is_empty() {
                return Err(PlaybookLoadError::EvidenceObligationMissing {
                    artifact_id: artifact_id.to_string(),
                    detail: format!(
                        "kind '{}' state '{}' role '{}' has no evidence_obligation",
                        machine.kind, state.name, role
                    ),
                });
            }

            if !machine.is_driven() && !spec.evidence_obligation.is_empty() {
                return Err(PlaybookLoadError::EvidenceObligationOnFreeRegister {
                    artifact_id: artifact_id.to_string(),
                    detail: format!(
                        "kind '{}' is registered FREE but declares evidence_obligation on state '{}' role '{}'",
                        machine.kind, state.name, role
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Validate the three graph contiguity invariants over the machine's states +
/// transitions. Runs at load/registration so a non-flowing machine is rejected.
///
/// Kept SEPARATE from `validate_with_id` (which enforces per-key cross-reference
/// integrity) so the two concerns compose independently: the per-key loader
/// path proves structural parsing, this graph pass proves the machine can flow.
/// Registration seams (`hearth_registry`, `persist_playbook`, and the engine's
/// load path) run BOTH; a machine that parses but can't flow never registers.
///
/// Like `validate_with_id`, this assumes cross-references are already sound
/// (every transition's from/to names a declared state); call it after
/// `validate_with_id` so dangling edges are reported as reference errors first.
///
/// The initial state is the FIRST declared state — the same first-state
/// convention the begin handler uses to pick the creation target
/// (`begin.rs`: `machine.states.first()`).
///
/// 1. **Reachability**: every declared state is reachable from the initial
///    state via forward transitions. An unreachable state is an error.
/// 2. **Forward-progress**: every NON-terminal state has \u{2265}1 outgoing
///    transition. A non-terminal dead-end is an error.
/// 3. **Terminal-reachability**: from every state, at least one `is_terminal`
///    state is reachable. A trap (can't ever complete) is an error.
///
/// A machine with zero states is left to the existing required-key checks; this
/// function no-ops when there is no initial state to anchor the scan.
pub fn validate_contiguity(
    machine: &PlaybookMachine,
    artifact_id: &str,
) -> Result<(), PlaybookLoadError> {
    // Exemption: a machine that declares NO standard transition edges is not
    // driven by the begin/complete transition lifecycle. These are
    // event/step/queue-driven playbooks (e.g. `import_transaction_history`):
    // they carry states + per-state event maps but an empty `transitions:`
    // list, and are validated + driven by their own mechanism. Running the
    // standard reachability / forward-progress / terminal-reachability checks
    // on them would wrongly see every non-terminal state as a dead-end and
    // reject the whole playbook. The exemption is decided purely from the
    // parsed machine (transitions empty) — never from a playbook name.
    if machine.transitions.is_empty() {
        return Ok(());
    }

    let initial_state = match machine.states.first() {
        Some(state) => state.name.as_str(),
        None => return Ok(()),
    };

    // Adjacency: from_state -> [to_state...].
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for transition in &machine.transitions {
        adjacency
            .entry(transition.from_state.as_str())
            .or_default()
            .push(transition.to_state.as_str());
    }

    // 1. Reachability from the initial state (forward BFS). Terminal states are
    // EXEMPT: real machines legitimately declare out-of-band terminal escape
    // hatches (e.g. `abandoned`, `superseded`) that are entered by an external
    // action rather than a declared forward transition. An unreachable
    // NON-terminal state, however, is genuinely broken — nothing can ever enter
    // it and the playbook can't use it.
    let reachable = forward_reachable(initial_state, &adjacency);
    for state in &machine.states {
        if state.is_terminal {
            continue;
        }
        if !reachable.contains(state.name.as_str()) {
            return Err(PlaybookLoadError::UnreachableState {
                artifact_id: artifact_id.to_string(),
                initial_state: initial_state.to_string(),
                unreachable_state: state.name.clone(),
            });
        }
    }

    // 2. Forward-progress: a non-terminal state must have an outgoing edge.
    for state in &machine.states {
        if state.is_terminal {
            continue;
        }
        let has_outgoing = adjacency
            .get(state.name.as_str())
            .map(|edges| !edges.is_empty())
            .unwrap_or(false);
        if !has_outgoing {
            return Err(PlaybookLoadError::DeadEndState {
                artifact_id: artifact_id.to_string(),
                state: state.name.clone(),
            });
        }
    }

    // 3. Terminal-reachability: from every state the playbook must be able to
    // settle — i.e. reach a place from which no NEW progress is possible. A
    // "settling point" is either:
    //   (a) an `is_terminal` state, or
    //   (b) a stable sink: a strongly-connected component (SCC) with no edges
    //       leaving it. Real machines model their natural end as a non-terminal
    //       state that loops (e.g. track's `completed` \u{2194} `amend` \u{2194}
    //       `amend_review` round-trip, with the formal terminals reachable only
    //       out-of-band). Such a closed loop is a legitimate settling point.
    //
    // A state fails ONLY when it can forward-reach NO settling point at all —
    // the genuine "trap that can never complete". Combined with the
    // forward-progress check above (no non-terminal dead-ends), this rejects a
    // machine that can't flow to completion without flagging the deliberate
    // non-terminal-sink pattern this codebase relies on.
    let settling = settling_states(machine, &adjacency);
    for state in &machine.states {
        let reachable_from_here = forward_reachable(state.name.as_str(), &adjacency);
        let can_settle = reachable_from_here.iter().any(|s| settling.contains(s));
        if !can_settle {
            return Err(PlaybookLoadError::NoTerminalReachable {
                artifact_id: artifact_id.to_string(),
                state: state.name.clone(),
            });
        }
    }

    Ok(())
}

/// Compute the set of "settling" states: every `is_terminal` state, plus every
/// state that belongs to a sink strongly-connected component (an SCC with no
/// outgoing edge to a different SCC). Reaching any settling state means the
/// playbook can come to rest.
fn settling_states<'a>(
    machine: &'a PlaybookMachine,
    adjacency: &std::collections::HashMap<&'a str, Vec<&'a str>>,
) -> std::collections::HashSet<&'a str> {
    let mut settling: std::collections::HashSet<&str> = machine
        .states
        .iter()
        .filter(|s| s.is_terminal)
        .map(|s| s.name.as_str())
        .collect();

    // A state is in a sink-SCC iff every state forward-reachable from it can
    // also reach it back (the reachable set is mutually reachable) — i.e. the
    // closure of its successors never escapes its own SCC. We detect this
    // directly: `s` is in a sink-SCC when, for every `t` reachable from `s`,
    // `s` is reachable from `t` too.
    for state in &machine.states {
        let name = state.name.as_str();
        let forward = forward_reachable(name, adjacency);
        let is_sink_scc = forward.iter().all(|&t| {
            // `s` must be reachable back from every forward-reachable `t`.
            forward_reachable(t, adjacency).contains(name)
        });
        if is_sink_scc {
            settling.insert(name);
        }
    }
    settling
}

/// Forward BFS from `start`; returns the set of states reachable via outgoing
/// transitions (inclusive of `start`).
fn forward_reachable<'a>(
    start: &'a str,
    adjacency: &std::collections::HashMap<&'a str, Vec<&'a str>>,
) -> std::collections::HashSet<&'a str> {
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(current) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(current) {
            for &next in neighbors {
                if visited.insert(next) {
                    queue.push_back(next);
                }
            }
        }
    }
    visited
}

/// Validate both hook filename safety and file existence in one call.
///
/// Runs `validate_hook_filename` first (path-safety), then checks the file is
/// present in `hook_file_names` (existence). Both state-level and role-level
/// hooks flow through this helper so the two checks are applied consistently;
/// the context string distinguishes the error site (e.g. `"state:spec"` vs
/// `"state:spec:role:doer"` vs `"transition:spec→spec_review"`).
fn validate_hook_file(
    artifact_id: &str,
    context: &str,
    filename: &str,
    hook_file_names: &[String],
) -> Result<(), PlaybookLoadError> {
    validate_hook_filename(artifact_id, context, filename)?;
    if !hook_file_names.iter().any(|f| f == filename) {
        return Err(PlaybookLoadError::UnknownHookReference {
            artifact_id: artifact_id.to_string(),
            context: context.to_string(),
            filename: filename.to_string(),
        });
    }
    Ok(())
}

fn validate_hook_filename(
    artifact_id: &str,
    context: &str,
    filename: &str,
) -> Result<(), PlaybookLoadError> {
    let is_direct_markdown = !filename.is_empty()
        && filename.ends_with(".md")
        && !filename.contains('/')
        && !filename.contains('\\')
        && filename != ".md"
        && filename != "."
        && filename != "..";

    if is_direct_markdown {
        Ok(())
    } else {
        Err(PlaybookLoadError::HookPathInvalid {
            artifact_id: artifact_id.to_string(),
            context: context.to_string(),
            filename: filename.to_string(),
        })
    }
}

/// Map a `serde_yaml` parse error into the named `PlaybookLoadError` variant:
/// a "missing field `X`" error becomes `MissingRequiredKey`; everything else
/// becomes `YamlParseError` carrying the location + message.
fn reclassify_parse_error(artifact_id: &str, e: &serde_yaml::Error) -> PlaybookLoadError {
    let msg = e.to_string();
    if let Some(field_name) = extract_missing_field(&msg) {
        return PlaybookLoadError::MissingRequiredKey {
            artifact_id: artifact_id.to_string(),
            key_path: field_name.to_string(),
        };
    }
    let (line, column) = e
        .location()
        .map(|loc| (loc.line() as u64, loc.column() as u64))
        .unwrap_or((0, 0));
    PlaybookLoadError::YamlParseError {
        artifact_id: artifact_id.to_string(),
        line,
        column,
        message: msg,
    }
}

/// Whether `yaml_text` declares the event/step/queue-driven machine schema
/// (#34). The discriminators are event-driven-only markers the standard
/// transition-graph schema never carries:
/// - a top-level `anvil_kind`, `trigger`, or `steps` key, OR
/// - any state declaring an `on:` event map (per-state event transitions).
///
/// The decision is made purely from the parsed document (never a playbook name).
fn is_event_driven_schema(yaml_text: &str) -> bool {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml_text) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let Some(map) = value.as_mapping() else {
        return false;
    };
    let has_top_level_marker = ["anvil_kind", "trigger", "steps"]
        .iter()
        .any(|key| map.contains_key(serde_yaml::Value::from(*key)));
    if has_top_level_marker {
        return true;
    }
    // Any state with an `on:` event map is the event-driven shape.
    map.get(serde_yaml::Value::from("states"))
        .and_then(|states| states.as_sequence())
        .map(|states| {
            states.iter().any(|s| {
                s.as_mapping()
                    .map(|m| m.contains_key(serde_yaml::Value::from("on")))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Parse a file known to declare the event-driven schema through the lenient
/// `EventDrivenMachine` mirror and convert it into a `PlaybookMachine`.
///
/// `kind` is required even on this path: a file declaring neither `kind` nor
/// `anvil_kind` reports `MissingRequiredKey { key_path: "kind" }`, preserving
/// the standard schema's required-key guarantee for the identity field.
fn parse_event_driven(
    artifact_id: &str,
    yaml_text: &str,
) -> Result<PlaybookMachine, PlaybookLoadError> {
    let event: EventDrivenMachine =
        serde_yaml::from_str(yaml_text).map_err(|e| reclassify_parse_error(artifact_id, &e))?;
    event
        .into_machine()
        .ok_or_else(|| PlaybookLoadError::MissingRequiredKey {
            artifact_id: artifact_id.to_string(),
            key_path: "kind".to_string(),
        })
}

/// Extract the field name from serde_yaml's "missing field `X`" error message.
/// Returns `Some("X")` if the pattern matches, `None` otherwise.
fn extract_missing_field(msg: &str) -> Option<&str> {
    // serde_yaml emits: "missing field `kind`" or
    // "missing field `kind` at line N column M"
    let prefix = "missing field `";
    let start = msg.find(prefix)?;
    let rest = &msg[start + prefix.len()..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}
