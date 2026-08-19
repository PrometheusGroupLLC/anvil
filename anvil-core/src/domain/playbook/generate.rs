//! Pure CandidatePlaybook -> PlaybookMachine generation.

use super::candidate::CandidatePlaybook;
use super::types::{
    EvidenceClass, MeasurementSpec, PlaybookMachine, Register, RouteConfig, StateDefinition,
    TransitionDefinition,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// Generated candidate playbooks use a fixed terminal sentinel. Lore-proposed
// states must not use it or any generated review/revision state name.
const TERMINAL_STATE: &str = "completed";
const OUTCOME_REFLECTION_REVIEW_STATE: &str = "outcome_reflection_review";
const RESERVED_REVIEWER_ROLE: &str = "reviewer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    EmptyProposedStates,
    BlankIntent,
    BlankRouteDescription,
    EmptyRouteTriggers,
    EmptyProjectionTargets,
    BlankProposedStateField { index: usize, field: String },
    ReservedRole { role: String },
    StateNameCollision { name: String },
    VacuousSuccessCriteria { state: String },
    MissingOutcomePredicate,
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenerateError::EmptyProposedStates => write!(f, "empty proposed states"),
            GenerateError::BlankIntent => write!(f, "blank intent"),
            GenerateError::BlankRouteDescription => write!(f, "blank route description"),
            GenerateError::EmptyRouteTriggers => write!(f, "empty route triggers"),
            GenerateError::EmptyProjectionTargets => write!(
                f,
                "projection_targets are required so generated playbooks remain auditable"
            ),
            GenerateError::BlankProposedStateField { index, field } => write!(
                f,
                "blank proposed state field '{}' at index {}",
                field, index
            ),
            GenerateError::ReservedRole { role } => {
                write!(f, "reserved role '{}'", role)
            }
            GenerateError::StateNameCollision { name } => {
                write!(f, "state name collision '{}'", name)
            }
            GenerateError::VacuousSuccessCriteria { state } => write!(
                f,
                "missing or vacuous success_criteria for proposed state '{}': provide a falsifiable/checkable criterion (a numeric threshold, a named error/status code, a file path or extension, a command, a backticked or quoted token, or an explicit zero/negative case) rather than a vague quality judgment",
                state
            ),
            GenerateError::MissingOutcomePredicate => write!(
                f,
                "missing outcome_predicate: a driven playbook must declare an outcome_predicate with a non-blank terminal_state naming the machine state whose reaching is the checkable 'did the world-change happen' fact"
            ),
        }
    }
}

impl std::error::Error for GenerateError {}

pub fn generate(candidate: &CandidatePlaybook) -> Result<PlaybookMachine, GenerateError> {
    if candidate.intent.trim().is_empty() {
        return Err(GenerateError::BlankIntent);
    }
    if candidate.proposed_states.is_empty() {
        return Err(GenerateError::EmptyProposedStates);
    }
    validate_route(candidate)?;
    validate_projection_targets(candidate)?;

    let kind = slug(&candidate.intent);
    if kind.is_empty() {
        return Err(GenerateError::BlankIntent);
    }

    validate_proposed_state_fields(candidate)?;
    validate_roles(candidate)?;
    validate_state_names(candidate)?;

    let mut roles = Vec::new();
    for proposed in &candidate.proposed_states {
        push_unique(&mut roles, proposed.role.clone());
    }
    push_unique(&mut roles, RESERVED_REVIEWER_ROLE.to_string());

    let mut states = Vec::new();
    let mut transitions = Vec::new();

    let mut final_revision_state = String::new();
    for (idx, proposed) in candidate.proposed_states.iter().enumerate() {
        let working_state = proposed.state.clone();
        let review_state = format!("{}_review", proposed.state);
        let revision_state = format!("{}_revision", proposed.state);
        let next_state = candidate
            .proposed_states
            .get(idx + 1)
            .map(|next| next.state.clone())
            .unwrap_or_else(|| OUTCOME_REFLECTION_REVIEW_STATE.to_string());
        final_revision_state = revision_state.clone();

        states.push(StateDefinition {
            name: working_state.clone(),
            registry_section: "active".to_string(),
            projection_targets: candidate.projection_targets.clone(),
            measurement_by_role: measurement_by_role(
                "doer",
                proposed.intent.clone(),
                proposed.expected_output.clone(),
                proposed.success_criteria.clone(),
                proposed.evidence_obligation.clone(),
            ),
            ..empty_state()
        });
        states.push(StateDefinition {
            name: review_state.clone(),
            registry_section: "active".to_string(),
            projection_targets: candidate.projection_targets.clone(),
            is_review_gate: true,
            measurement_by_role: review_measurement_by_role(
                &proposed.state,
                proposed.success_criteria.clone(),
                proposed.evidence_obligation.clone(),
            ),
            ..empty_state()
        });
        states.push(StateDefinition {
            name: revision_state.clone(),
            registry_section: "active".to_string(),
            projection_targets: candidate.projection_targets.clone(),
            measurement_by_role: measurement_by_role(
                "doer",
                format!(
                    "Revise the {} output until it satisfies reviewer findings.",
                    proposed.state
                ),
                format!("An updated {} ready for review.", proposed.expected_output),
                proposed.success_criteria.clone(),
                proposed.evidence_obligation.clone(),
            ),
            ..empty_state()
        });

        transitions.push(TransitionDefinition {
            from_state: working_state,
            to_state: review_state.clone(),
            required_role: proposed.role.clone(),
            required_satisfaction: None,
            requires_approver: false,
            hook: None,
        });
        transitions.push(TransitionDefinition {
            from_state: review_state.clone(),
            to_state: next_state,
            required_role: RESERVED_REVIEWER_ROLE.to_string(),
            required_satisfaction: Some(vec!["satisfied".to_string()]),
            requires_approver: true,
            hook: None,
        });
        transitions.push(TransitionDefinition {
            from_state: review_state.clone(),
            to_state: revision_state.clone(),
            required_role: RESERVED_REVIEWER_ROLE.to_string(),
            required_satisfaction: Some(vec!["needs_revision".to_string()]),
            requires_approver: false,
            hook: None,
        });
        transitions.push(TransitionDefinition {
            from_state: revision_state,
            to_state: review_state,
            required_role: proposed.role.clone(),
            required_satisfaction: None,
            requires_approver: false,
            hook: None,
        });
    }

    states.push(StateDefinition {
        name: OUTCOME_REFLECTION_REVIEW_STATE.to_string(),
        registry_section: "active".to_string(),
        projection_targets: candidate.projection_targets.clone(),
        is_review_gate: true,
        measurement_by_role: outcome_reflection_measurement_by_role(candidate_evidence_obligation(
            candidate,
        )),
        ..empty_state()
    });
    transitions.push(TransitionDefinition {
        from_state: OUTCOME_REFLECTION_REVIEW_STATE.to_string(),
        to_state: TERMINAL_STATE.to_string(),
        required_role: RESERVED_REVIEWER_ROLE.to_string(),
        required_satisfaction: Some(vec!["satisfied".to_string()]),
        requires_approver: true,
        hook: None,
    });
    transitions.push(TransitionDefinition {
        from_state: OUTCOME_REFLECTION_REVIEW_STATE.to_string(),
        to_state: final_revision_state,
        required_role: RESERVED_REVIEWER_ROLE.to_string(),
        required_satisfaction: Some(vec!["needs_revision".to_string()]),
        requires_approver: false,
        hook: None,
    });

    states.push(StateDefinition {
        name: TERMINAL_STATE.to_string(),
        registry_section: "completed".to_string(),
        projection_targets: candidate.projection_targets.clone(),
        is_terminal: true,
        ..empty_state()
    });

    let directory = format!("{}s", kind);
    let success_rubric = generated_success_rubric(candidate);

    Ok(PlaybookMachine {
        kind: kind.clone(),
        directory: directory.clone(),
        registry: format!("{}.md", directory),
        parent_kind: None,
        description: candidate.intent.clone(),
        route: RouteConfig {
            description: Some(candidate.route_description.clone()),
            triggers: candidate.route_triggers.clone(),
        },
        required_fields: vec![],
        roles,
        states,
        transitions,
        register: candidate.register,
        success_rubric,
        // Always-on + None-safe: whatever the candidate carries (including
        // nothing) is threaded straight onto the generated machine. Presence
        // enforcement lives in `validate_measurement_definition`, not here.
        outcome_predicate: candidate.outcome_predicate.clone(),
        ..Default::default()
    })
}

/// Measurement-enforcing variant of [`generate`].
///
/// Runs the DEFINE block ([`validate_measurement_definition`]) FIRST — for a
/// `driven` candidate, every doer/review state must carry a present and
/// falsifiable `success_criteria`, or generation is rejected with
/// [`GenerateError::VacuousSuccessCriteria`]. On success it delegates to
/// [`generate`], which is otherwise identical (and always threads whatever
/// `success_criteria` the candidate carries onto the built `MeasurementSpec`s,
/// enforcement or not).
///
/// This is deliberately a SEPARATE entry point rather than a mode of
/// `generate`: the anti-Goodhart gate is dark-gated (default off) exactly like
/// the loader-block decision. Enforcement lives at a binary/config boundary
/// (the engine reads `ANVIL_ENFORCE_MEASUREMENT_DEFINITION`); this pure
/// library exposes both shapes and reads no environment. The default public
/// `generate` never enforces, so existing candidates (and the proto-intake
/// path, whose wire contract cannot yet carry criteria) keep working.
pub fn generate_enforcing(candidate: &CandidatePlaybook) -> Result<PlaybookMachine, GenerateError> {
    validate_measurement_definition(candidate)?;
    generate(candidate)
}

fn generated_success_rubric(candidate: &CandidatePlaybook) -> Option<super::types::SuccessRubric> {
    let mut rubric = candidate.success_rubric.clone();
    if !candidate.anchors.is_empty() {
        let rubric = rubric.get_or_insert_with(Default::default);
        for anchor in &candidate.anchors {
            if !rubric.anchors.iter().any(|existing| existing == anchor) {
                rubric.anchors.push(anchor.clone());
            }
        }
    }
    rubric
}

fn validate_route(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    if candidate.route_description.trim().is_empty() {
        return Err(GenerateError::BlankRouteDescription);
    }
    if candidate.route_triggers.is_empty() {
        return Err(GenerateError::EmptyRouteTriggers);
    }
    Ok(())
}

fn validate_projection_targets(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    if candidate.projection_targets.is_empty() {
        return Err(GenerateError::EmptyProjectionTargets);
    }
    Ok(())
}

fn validate_proposed_state_fields(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    for (index, proposed) in candidate.proposed_states.iter().enumerate() {
        for (field, value) in [
            ("state", proposed.state.as_str()),
            ("role", proposed.role.as_str()),
            ("intent", proposed.intent.as_str()),
            ("expected_output", proposed.expected_output.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(GenerateError::BlankProposedStateField {
                    index,
                    field: field.to_string(),
                });
            }
        }
    }

    Ok(())
}

fn validate_roles(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    for proposed in &candidate.proposed_states {
        if proposed.role.eq_ignore_ascii_case(RESERVED_REVIEWER_ROLE) {
            return Err(GenerateError::ReservedRole {
                role: proposed.role.clone(),
            });
        }
    }

    Ok(())
}

fn validate_state_names(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    let mut emitted_names = BTreeSet::new();
    emitted_names.insert(TERMINAL_STATE.to_string());
    emitted_names.insert(OUTCOME_REFLECTION_REVIEW_STATE.to_string());

    for proposed in &candidate.proposed_states {
        let names = [
            proposed.state.clone(),
            format!("{}_review", proposed.state),
            format!("{}_revision", proposed.state),
        ];
        for name in names {
            if !emitted_names.insert(name.clone()) {
                return Err(GenerateError::StateNameCollision { name });
            }
        }
    }

    Ok(())
}

fn measurement_by_role(
    role: &str,
    intent: String,
    expected_output: String,
    success_criteria: Option<String>,
    evidence_obligation: Vec<EvidenceClass>,
) -> BTreeMap<String, MeasurementSpec> {
    let mut measurement_by_role = BTreeMap::new();
    measurement_by_role.insert(
        role.to_string(),
        MeasurementSpec {
            intent,
            expected_output,
            success_criteria,
            evidence_obligation,
        },
    );
    measurement_by_role
}

fn review_measurement_by_role(
    state: &str,
    success_criteria: Option<String>,
    evidence_obligation: Vec<EvidenceClass>,
) -> BTreeMap<String, MeasurementSpec> {
    let mut measurement_by_role = BTreeMap::new();
    let mut spec = MeasurementSpec {
        intent: format!(
            "Review the {} output for correctness, completeness, and readiness to advance.",
            state
        ),
        expected_output: format!(
            "A review verdict for {}: satisfied or needs_revision, with reasons.",
            state
        ),
        // The reviewer judges the same author-authored success signal the doer
        // was measured against, so satisfied/needs_revision verdicts are
        // grounded in the same checkable criterion rather than the reviewer's
        // own independent (and potentially vaguer) judgment call.
        success_criteria,
        evidence_obligation,
    };
    if state == "trial" {
        // Overrides any candidate-authored criteria: the "trial" state is a
        // generator-owned convention (trialing a *generated* playbook), and
        // its review guidance must always point at anchor coverage, not
        // whatever per-step criterion the candidate happened to author.
        spec.success_criteria = Some(
            "Run anchor coverage validation against the success_rubric and resolved exemplars. If a scored dimension lacks a resolved anchor, accept only when ledger_classification is none_yet and none_yet_justification is complete."
                .to_string(),
        );
    }
    measurement_by_role.insert(RESERVED_REVIEWER_ROLE.to_string(), spec);
    measurement_by_role
}

/// The DEFINE block: for `driven` candidates, every proposed state must carry
/// a `success_criteria` that is present and falsifiable (see
/// [`criterion_is_falsifiable`]), AND the candidate must declare an
/// `outcome_predicate` with a non-blank `terminal_state`. This is the
/// anti-Goodhart gate — a generated playbook whose steps are never told what
/// "good" looks like cannot be measured, only rubber-stamped, and a playbook
/// with no declared checkable-fact outcome can never be evaluated by
/// `fold_outcome_predicate` — only ever guessed at via the registry's generic
/// per-state terminal flag.
///
fn validate_measurement_definition(candidate: &CandidatePlaybook) -> Result<(), GenerateError> {
    if candidate.register != Register::Driven {
        return Ok(());
    }

    for proposed in &candidate.proposed_states {
        let criterion = proposed.success_criteria.as_deref().unwrap_or("").trim();
        if criterion.is_empty() || !criterion_is_falsifiable(criterion) {
            return Err(GenerateError::VacuousSuccessCriteria {
                state: proposed.state.clone(),
            });
        }
    }

    let terminal_state = candidate
        .outcome_predicate
        .as_ref()
        .map(|predicate| predicate.terminal_state.trim())
        .unwrap_or("");
    if terminal_state.is_empty() {
        return Err(GenerateError::MissingOutcomePredicate);
    }

    Ok(())
}

/// Whether `criterion` names something checkable rather than a vague quality
/// judgment — the anti-Goodhart gate's falsifiability lint.
///
/// This is a deliberately SIMPLE heuristic, not NLP. It looks for the
/// presence of any concrete, checkable marker; the ABSENCE of every marker is
/// treated as vacuous even without a blocklist of known-bad phrases ("looks
/// good", "high quality") — bias is toward REJECTING when in doubt, and
/// ACCEPTING as soon as one concrete token is found.
///
/// A criterion is falsifiable when it contains at least one of:
/// - a digit (a numeric threshold, an HTTP/error status code, a count)
/// - a backtick- or double-quote-delimited token (a command, a literal, a
///   code reference)
/// - a file path or extension (`foo.rs`, `Cargo.toml`, a `/`-separated path)
/// - an ALL-CAPS symbol of 3+ characters (a named error/status code such as
///   `ENOENT` or `ECONNREFUSED`)
/// - a known command-name keyword (`cargo`, `make`, `npm`, `git`, `pytest`, …)
/// - an explicit zero/negative-case keyword (`zero`, `empty`, `none`,
///   `fails`, `rejects`, `panics`, …)
pub fn criterion_is_falsifiable(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }

    has_digit(trimmed)
        || has_delimited_token(trimmed)
        || has_path_or_extension(trimmed)
        || has_named_code_symbol(trimmed)
        || has_command_keyword(trimmed)
        || has_negative_case_keyword(trimmed)
}

fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

fn has_delimited_token(s: &str) -> bool {
    s.matches('`').count() >= 2 || s.matches('"').count() >= 2
}

fn has_path_or_extension(s: &str) -> bool {
    s.split_whitespace().any(|word| {
        let cleaned =
            word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '/');
        if cleaned.contains('/') && cleaned.len() > 1 {
            return true;
        }
        match cleaned.rfind('.') {
            Some(dot_idx) => {
                let extension = &cleaned[dot_idx + 1..];
                let len = extension.chars().count();
                (2..=10).contains(&len) && extension.chars().all(|c| c.is_ascii_alphanumeric())
            }
            None => false,
        }
    })
}

fn has_named_code_symbol(s: &str) -> bool {
    s.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| {
            token.len() >= 3
                && token.chars().any(|c| c.is_ascii_alphabetic())
                && token
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
}

const COMMAND_KEYWORDS: [&str; 11] = [
    "cargo", "npm", "yarn", "make", "git", "pytest", "docker", "curl", "bash", "python", "brine",
];

fn has_command_keyword(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    COMMAND_KEYWORDS.iter().any(|keyword| lower.contains(keyword))
}

const NEGATIVE_CASE_KEYWORDS: [&str; 9] = [
    "zero", "empty", "none", "fails", "failing", "rejects", "rejected", "panics", "errors",
];

fn has_negative_case_keyword(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    NEGATIVE_CASE_KEYWORDS.iter().any(|keyword| lower.contains(keyword))
}

fn outcome_reflection_measurement_by_role(
    evidence_obligation: Vec<EvidenceClass>,
) -> BTreeMap<String, MeasurementSpec> {
    let mut measurement_by_role = BTreeMap::new();
    measurement_by_role.insert(
        RESERVED_REVIEWER_ROLE.to_string(),
        MeasurementSpec {
            intent: "Review the generated playbook's outcome reflection before terminal completion."
                .to_string(),
            expected_output: "A verdict that records whether the observed outcome is judgeable now and, when it is, compares scores vs anchors before allowing completion."
                .to_string(),
            success_criteria: Some(
                "Use scores vs anchors when judgeable-now; otherwise record why the outcome cannot yet be scored without invoking deferred-measurement machinery."
                    .to_string(),
            ),
            evidence_obligation,
        },
    );
    measurement_by_role
}

fn candidate_evidence_obligation(candidate: &CandidatePlaybook) -> Vec<EvidenceClass> {
    let mut obligation = Vec::new();
    for class in candidate
        .proposed_states
        .iter()
        .flat_map(|state| state.evidence_obligation.iter().copied())
    {
        if !obligation.contains(&class) {
            obligation.push(class);
        }
    }
    obligation
}

fn empty_state() -> StateDefinition {
    StateDefinition {
        name: String::new(),
        role_filters: vec![],
        registry_section: String::new(),
        projection_targets: vec![],
        is_review_gate: false,
        is_terminal: false,
        hook: None,
        hooks_by_role: BTreeMap::new(),
        measurement_by_role: BTreeMap::new(),
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn slug(input: &str) -> String {
    let mut out = String::new();
    let mut previous_was_underscore = false;

    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_underscore = false;
        } else if !previous_was_underscore {
            out.push('_');
            previous_was_underscore = true;
        }
    }

    out.trim_matches('_').to_string()
}
