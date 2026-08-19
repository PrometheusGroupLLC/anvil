//! Evidence-obligation satisfaction semantics (T-EEC-1 Phase 1).
//!
//! Pure functions — no I/O, no environment. Encodes the strength ordering and
//! the v1 satisfaction predicate for the `evidence_obligation` a
//! [`MeasurementSpec`](super::types::MeasurementSpec) declares.
//!
//! ## Consumption contract (T-EEC-2 boundary)
//!
//! T-EEC-1 owns the predicate; it consumes a *set of claimed evidence classes*
//! (`&[EvidenceClass]`). T-EEC-2 owns the `(class, opaque-reference)`
//! `claimed_evidence` channel on the lifecycle request (§8-Q3); it feeds this
//! predicate by PROJECTING its claimed items to their classes
//! (`items.iter().map(|it| it.class)`) — no redefinition, no coupling drift.

use super::types::EvidenceClass;

/// Structural result of comparing caller-claimed evidence with a declared
/// step obligation.
///
/// This result deliberately says only what was *claimed*. Whether an opaque
/// reference is admissible evidence is a downstream judging concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceAssessmentStatus {
    /// Every obligated class has an equal-or-stronger claimed class.
    PresentAsClaimed,
    /// At least one claim was supplied, but one or more obligated classes are
    /// still unsatisfied.
    Incomplete,
    /// The step declares an obligation and the caller supplied no claims.
    Absent,
}

impl EvidenceAssessmentStatus {
    /// Stable token persisted on the step-measurement record.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PresentAsClaimed => "present-as-claimed",
            Self::Incomplete => "incomplete",
            Self::Absent => "absent",
        }
    }

    /// Parse the stable persisted token.
    pub fn from_token(value: &str) -> Option<Self> {
        match value {
            "present-as-claimed" => Some(Self::PresentAsClaimed),
            "incomplete" => Some(Self::Incomplete),
            "absent" => Some(Self::Absent),
            _ => None,
        }
    }
}

/// Pure evidence-obligation assessment.
///
/// `missing_classes` preserves first-declaration order and contains each
/// unsatisfied class at most once, even when the authored obligation repeats a
/// class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAssessment {
    pub status: EvidenceAssessmentStatus,
    pub missing_classes: Vec<EvidenceClass>,
}

/// Explicit obligation-satisfaction strength ranking. Stronger = higher:
/// `ArtifactOfConsequence` (2) > `VerifiableCitation` (1) > `SelfDescription`
/// (0).
///
/// This is DELIBERATELY an explicit `match`, not a derived `Ord`/`PartialOrd`
/// on `EvidenceClass` and not `std::mem::discriminant` order (which is
/// strongest-first — the reverse of strength). The rubric `EvidenceClass`
/// enum's derive set is load-bearing elsewhere (its default-to-weakest
/// semantics), and obligation-satisfaction ordering is an independent surface
/// from rubric judging (spec §1.1); an explicit match keeps the two decoupled
/// and is robust to future variant reordering.
pub fn evidence_strength(class: EvidenceClass) -> u8 {
    match class {
        EvidenceClass::ArtifactOfConsequence => 2,
        EvidenceClass::VerifiableCitation => 1,
        EvidenceClass::SelfDescription => 0,
    }
}

/// The v1 obligation-satisfaction predicate.
///
/// `required` is the obligation's declared classes (order- and
/// duplicate-insensitive; treated as a set). `claimed_classes` is the set of
/// evidence classes the actor claimed at this step. Returns `true` iff every
/// required class has ≥1 claimed item of that class OR STRONGER (min-count 1;
/// item reuse allowed — a single claimed item may satisfy multiple distinct
/// required classes simultaneously, decision M2).
pub fn obligation_satisfied(required: &[EvidenceClass], claimed_classes: &[EvidenceClass]) -> bool {
    required.iter().all(|req| {
        claimed_classes
            .iter()
            .any(|claimed| evidence_strength(*claimed) >= evidence_strength(*req))
    })
}

/// Assess the caller's claimed classes against one step's obligation.
///
/// An empty obligation produces no assessment: there is nothing to observe and
/// legacy callers retain their pre-evidence record shape. For a declared
/// obligation, all strength and item-reuse semantics delegate to
/// [`obligation_satisfied`]. Missing classes are computed with the same
/// predicate one required class at a time; no second rank table exists here.
pub fn assess_evidence_obligation(
    required: &[EvidenceClass],
    claimed_classes: &[EvidenceClass],
) -> Option<EvidenceAssessment> {
    if required.is_empty() {
        return None;
    }

    let satisfied = obligation_satisfied(required, claimed_classes);
    let status = if satisfied {
        EvidenceAssessmentStatus::PresentAsClaimed
    } else if claimed_classes.is_empty() {
        EvidenceAssessmentStatus::Absent
    } else {
        EvidenceAssessmentStatus::Incomplete
    };

    let mut missing_classes = Vec::new();
    for required_class in required.iter().copied() {
        if !obligation_satisfied(std::slice::from_ref(&required_class), claimed_classes)
            && !missing_classes.contains(&required_class)
        {
            missing_classes.push(required_class);
        }
    }

    Some(EvidenceAssessment {
        status,
        missing_classes,
    })
}
