//! Fixed Lore-to-Anvil candidate playbook seam contract.

use super::exemplar::ExemplarFrontmatter;
use super::types::{AnchorRef, EvidenceClass, OutcomePredicate, Register, SuccessRubric};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePlaybook {
    pub source: String,
    pub evidence: Vec<String>,
    pub intent: String,
    #[serde(default)]
    pub route_description: String,
    #[serde(default)]
    pub route_triggers: Vec<String>,
    #[serde(default)]
    pub projection_targets: Vec<String>,
    /// Whether the candidate is router-driven or freely invocable. Existing
    /// candidates omit this field and therefore retain the driven default.
    #[serde(default, skip_serializing_if = "is_driven_register")]
    pub register: Register,
    pub proposed_states: Vec<ProposedState>,
    #[serde(default)]
    pub success_rubric: Option<SuccessRubric>,
    /// Author-supplied checkable-fact outcome predicate (distinct from
    /// `success_rubric`'s HOW WELL) — the machine state whose reaching is the
    /// checkable "did the world-change happen" fact. Threaded onto the
    /// generated `PlaybookMachine.outcome_predicate` by `generate::generate`
    /// and, for `driven` machines, enforced as present with a non-blank
    /// `terminal_state` by `generate::validate_measurement_definition`.
    ///
    /// `#[serde(default)]` is mandatory — existing candidates (authored before
    /// this field existed) omit this key and must continue to parse.
    #[serde(default)]
    pub outcome_predicate: Option<OutcomePredicate>,
    #[serde(default)]
    pub anchors: Vec<AnchorRef>,
    #[serde(default)]
    pub exemplars: Vec<CandidateExemplar>,
    #[serde(default)]
    pub ledger_classification: Option<LedgerClassification>,
    #[serde(default)]
    pub none_yet_justification: Option<NoneYetJustification>,
    pub at: String,
}

fn is_driven_register(register: &Register) -> bool {
    *register == Register::Driven
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedState {
    pub state: String,
    pub role: String,
    pub intent: String,
    pub expected_output: String,
    /// Author-supplied per-step success signal: the checkable criterion that
    /// distinguishes a *good* completion of this step from one that merely
    /// produced the expected output. Threaded onto the generated doer and
    /// review `MeasurementSpec`s by `generate::generate` and, for `driven`
    /// machines, enforced as present and falsifiable by
    /// `generate::validate_measurement_definition`.
    ///
    /// `#[serde(default)]` is mandatory — existing candidates (authored before
    /// this field existed) omit this key and must continue to parse.
    #[serde(default)]
    pub success_criteria: Option<String>,
    /// Ordered evidence classes declared for this proposed step. Generation
    /// carries these onto the doer, review, and revision measurements; the
    /// generated outcome review receives their stable deduplicated union.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_obligation: Vec<EvidenceClass>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateExemplar {
    pub frontmatter: ExemplarFrontmatter,
    pub body: String,
}

impl CandidateExemplar {
    pub fn id(&self) -> &str {
        &self.frontmatter.id
    }

    pub fn to_markdown(&self) -> Result<String, serde_yaml::Error> {
        let frontmatter = serde_yaml::to_string(&self.frontmatter)?;
        Ok(format!("---\n{}---\n{}\n", frontmatter, self.body.trim()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerClassification {
    pub corpus: String,
    pub ledger: String,
    pub classification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoneYetJustification {
    pub corpus_searched: String,
    pub ledger_searched: String,
    pub why_no_exemplar: String,
    pub production_routing_allowed: bool,
    pub followup_condition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedExemplarFile {
    pub id: String,
    pub markdown: String,
}

impl TryFrom<&CandidateExemplar> for GeneratedExemplarFile {
    type Error = serde_yaml::Error;

    fn try_from(exemplar: &CandidateExemplar) -> Result<Self, Self::Error> {
        Ok(Self {
            id: exemplar.id().to_string(),
            markdown: exemplar.to_markdown()?,
        })
    }
}
