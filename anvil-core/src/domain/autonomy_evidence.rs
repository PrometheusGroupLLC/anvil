//! The case a playbook makes for a rung: which runs were clean, who caught what
//! and when, and how many of the steps a person had a hand in.
//!
//! ## What this fold adds, and what it deliberately does NOT re-derive
//!
//! Three folds in this crate already answer "how many runs were clean":
//! [`crate::domain::playbook_run_fidelity`] computes `revision_cycles`,
//! `reached_terminal` and `dangling` **per instance**;
//! [`crate::domain::step_two_by_two`]'s one-shot axis is *"passed one-shot iff
//! the instance never bounced back into `{step_kind}_revision`"*; and
//! [`crate::domain::survivor_outcome`] takes begun as the denominator so a
//! dangling run counts as a failure. A fourth opinion about the same question
//! is how two surfaces come to disagree about the same run while both look
//! right, which is the failure [`crate::domain::run_detail`] exists to prevent.
//!
//! So **this fold does not define cleanliness. It takes it.**
//! [`EvidenceRunInput::revision_cycles`] and
//! [`EvidenceRunInput::reached_terminal`] are the caller's — the values the
//! fidelity fold already produced — and `clean` is
//! `reached_terminal && revision_cycles == 0`, which is the existing predicate
//! written down rather than a new one. The structural rule underneath it,
//! [`crate::domain::shared_types::is_revision_state`], is now shared by all
//! three modules instead of copied into each.
//!
//! What is genuinely absent from every existing fold, and is what this module
//! is for, is **attribution**. `revision_cycles` is a count: it never says which
//! moment, which step, or who caught it. `ReviewAuthenticity` compares the
//! entering and exiting actors and deliberately names neither. The autonomy
//! ladder's fourth evidence tile is *"what it got wrong … which week and who
//! caught it"*, and a count answers none of that.
//!
//! ## Every definition here can be disagreed with, and that is the point
//!
//! A measurement whose definition cannot be wrong is not a measurement.
//!
//! ### THE ONE PLACE A CORRECTION IS COUNTED
//!
//! [`count_revision_cycles`] is the only function that turns a run's history
//! into a number of corrections, and the engine's emit path calls it rather
//! than writing the loop a second time.
//!
//! The two folds that need this number nonetheless read DIFFERENT SOURCES, and
//! that is a property of the question each is answering rather than an oversight
//! to be tidied away:
//!
//! * [`crate::domain::playbook_run_fidelity`] folds the hearth's ACTIVITY LOG,
//!   because it answers "every instance of every kind" and the activity log is
//!   the one stream that carries all of them.
//! * This fold and the emit path read the ARTIFACT'S OWN per-file transition
//!   store, because they answer "this run", and for one run that store is the
//!   evidence — [`crate::domain::transition_log::fold_state`] already rules that
//!   the events are authoritative and the denormalised header is a projection.
//!
//! They can disagree: a pruned activity log, a pruned event directory, or an
//! artifact adopted into governance from outside it are each sufficient. That
//! is why [`RunVerdict`] reports the difference in BOTH directions
//! ([`RunVerdict::corrections_unattributed`] and
//! [`RunVerdict::corrections_beyond_count`]) instead of taking whichever number
//! it was handed. A fold that silently preferred one would make a pruned history
//! read as a clean stretch in one direction and hide a real correction in the
//! other.
//!
//! ### CLEAN — taken, not invented
//!
//! `reached_terminal && revision_cycles == 0`. Ways to disagree, all of them
//! inherited from the definition this adopts rather than introduced here:
//!
//! 1. It sees only corrections that ROUTED — work sent into a `*_revision`
//!    state. A reviewer who recorded `full_revision` on a machine with no such
//!    state, or a doer who quietly redid a step, leaves the run clean.
//! 2. It counts a run still in flight as not clean, which makes the case worse
//!    the more work is open. Counting only finished runs would bias the case
//!    the other way — that is the survivorship argument
//!    `survivor_outcome` settles, and this fold follows it rather than
//!    reopening it.
//! 3. It says nothing about what the run produced. A run can be clean here and
//!    have shipped something bad.
//!
//! ### THE CATCH — [`Correction`]
//!
//! One correction per transition INTO a revision state. The catcher is the
//! actor who recorded that transition; the step it caught is the most recent
//! earlier step a DIFFERENT actor recorded, since a reviewer cannot have caught
//! their own step. Ways to disagree:
//!
//! 1. It blames the immediately preceding step by somebody else. The defect may
//!    have entered three steps earlier.
//! 2. `VerdictFinding::origin_phase` on the review-verdict sink already asks the
//!    FINDER where the defect originated, which is strictly better information
//!    than guessing from position — and it is `Vec::new()` on every emit today
//!    (`anvil-engine/src/main.rs`, `emit_review_verdict`). This positional rule
//!    is the weaker stand-in, named as one, and it should be replaced when
//!    findings are authored rather than layered on top of.
//! 3. It can only see what somebody RECORDED. A wrong action nobody caught is
//!    invisible, so the fact is *what was caught*, not *what was wrong*.
//!
//! [`RunVerdict::corrections_unattributed`] is the honest remainder: when the
//! declared `revision_cycles` exceeds the number of corrections the history
//! could name, the difference is reported rather than dropped. A fold that
//! silently returned the shorter list would make a missing history look like a
//! clean stretch.
//!
//! ### THE HANDS — [`count_human_touches`]
//!
//! A human touch is a step naming an **approver**, plus a step whose actor the
//! run's own `actors:` table types as `human`.
//!
//! The approver is primary because it is the only human participation this
//! engine actually records. Measured in `foundry-business-hearth`: `approver:`
//! appears on **80 of the 249** transition events written since 2026-06-15,
//! against **0 of 50** tracks carrying a `type: human` actor at all. The
//! actors-table route is dormant by construction — `begin`, `snapshot` and
//! `complete` each require a non-empty `actor_model` AND `actor_provider`,
//! which a person does not have, and the actor writer discards both for a
//! human — so a fold resting on it alone would report "no human touches" on
//! every run that exists. It stays a contributor because the dormancy is a
//! defect to be fixed rather than a shape to design around.
//!
//! Ways to disagree:
//!
//! 1. It counts STEPS, not minutes. The drawn card says "2 min, down from 14".
//!    No duration is recorded anywhere in this engine. A count is a different
//!    quantity from a duration, and this module says so rather than converting
//!    one into the other behind a label that reads like time.
//! 2. An approver named on a step may not have done anything, and the same
//!    person named on every step counts once per step — so a run with more
//!    steps reads as more hands even when the person did the same work.
//! 3. An actor the table does not list counts as not-human, which undercounts
//!    an incomplete table. Counting unknowns as human would overcount instead.
//!
//! ## Absent is absent, and zero is a real answer
//!
//! [`fold_autonomy_evidence`] returns `None` for a playbook with no runs. A
//! playbook nobody has run has no case; `0 clean of 0 attempted` reads as a
//! measured perfect failure rather than as the absence of evidence, and the
//! ladder's first beat is that the evidence must exist BEFORE the act.
//! [`RunVerdict::human_touches`] is `None` for a run that has taken no step —
//! there is nothing to count over — and `Some(0)` for a run whose steps were
//! examined and none was a person's. Those are different facts. The
//! discriminator is therefore always `is_some()`; a check written against zero
//! would pass an implementation that eats the real zeros, which are common here.
//!
//! ## Where a name may appear, and where it may not
//!
//! [`Correction::caught_by`] carries a RAW actor name. That is permitted here
//! and forbidden downstream, and the split is deliberate:
//!
//! * This is a READ fold, served to the product's own screens beside
//!   [`crate::domain::run_detail`], whose `RunStep::actor` and `RunActor::name`
//!   already carry raw names on the same surface.
//! * The redacted playbook-measurement sink states that it *"never carries raw
//!   actor identity"* (`crate::ports::review_verdict_port`), and
//!   `ReviewAuthenticity` compares actors while naming neither. Nothing from
//!   this record may be copied into that sink except a SCORE.
//!   [`grade_cleanliness_score`] exists precisely so the emit path has a number
//!   to carry and no name to leak.
//!
//! ## No cost
//!
//! `proto/anvil.proto` refuses a cost figure on the `RunDetail` response, and
//! that refusal holds here: this record carries none, and
//! `no part of the case carries a cost figure` in
//! `anvil-core/features/autonomy_evidence.feature` serialises the whole record
//! and fails on any cost-shaped key so an addition cannot slip in quietly.
//!
//! The refusal's wording — *"no spend, no token count, no price, on any
//! artifact or any event"* — is nonetheless broader than what is true. A
//! per-run `total_cost_usd` and `total_tokens` already reach this engine from
//! temper's scorecards (`anvil-engine/src/scorecard_reader.rs`), both
//! `#[serde(default)]`, so a scorecard missing the key yields a fabricated
//! `0.0` today. **Whether the ladder card may show a cost is the owner's
//! decision, not this module's**, and the case for making it is written up in
//! the track's spec.
//!
//! ## What this module does NOT decide
//!
//! The rung ladder's levels and the number of bad runs that costs a playbook
//! its rung. The drawn frames name two rungs and the number two; neither
//! appears in any decision record, and a drawn frame is a proposal rather than
//! a ruling. [`AutonomyEvidence::consecutive_unclean_from_newest`] measures the
//! quantity such a threshold is applied to, and stops there.

use crate::domain::shared_types::is_revision_state;
use crate::domain::status::StatusTransition;
use serde::Serialize;
use std::collections::HashMap;

/// The `actors:` table `type` that marks a person.
pub const ACTOR_TYPE_HUMAN: &str = "human";

/// The name this fold records itself under when it grades a run for the
/// playbook-measurement sink.
///
/// A record carrying it was graded by THIS rule; a record without it was not
/// graded at all. The NAME is the discriminator — the score is not, because an
/// unclean run scores a real `0.0` and a reader keying on the score cannot tell
/// that from an implementation that never graded anything.
pub const CLEANLINESS_GRADER: &str = "run_cleanliness/v1";

/// The quality dimension this fold contributes to a measurement record.
pub const CLEANLINESS_DIMENSION: &str = "run_cleanliness";

/// One run, as the caller has ALREADY read, folded and graded it.
///
/// This module performs no I/O and re-derives nothing. The ordered history is
/// resolved once by [`crate::domain::transition_log`]; whether the run reached a
/// terminal state and how many times it was sent back are
/// [`crate::domain::playbook_run_fidelity`]'s answers, carried in rather than
/// recomputed. A second opinion about either, formed here, would be a second
/// opinion about what happened.
#[derive(Debug, Clone, Default)]
pub struct EvidenceRunInput {
    /// The run's artifact-directory id — the same value `LiveInstance` and
    /// `RunNode` carry.
    pub instance_id: String,
    /// Whether the run reached a state the playbook machine declares terminal.
    /// `PlaybookRunInstanceFidelity::reached_terminal`.
    pub reached_terminal: bool,
    /// How many times the run was sent back.
    /// `PlaybookRunInstanceFidelity::revision_cycles`.
    pub revision_cycles: u64,
    /// The ALREADY-FOLDED ordered history, oldest first. Used ONLY to attribute
    /// the corrections the count above already established, and to count hands.
    pub transitions: Vec<StatusTransition>,
    /// The run's `actors:` table, exactly as `status.yaml` carries it.
    pub actors: Option<HashMap<String, serde_yaml::Value>>,
}

/// One wrong action, the moment it was caught, and the person who caught it.
///
/// Every field is a plain `String` and EMPTY means the record does not carry it.
/// A correction recorded as the very first step of a run has no earlier step by
/// anybody else, so `wrong_step_state` and `wrong_step_actor` are empty there —
/// the catch is real, what it caught is not identifiable, and saying so is
/// different from naming an arbitrary step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Correction {
    /// WHICH RUN. The ladder card's wrong-run fact names a specific run, so the
    /// correction carries the run's identity rather than leaving a caller to
    /// remember which list it came out of.
    pub run_id: String,
    /// WHICH WEEK. When the correction was recorded, as recorded. Empty when the
    /// step carries no time — never a substituted "now", which would date a
    /// mistake to the moment somebody looked at it.
    pub at: String,
    /// The revision state the run was sent into.
    pub revision_state: String,
    /// The actor who recorded the transition — who caught it. A RAW name; see
    /// the module doc for where that is permitted and where it is not.
    pub caught_by: String,
    /// The reviewer verdict recorded on the same step, when there was one.
    /// Empty when the machine routed the run back without a recorded verdict.
    pub verdict: String,
    /// The state of the step the correction overturned.
    pub wrong_step_state: String,
    /// The actor who took the overturned step.
    pub wrong_step_actor: String,
}

/// One run's grade.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunVerdict {
    pub run_id: String,
    /// Whether the run reached a machine-declared terminal state — the
    /// completion FLOOR, carried through unchanged. Never the quality measure.
    pub reached_terminal: bool,
    /// How many times the run was sent back, as the fidelity fold counted them.
    pub revision_cycles: u64,
    /// `reached_terminal && revision_cycles == 0`.
    pub clean: bool,
    /// Every correction the history could attribute.
    pub corrections: Vec<Correction>,
    /// Corrections the declared count knows about that the history could NOT
    /// name. A fold that returned the shorter list silently would make a pruned
    /// history read as a clean stretch.
    pub corrections_unattributed: u64,
    /// Corrections the HISTORY names that the declared count does not know
    /// about — the mirror image, and the one a `saturating_sub` swallows. It is
    /// reported because the alternative is a run that reads `clean` while
    /// carrying a non-empty list of what went wrong.
    pub corrections_beyond_count: u64,
    /// Whether the declared count and the attributed history agree. `false`
    /// means one of the two sources is incomplete and NEITHER number should be
    /// read as settled.
    pub counts_agree: bool,
    /// Human touches, or `None` for a run that has taken no step at all. NEVER
    /// `Some(0)` for a run with nothing to count over, and never `None` for a
    /// run whose steps were examined and held no person — those are different
    /// facts and the discriminator is `is_some()`.
    pub human_touches: Option<u32>,
}

/// The whole case, folded across a playbook's runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AutonomyEvidence {
    /// How many runs there were. Printed beside `runs_clean` — the ratio is the
    /// fact, and a clean count on its own passes on a playbook with one run.
    pub runs_attempted: u32,
    /// How many of them were clean.
    pub runs_clean: u32,
    /// Per-run grades, oldest run first.
    pub run_verdicts: Vec<RunVerdict>,
    /// Every attributed correction across every run, oldest run first.
    pub corrections: Vec<Correction>,
    /// Hands on the OLDEST run, `None` when that run has nothing to count over.
    pub human_touches_earliest: Option<u32>,
    /// Hands on the NEWEST run, `None` when that run has nothing to count over.
    pub human_touches_latest: Option<u32>,
    /// The DELTA the ladder card asks for — latest minus earliest, signed, so a
    /// playbook that got worse says so. `None` unless BOTH ends carry a count:
    /// a delta against an absent endpoint would be the endpoint's value wearing
    /// a difference's name.
    pub human_touch_delta: Option<i64>,
    /// How many runs at the newest end were unclean, in an unbroken run,
    /// stopping at the first clean one. The quantity a demotion threshold is
    /// applied to. The THRESHOLD is not decided here.
    pub consecutive_unclean_from_newest: u32,
}

/// The verdict a step carries, or `None`. An empty string is no verdict.
fn verdict_of(step: &StatusTransition) -> Option<&str> {
    step.satisfaction
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// Whether a name resolves, through this run's own table, to a person.
fn is_human(actors: Option<&HashMap<String, serde_yaml::Value>>, name: &str) -> bool {
    actors
        .and_then(|table| table.get(name))
        .and_then(|entry| entry.get("type"))
        .and_then(|t| t.as_str())
        .map(|t| t.trim() == ACTOR_TYPE_HUMAN)
        .unwrap_or(false)
}

/// Count the steps a person had a hand in.
///
/// `None` for a run that has taken no step: there is nothing to count over, and
/// answering `0` would assert that a person was absent from steps that do not
/// exist. `Some(0)` for a run whose steps were examined and held no person.
pub fn count_human_touches(
    transitions: &[StatusTransition],
    actors: Option<&HashMap<String, serde_yaml::Value>>,
) -> Option<u32> {
    if transitions.is_empty() {
        return None;
    }
    let mut touches = 0u32;
    for step in transitions {
        // The approver: the only human participation this engine records
        // today. A named approver is a person by construction — the field
        // exists to say who allowed the step.
        if step
            .approver
            .as_deref()
            .map(|a| !a.trim().is_empty())
            .unwrap_or(false)
        {
            touches += 1;
        }
        // The actor, when the run's own table types them as a person. Dormant
        // in every hearth measured so far; see the module doc.
        if let Some(actor) = step.actor.as_deref() {
            if is_human(actors, actor) {
                touches += 1;
            }
        }
    }
    Some(touches)
}

/// The step a correction at `index` overturned: the most recent earlier step a
/// DIFFERENT actor recorded.
fn overturned_step(transitions: &[StatusTransition], index: usize) -> Option<&StatusTransition> {
    let catcher = transitions[index].actor.as_deref().unwrap_or_default();
    transitions[..index]
        .iter()
        .rev()
        .find(|earlier| earlier.actor.as_deref().unwrap_or_default() != catcher)
}

/// How many corrections a run's history records: transitions INTO a revision
/// state, by the one shared structural rule.
///
/// THE ONLY PLACE THIS LOOP IS WRITTEN. The engine's emit path calls it instead
/// of counting for itself — two loops over the same rule is how two surfaces
/// come to report different numbers for the same run while both look right.
pub fn count_revision_cycles(transitions: &[StatusTransition]) -> u64 {
    transitions
        .iter()
        .filter(|t| is_revision_state(&t.to))
        .count() as u64
}

/// Attribute the corrections a run's history can name.
pub fn attribute_corrections(run: &EvidenceRunInput) -> Vec<Correction> {
    let mut out = Vec::new();
    for (index, step) in run.transitions.iter().enumerate() {
        if !is_revision_state(&step.to) {
            continue;
        }
        let overturned = overturned_step(&run.transitions, index);
        out.push(Correction {
            run_id: run.instance_id.clone(),
            at: step.at.clone().unwrap_or_default(),
            revision_state: step.to.clone(),
            caught_by: step.actor.clone().unwrap_or_default(),
            verdict: verdict_of(step).unwrap_or_default().to_string(),
            wrong_step_state: overturned.map(|s| s.to.clone()).unwrap_or_default(),
            wrong_step_actor: overturned
                .and_then(|s| s.actor.clone())
                .unwrap_or_default(),
        });
    }
    out
}

/// Grade one run: clean or not, what was caught, and how many hands.
pub fn grade_run(run: &EvidenceRunInput) -> RunVerdict {
    let corrections = attribute_corrections(run);
    let attributed = corrections.len() as u64;
    RunVerdict {
        run_id: run.instance_id.clone(),
        reached_terminal: run.reached_terminal,
        revision_cycles: run.revision_cycles,
        // TAKEN, NOT INVENTED. The same predicate `playbook_run_fidelity`
        // already computes and `step_two_by_two`'s one-shot axis already uses.
        clean: run.reached_terminal && run.revision_cycles == 0,
        corrections_unattributed: run.revision_cycles.saturating_sub(attributed),
        corrections_beyond_count: attributed.saturating_sub(run.revision_cycles),
        counts_agree: attributed == run.revision_cycles,
        corrections,
        human_touches: count_human_touches(&run.transitions, run.actors.as_ref()),
    }
}

/// The cleanliness SCORE for a run, for the redacted measurement sink.
///
/// `1.0` clean, `0.0` not. A number and nothing else: the sink states that it
/// never carries raw actor identity, so the catcher's name stops here.
///
/// The zero is a REAL zero. A reader deciding whether a run was graded must key
/// on [`CLEANLINESS_GRADER`] being present, never on this being non-zero.
pub fn grade_cleanliness_score(reached_terminal: bool, revision_cycles: u64) -> f64 {
    if reached_terminal && revision_cycles == 0 {
        1.0
    } else {
        0.0
    }
}

/// The key runs are ordered by: when the run's first step happened, then its own
/// id.
///
/// The same rule [`crate::domain::run_detail`] orders nested runs by, and for
/// the same reason: ids are minted at creation and can be minted in a different
/// order than the work begins in.
fn run_order_key(run: &EvidenceRunInput) -> (String, String) {
    let first_at = run
        .transitions
        .first()
        .and_then(|t| t.at.clone())
        .unwrap_or_default();
    (first_at, run.instance_id.clone())
}

/// Fold a playbook's runs into the case for a rung.
///
/// `None` for a playbook with no runs — see the module doc. Never a record
/// reporting `0 of 0`.
pub fn fold_autonomy_evidence(runs: &[EvidenceRunInput]) -> Option<AutonomyEvidence> {
    if runs.is_empty() {
        return None;
    }

    let mut ordered: Vec<&EvidenceRunInput> = runs.iter().collect();
    ordered.sort_by_key(|run| run_order_key(run));

    let run_verdicts: Vec<RunVerdict> = ordered.iter().map(|run| grade_run(run)).collect();

    let runs_attempted = run_verdicts.len() as u32;
    let runs_clean = run_verdicts.iter().filter(|v| v.clean).count() as u32;
    let corrections: Vec<Correction> = run_verdicts
        .iter()
        .flat_map(|v| v.corrections.iter().cloned())
        .collect();

    let consecutive_unclean_from_newest =
        run_verdicts.iter().rev().take_while(|v| !v.clean).count() as u32;

    let human_touches_earliest = run_verdicts.first().and_then(|v| v.human_touches);
    let human_touches_latest = run_verdicts.last().and_then(|v| v.human_touches);
    // BOTH ends or nothing. A delta computed against an absent endpoint is the
    // present endpoint's value wearing a difference's name.
    let human_touch_delta = match (human_touches_earliest, human_touches_latest) {
        (Some(first), Some(last)) => Some(last as i64 - first as i64),
        _ => None,
    };

    Some(AutonomyEvidence {
        runs_attempted,
        runs_clean,
        run_verdicts,
        corrections,
        human_touches_earliest,
        human_touches_latest,
        human_touch_delta,
        consecutive_unclean_from_newest,
    })
}
