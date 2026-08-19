//! Pure domain handler for the `route` RPC (playbook_routing_layer).
//!
//! `route(input) → candidate workflow_kinds`: returns the active, driven
//! (registry-resolvable, `register != free`) candidate machines with the
//! selection metadata the surface LLM needs to choose. The surface selects; the
//! engine then `begin`s the selected machine. When the driven candidate set is
//! empty, the handler returns a typed `no_match → candidate_playbook_intake`
//! outcome carrying the originating intent (M-3) — never an error.
//!
//! This handler is pure: it reads a `&dyn PlaybookRegistry` and returns a
//! `RouteResult`. The engine constructs a fresh registry per call (live-reload)
//! and maps the result onto the proto response.

use crate::domain::begin_adoption::has_open_begin;
use crate::domain::playbook::interpreter::outgoing_transitions;
use crate::domain::playbook::registry::{
    granted_driven_candidates, PlaybookRegistry, RouteOutcome,
};
use crate::domain::shared_types::RequestContext;
use crate::ports::query_port::{QueryError, QueryPort};

/// The originating-intent input the surface ships to `route`.
///
/// Only `message` participates in v0 routing logic (it echoes into the no-match
/// outcome's `intent`). `signal` is reserved for the optional BP4 ranker;
/// actor/scope identity is carried by the RPC layer for the downstream
/// `candidate_playbook_intake` handoff, not by this pure handler.
#[derive(Debug, Clone, Default)]
pub struct RouteRequest {
    /// The surface's free-text input (transcript / vision / …).
    pub message: String,
    /// Optional coarse context tag — reserved for the BP4 ranker, ignored here.
    pub signal: String,
    /// Caller access context used for route enforcement.
    pub ctx: RequestContext,
    /// The originating conversation id (resume-aware routing). Carried from the
    /// proto `RouteRequest.conversation_id` so the engine can bridge a
    /// continuation message to the conversation's open playbook. Empty when the
    /// surface omits it — resume simply never fires for that surface.
    pub conversation_id: String,
}

/// Selection metadata for one driven candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateMeta {
    pub kind: String,
    pub description: String,
    pub required_fields: Vec<String>,
}

/// The outcome of a `route` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// The driven candidate set for the surface LLM to select from.
    Candidates(Vec<CandidateMeta>),
    /// No driven machine is a candidate → hand off to the generation half.
    /// Carries the originating intent so the downstream caller can seed a
    /// `CandidatePlaybook` (M-3). `handoff` is always
    /// `"candidate_playbook_intake"`.
    NoMatch { handoff: String, intent: String },
}

/// The fixed handoff name for the no-match outcome.
pub const CANDIDATE_PLAYBOOK_INTAKE: &str = "candidate_playbook_intake";

/// Pure route handler.
pub struct RouteQueryHandler;

impl RouteQueryHandler {
    /// Resolve the request against the registry.
    ///
    /// v0 semantics (AC3-v0): the candidate set is the granted driven set, so
    /// an empty granted driven set is the only no-match trigger — the router does not
    /// itself decide a message is unroutable (the non-empty-narrowing form is
    /// deferred to the BP4 ranker). Never returns an error.
    pub fn execute(registry: &dyn PlaybookRegistry, request: &RouteRequest) -> RouteResult {
        let candidates: Vec<CandidateMeta> = granted_driven_candidates(registry, &request.ctx)
            .into_iter()
            .map(|m| CandidateMeta {
                kind: m.kind.clone(),
                description: m
                    .route
                    .description
                    .as_deref()
                    .filter(|description| !description.trim().is_empty())
                    .unwrap_or(&m.description)
                    .to_string(),
                required_fields: m.required_fields.iter().map(|f| f.name.clone()).collect(),
            })
            .collect();

        if candidates.is_empty() {
            RouteResult::NoMatch {
                handoff: CANDIDATE_PLAYBOOK_INTAKE.to_string(),
                intent: request.message.clone(),
            }
        } else {
            RouteResult::Candidates(candidates)
        }
    }
}

// ===========================================================================
// Resume-aware routing (resume_aware_routing track, Phases 2 & 3).
// ===========================================================================

/// The fixed set of continuation tokens (req #3): a bare message that, when an
/// open playbook exists for the conversation, signals "keep going" rather than a
/// fresh intent. Compared after trimming + lowercasing. Exact membership only —
/// no fuzzy matching, so a new-intent message can never be mis-read as resume
/// (req #4).
const CONTINUATION_TOKENS: &[&str] = &[
    "go",
    "continue",
    "proceed",
    "next",
    "yes",
    "ok",
    "keep going",
];

/// Whether `message` is a continuation signal — its trimmed, lowercased form is
/// exactly one of [`CONTINUATION_TOKENS`]. Empty / near-empty messages are not a
/// signal (the hooks layer suppresses them before route; they are not
/// observable). Pure predicate (req #3).
pub fn is_continuation_token(message: &str) -> bool {
    let normalized = message.trim().to_lowercase();
    CONTINUATION_TOKENS.contains(&normalized.as_str())
}
// ===========================================================================
// Continuation lexicons (continuation_recognition, spec r12, plan phase 2).
//
// The seven exact tokens above stay exactly as they are — this widens the path,
// it does not replace the exact-match one. What follows is the pure vocabulary
// the ordered decision procedure composes. Every set below was shaped by a
// reviewer producing a counter-example against an earlier revision; the comments
// say which, because the reason a token is in or out is the only thing that
// stops it being "tidied" back later.
// ===========================================================================

/// Never substitute prior context for a message carrying any of these.
/// Deliberately OVER-BROAD: a missed continuation costs one turn of guidance,
/// while a mis-routed rejection actions work the human explicitly refused.
/// `other` and `actually` are known-noisy and kept anyway — deciding when
/// `actually` is a correction and when it is an intensifier is the semantic
/// judgement this recogniser exists to avoid making.
const REJECTION_MARKERS: &[&str] = &[
    "no", "not", "don't", "dont", "stop", "abandon", "cancel", "never", "instead", "actually",
    "wrong", "nope", "other",
];

/// Deferral suppresses substitution exactly as rejection does, but is NOT
/// recorded as a rejection — deferring is not refusing, and conflating them
/// would corrupt the rejection rate.
///
/// `first` and `maybe` are deliberately ABSENT. They were here in revision 6 and
/// silenced "First, run the tests." and "Fix this first." — ordinary sequencing
/// language, not deferral.
const DEFERRAL_MARKERS: &[&str] =
    &["later", "wait", "hold", "pause", "break", "confused", "unsure"];

/// Asserts continuing the CURRENT work, so a proposal must never preempt it.
/// This is the half of the seven tokens that names its own object; `yes` and
/// `ok` answer a question somebody else asked.
const RESUMPTION_TOKENS: &[&str] = &["continue", "proceed", "next", "keep"];

/// The vocabulary a message may consist ENTIRELY of and still be bare agreement.
/// `going` is absent: "Good going." is praise, and with it in the set a proposal
/// could redirect a route on the strength of a compliment.
const CONSENT_TOKENS: &[&str] = &["yes", "yeah", "yep", "ok", "okay", "go", "sure"];
const PROCEED_TOKENS: &[&str] = &["do"];
const PRO_FORM_TOKENS: &[&str] = &["it", "that", "this", "them", "those"];
/// `and` and `then` are absent: a conjunction is exactly the token that turns
/// agreement into a compound instruction ("Do this and then that").
const FILLER_TOKENS: &[&str] = &[
    "great", "perfect", "good", "sounds", "right", "nice", "please", "thanks", "let's", "lets",
];
/// Adverbial modifiers intensify or time a request without naming anything new,
/// so they cannot turn agreement into an instruction. Without this class,
/// "Yes, do it now" — plain consent — declined and widened the WRONG kind.
const MODIFIER_TOKENS: &[&str] = &[
    "now",
    "ahead",
    "again",
    "soon",
    "away",
    "just",
    "already",
    "finally",
    "quickly",
    "definitely",
    "absolutely",
];

/// Positive evidence that the human is asking for work to continue. Five of
/// these are evidenced by the measured miss corpus ("Great do it", "continue
/// please", "let's get them fixed please", "Continue from where you left off");
/// the rest are a hand extension and are UNVALIDATED until a week of live
/// `widened_no_proposal` rates says otherwise.
const DIRECTIVE_TOKENS: &[&str] = &[
    "go", "going", "do", "doing", "continue", "proceed", "next", "keep", "finish", "ship", "fix",
    "fixed", "start", "run", "send", "make", "resume", "carry",
];
/// Evaluation is not a request. "Good point." judges something; it does not ask
/// for anything, and a flat affirmation set resumed on it.
const EVALUATIVE_TOKENS: &[&str] = &["great", "perfect", "good", "right", "sounds", "nice"];

/// WH words lead a question on their own.
const WH_TOKENS: &[&str] = &["what", "why", "how", "when", "where", "who", "which"];
/// Auxiliaries are interrogative ONLY before a pronoun — that two-token shape is
/// what keeps "Will do" an answer while "Do you continue" is a question, a
/// distinction no single-token test can make in both directions at once.
const AUXILIARY_TOKENS: &[&str] = &[
    "do", "does", "did", "is", "are", "was", "were", "can", "could", "should", "would", "has",
    "have", "will", "may", "might", "shall", "must", "am",
];
/// `it` is deliberately absent: "do it" and "do that" are the commonest
/// imperatives in the corpus, and no English question begins "do it".
const QUESTION_PRONOUNS: &[&str] = &["you", "i", "we", "they", "he", "she", "that", "this"];

/// The word bound for "short". Never sufficient alone.
pub const CONTINUATION_WORD_BOUND: usize = 6;

fn is_unicode_punct(c: char) -> bool {
    matches!(
        c,
        '\u{00A1}'
            | '\u{00BF}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{2018}'
            | '\u{2019}'
            | '\u{201C}'
            | '\u{201D}'
            | '\u{2026}'
            | '\u{2022}'
    )
}

/// The ONE normalisation, used everywhere this design says "tokenise".
///
/// Typographic apostrophes are folded because a curly apostrophe from an Apple
/// keyboard is ordinary input and must be the same token as the ASCII form.
/// Splitting on `/` and dashes makes "no/problem" two tokens. Punctuation is
/// stripped from token EDGES only, so internal apostrophes survive.
pub fn continuation_tokens(message: &str) -> Vec<String> {
    let folded: String = message
        .chars()
        .map(|c| match c {
            '\u{2019}' | '\u{02BC}' => '\'',
            _ => c,
        })
        .collect();
    folded
        .to_lowercase()
        .split(|c: char| {
            c.is_whitespace()
                || c == '/'
                || c == '\\'
                || c == '-'
                || ('\u{2010}'..='\u{2015}').contains(&c)
        })
        .map(|t| {
            t.trim_matches(|c: char| {
                (c.is_ascii_punctuation() && c != '\'') || is_unicode_punct(c)
            })
        })
        .map(|t| t.trim_matches('\'').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn any_in(tokens: &[String], set: &[&str]) -> bool {
    tokens.iter().any(|t| set.contains(&t.as_str()))
}

/// Word count under the shared normalisation.
pub fn continuation_word_count(message: &str) -> usize {
    continuation_tokens(message).len()
}

pub fn is_rejection(message: &str) -> bool {
    any_in(&continuation_tokens(message), REJECTION_MARKERS)
}

pub fn is_deferral(message: &str) -> bool {
    any_in(&continuation_tokens(message), DEFERRAL_MARKERS)
}

pub fn has_resumption_token(message: &str) -> bool {
    any_in(&continuation_tokens(message), RESUMPTION_TOKENS)
}

/// A message a prior proposal is allowed to preempt: one carrying no
/// independent action content. Bounds COUNTS as well as membership — one verb
/// and one referent is what "agreement to the thing on the table" looks like,
/// and a second of either means the message is directing work of its own.
pub fn is_pure_consent(message: &str) -> bool {
    let tokens = continuation_tokens(message);
    if tokens.is_empty() {
        return false;
    }
    let mut proceed = 0usize;
    let mut pro_form = 0usize;
    for t in &tokens {
        let t = t.as_str();
        if PROCEED_TOKENS.contains(&t) {
            proceed += 1;
        } else if PRO_FORM_TOKENS.contains(&t) {
            pro_form += 1;
        } else if !(CONSENT_TOKENS.contains(&t)
            || FILLER_TOKENS.contains(&t)
            || MODIFIER_TOKENS.contains(&t))
        {
            return false;
        }
    }
    proceed <= 1 && pro_form <= 1
}

/// A question is a request for information, never a continuation. Two guards,
/// neither positional: a `?` ANYWHERE (so trailing whitespace, a newline or an
/// emoji cannot defeat it), or an interrogative two-token shape.
pub fn is_question(message: &str) -> bool {
    if message.contains('?') {
        return true;
    }
    let tokens = continuation_tokens(message);
    let Some(first) = tokens.first().map(|t| t.as_str()) else {
        return false;
    };
    if WH_TOKENS.contains(&first) {
        return true;
    }
    if AUXILIARY_TOKENS.contains(&first) {
        if let Some(second) = tokens.get(1).map(|t| t.as_str()) {
            return QUESTION_PRONOUNS.contains(&second);
        }
    }
    false
}

/// Positive evidence for widening: a DIRECTIVE token, or a message that is
/// wholly agreement. Never the mere absence of objection — under that rule
/// "Hold on." returned resume guidance.
pub fn is_affirmative(message: &str) -> bool {
    if is_deferral(message) || is_question(message) {
        return false;
    }
    let tokens = continuation_tokens(message);
    if tokens.is_empty() {
        return false;
    }
    if any_in(&tokens, DIRECTIVE_TOKENS) {
        return true;
    }
    tokens.iter().all(|t| {
        let t = t.as_str();
        CONSENT_TOKENS.contains(&t)
            || EVALUATIVE_TOKENS.contains(&t)
            || PRO_FORM_TOKENS.contains(&t)
            || FILLER_TOKENS.contains(&t)
            || MODIFIER_TOKENS.contains(&t)
    })
}


/// The call-state of a single route turn — the honest-adoption classification
/// (Layer 1). Every route turn is exactly one of these so coverage and
/// start-conversion are computed over the correct denominators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    /// The router found no relevant playbook (the `no_match` outcome).
    NoPlaybook,
    /// A playbook is relevant AND the conversation has no open (non-terminal)
    /// playbook — the agent should begin.
    StartOpportunity,
    /// The conversation already has an open (non-terminal) playbook — the call
    /// should continue it, not begin a new one.
    MidPlaybookRun,
    /// The turn was answered by resuming an open playbook run.
    Resume,
}

impl CallState {
    /// The stable string form persisted on the durable route record and folded
    /// by ActivitySummary.
    pub fn as_str(&self) -> &'static str {
        match self {
            CallState::NoPlaybook => "no_playbook_run",
            CallState::StartOpportunity => "start_opportunity",
            CallState::MidPlaybookRun => "mid_playbook_run",
            CallState::Resume => "resume",
        }
    }
}

/// Classify a route turn into its [`CallState`] (pure, req #1). A `no_match`
/// outcome is always [`CallState::NoPlaybook`]; a matched outcome is
/// [`CallState::MidPlaybookRun`] ONLY when the conversation has an open
/// (non-terminal) playbook whose kind is RELEVANT to this turn — i.e. it appears
/// in `matching_candidates` (resume-signal context-awareness). A matched turn
/// that is UNRELATED to the open playbook (the agent moved to different work) is
/// NORMAL routing — [`CallState::StartOpportunity`], never `MidPlaybookRun` — so the
/// honest-adoption denominator does not count a context-blind resume as
/// mid-playbook continuation.
///
/// `open_playbook_run` is the result of [`find_open_playbook_run_for_conversation`],
/// which already excludes terminal artifacts AND returns `None` for an empty
/// conversation id — so a terminal-only conversation and an empty conversation
/// both classify as `StartOpportunity` (never `MidPlaybookRun`) by construction.
/// `matching_candidates` is the turn's relevant kind set (empty for `no_match`).
pub fn classify_call_state(
    outcome: &RouteOutcome,
    open_playbook_run: Option<&OpenPlaybookRun>,
    matching_candidates: &[String],
) -> CallState {
    match outcome {
        RouteOutcome::NoMatch => CallState::NoPlaybook,
        RouteOutcome::Single | RouteOutcome::Candidates => {
            match open_playbook_run {
                // Mid-playbook ONLY when the open playbook is relevant to this
                // matched turn. An unrelated matched turn is a start opportunity.
                Some(open) if matching_candidates.iter().any(|k| k == &open.kind) => {
                    CallState::MidPlaybookRun
                }
                _ => CallState::StartOpportunity,
            }
        }
    }
}

/// Whether `open`'s kind is RELEVANT to a matched turn — i.e. it appears in the
/// turn's `matching_candidates`. The relevance gate on the post-resolution resume
/// nudge (resume-signal context-awareness): an open playbook is resumed only when
/// the current turn relates to it; an unrelated matched turn suppresses the resume
/// and routes normally.
pub fn open_playbook_run_is_relevant(open: &OpenPlaybookRun, matching_candidates: &[String]) -> bool {
    matching_candidates.iter().any(|k| k == &open.kind)
}

/// Render the machine-declared PARK (abandon) action for `(kind, state)` — the
/// outgoing transition whose `to_state` is `abandoned`, formatted as
/// `"snapshot → abandoned (role: <role>)"`. Parking is snapshot-driven (an
/// abandon edge is satisfaction-gated so `complete` never selects it), so the
/// rendered action names `snapshot`. Empty when the state declares no abandon
/// edge or the kind is unknown.
pub fn abandon_action_for(registry: &dyn PlaybookRegistry, kind: &str, state: &str) -> String {
    let Some(machine) = registry.machine_for(kind) else {
        return String::new();
    };
    outgoing_transitions(machine, state)
        .into_iter()
        .find(|t| t.to_state == "abandoned")
        .map(|t| format!("snapshot → abandoned (role: {})", t.required_role))
        .unwrap_or_default()
}

/// An open (begun, not-yet-terminal) playbook recorded for a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPlaybookRun {
    /// The artifact's id (directory name).
    pub artifact_id: String,
    /// The artifact's kind.
    pub kind: String,
    /// The artifact's current state.
    pub state: String,
}

/// Whether `state` is terminal for `kind`'s machine. Defaults to `false` when
/// the kind/state is unknown to the registry (an unknown state is treated as
/// non-terminal — a conservative "still open" default, never a false resume on
/// its own since the marker must also match).
pub fn state_is_terminal(registry: &dyn PlaybookRegistry, kind: &str, state: &str) -> bool {
    registry
        .machine_for(kind)
        .and_then(|m| m.states.iter().find(|s| s.name == state))
        .map(|s| s.is_terminal)
        .unwrap_or(false)
}

/// Render the supported advance action for `(kind, state)` — the first outgoing
/// transition's target + required role. Empty when the state has no outgoing
/// transition (terminal / no-exit) or the kind is unknown.
pub fn advance_action_for(registry: &dyn PlaybookRegistry, kind: &str, state: &str) -> String {
    let Some(machine) = registry.machine_for(kind) else {
        return String::new();
    };
    let transitions = outgoing_transitions(machine, state);
    match transitions.first() {
        Some(t) => format!("complete → {} (role: {})", t.to_state, t.required_role),
        None => String::new(),
    }
}

/// Find the conversation's open playbook (req #2). Over the enumerated artifacts,
/// select those whose open-begin marker's `conversation_id` matches AND whose
/// current state is NOT terminal, then return the most-recently-begun one
/// (begin marker `at` descending — deterministic multiplicity, req #2).
///
/// "Open" requires BOTH (a) a begin marker for this conversation that is not yet
/// closed (`has_open_begin` for the marker's actor+state) AND (b) the artifact's
/// current state is non-terminal — so a completed artifact whose marker happens
/// to be left unclosed by a terminal transition recorded by a different actor is
/// NOT treated as open (plan H2).
///
/// An empty `conversation_id` never matches (back-compat: pre-existing markers
/// with no conversation_id, and surfaces that omit it, simply never resume).
pub fn find_open_playbook_run_for_conversation(
    query: &dyn QueryPort,
    registry: &dyn PlaybookRegistry,
    conversation_id: &str,
) -> Result<Option<OpenPlaybookRun>, QueryError> {
    if conversation_id.trim().is_empty() {
        return Ok(None);
    }

    let mut matches: Vec<(String, OpenPlaybookRun)> = Vec::new();
    for (artifact_id, kind) in query.list_artifacts()? {
        if let Some(confirmed) = confirm_open_playbook_run_for_artifact(
            query,
            registry,
            conversation_id,
            &artifact_id,
            &kind,
        )? {
            matches.push(confirmed);
        }
    }

    Ok(most_recently_begun(matches))
}

/// Confirm whether ONE artifact is an open (begun, non-terminal) playbook for
/// `conversation_id`, returning `(begun_at, OpenPlaybookRun)` when it is. This is
/// the single per-artifact confirmation step shared by BOTH the full scan
/// ([`find_open_playbook_run_for_conversation`]) and the indexed lookup
/// ([`find_open_playbook_run_indexed`]) so the two paths cannot disagree.
///
/// "Open" requires BOTH (a) a begin marker for this conversation that is not yet
/// closed (`has_open_begin` for the marker's actor+state) AND (b) the artifact's
/// CURRENT state is non-terminal — the index supplies CANDIDATES, this function
/// supplies the VERDICT by reading the candidate fresh, so a stale candidate id
/// can never yield a wrong "open" answer (it just reconfirms terminal/closed →
/// `None`).
fn confirm_open_playbook_run_for_artifact(
    query: &dyn QueryPort,
    registry: &dyn PlaybookRegistry,
    conversation_id: &str,
    artifact_id: &str,
    kind: &str,
) -> Result<Option<(String, OpenPlaybookRun)>, QueryError> {
    let activity = query.read_activity_entries(artifact_id)?;
    let transitions = query.read_transitions(artifact_id)?;

    // The begin markers for THIS conversation that are still open
    // (marker present + no later closing transition by the marker's actor).
    // We track the most-recent matching marker `at` for tie-breaking.
    let mut newest_open_at: Option<String> = None;
    for entry in activity.iter() {
        if entry.kind != "begin" || entry.conversation_id != conversation_id {
            continue;
        }
        if !has_open_begin(&activity, &transitions, &entry.actor, &entry.state) {
            continue;
        }
        if newest_open_at
            .as_deref()
            .map(|cur| entry.at.as_str() > cur)
            .unwrap_or(true)
        {
            newest_open_at = Some(entry.at.clone());
        }
    }

    let Some(begun_at) = newest_open_at else {
        return Ok(None);
    };

    // (b) The artifact's CURRENT state must be non-terminal.
    let state = query.read_artifact_state(artifact_id)?;
    if state_is_terminal(registry, kind, &state) {
        return Ok(None);
    }

    Ok(Some((
        begun_at,
        OpenPlaybookRun {
            artifact_id: artifact_id.to_string(),
            kind: kind.to_string(),
            state,
        },
    )))
}

/// Most-recently-begun wins (begin `at` desc); ties break by id desc for
/// determinism (list_artifacts already sorts by id asc).
fn most_recently_begun(mut matches: Vec<(String, OpenPlaybookRun)>) -> Option<OpenPlaybookRun> {
    matches.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.artifact_id.cmp(&a.1.artifact_id))
    });
    matches.into_iter().next().map(|(_, w)| w)
}

/// Indexed open-playbook lookup — the cheap path that reads ONLY the supplied
/// `candidates` (the artifact ids the open-marker index recorded for this
/// `conversation_id`) instead of enumerating EVERY artifact in the hearth.
///
/// CRITICAL DESIGN (candidates, not verdicts + fail-open): `candidates` is the
/// index's best guess at which artifacts MIGHT carry an open begin marker for
/// this conversation; this function CONFIRMS each one fresh (open begin marker +
/// non-terminal current state) so index staleness can never produce a wrong
/// answer — a candidate that has since gone terminal or been closed simply
/// confirms to `None`. It never calls `list_artifacts`, so the common path pays
/// only the candidate reads.
///
/// Fail-open is the CALLER's job: `candidates == None` means "cold / nothing
/// indexed yet", and the caller must degrade to
/// [`find_open_playbook_run_for_conversation`] (a full scan that also rebuilds the
/// index). This function never falls back itself — it only resolves what the
/// caller hands it — so a `Some(empty)` candidate set is an explicit "the index
/// knows this conversation has no open playbook" and resolves to `None` WITHOUT
/// a scan.
///
/// Semantics are identical to the scan: empty `conversation_id` → `None`,
/// terminal excluded, most-recently-begun on multiplicity. A candidate id the
/// adapter cannot read (e.g. a since-deleted artifact) is skipped, not an error.
pub fn find_open_playbook_run_indexed(
    query: &dyn QueryPort,
    registry: &dyn PlaybookRegistry,
    conversation_id: &str,
    candidates: &[String],
) -> Result<Option<OpenPlaybookRun>, QueryError> {
    if conversation_id.trim().is_empty() {
        return Ok(None);
    }

    let mut matches: Vec<(String, OpenPlaybookRun)> = Vec::new();
    for artifact_id in candidates {
        // Resolve the candidate's kind; a candidate that no longer exists is a
        // stale index entry → skip it (fail-open, never an error on lookup).
        let kind = match query.read_artifact_kind(artifact_id) {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        if let Some(confirmed) = confirm_open_playbook_run_for_artifact(
            query,
            registry,
            conversation_id,
            artifact_id,
            &kind,
        )? {
            matches.push(confirmed);
        }
    }

    Ok(most_recently_begun(matches))
}

// ===========================================================================
// prior_proposal — SYNTACTIC extraction (continuation_recognition, plan phase 3)
//
// Shape only. This decides whether the last assistant turn LOOKS like a single
// unambiguous proposal; it never decides what that proposal MEANS. Resolving
// <X> to a granted kind needs the engine's trigger matcher, and building a
// second matcher here is the one duplication the plan explicitly forbids — so
// zero/multi-kind rejection and the `Next:` trigger-position guard live in the
// engine, not in this function.
//
// When in doubt, OMIT. Many genuine proposals will not match and will simply
// resume as they do today. Recall can be widened later against a measured miss
// rate; a wrong substitution cannot be taken back.
// ===========================================================================

/// A proposal the assistant made on its last turn, extracted by shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorProposal {
    /// The matched `<X>`, verbatim after normalisation of surrounding syntax.
    pub text: String,
    /// Set AT CUT TIME, never inferred. `truncate` appends an ellipsis that is
    /// indistinguishable from an author's own, and the tail window leaves no
    /// trace at all — so a truncated turn must be MARKED when it is cut, not
    /// guessed at afterwards.
    pub was_truncated: bool,
}

/// The proposal forms. All five assert that the SPEAKER proposes to act, which
/// is what separates a proposal from a report.
///
/// `Next: <X>` was dropped in spec revision 5 for admitting "Next: deployment
/// failed", and restored in revision 6 when the removal turned out to cost more
/// than it saved: with no form matching, a following "yes" resumed the WRONG
/// kind — moving a case from declined to wrongly answered. It comes back with
/// structural guards instead.
const PROPOSAL_FORMS: &[(&str, &str)] = &[
    ("want me to ", "?"),
    ("shall i ", "?"),
    ("i'd take ", ""),
    ("next: ", ""),
];

/// Word markers apply to the CANDIDATE LINE only — unresolved options *in the
/// proposal*. Revision 5 widened this to the whole turn and rejected a singular
/// explicit proposal preceded by ordinary explanatory prose ("The failure could
/// be routing or telemetry. / Shall I start a track?"), which assistants write
/// constantly.
const ENUM_WORD_MARKERS: &[&str] = &["either", "or", "versus", "vs"];

/// The straight apostrophe is NOT here: it is a contraction mark, and every
/// `I'd take <X>` form carries one. Guarding on it would have silently disabled
/// two of the five proposal forms — a whole class of proposals declined for a
/// reason no test named.
const QUOTE_CHARS: &[char] = &['"', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'];

/// A list prefix, read from the TRIMMED RAW line — before the shared tokeniser,
/// which strips exactly the punctuation that marks one. Revision 5 specified
/// these as tokenised and would have had every advertised case fail.
fn is_list_prefix_line(raw: &str) -> bool {
    let t = raw.trim_start();
    let mut chars = t.chars();
    match chars.next() {
        // Bullets: a marker followed by whitespace.
        Some('-') | Some('*') | Some('\u{2022}') => {
            matches!(chars.next(), Some(c) if c.is_whitespace())
        }
        // Ordered: digits or a single letter, then `.` or `)`, then whitespace.
        // A PATTERN, not the literal `1.` `2.` `3.` of revision 5 — which missed
        // `4.`, `1)`, `A.` and every bullet a reviewer produced.
        Some(c0) if c0.is_ascii_digit() || c0.is_ascii_alphabetic() => {
            let label_len = if c0.is_ascii_digit() {
                t.chars().take_while(|c| c.is_ascii_digit()).count()
            } else {
                1
            };
            let mut rest = t.chars().skip(label_len);
            matches!(rest.next(), Some('.') | Some(')'))
                && matches!(rest.next(), Some(c) if c.is_whitespace())
        }
        _ => false,
    }
}

/// Extract a prior proposal from an assistant turn's text, by shape alone.
///
/// `was_truncated` is supplied by the CALLER, which is the only place that
/// knows whether this turn was cut — by the 200-char digest cap or by the tail
/// window. A truncated turn is disqualified outright: a cut proposal is not a
/// proposal.
pub fn extract_prior_proposal(turn_text: &str, was_truncated: bool) -> Option<PriorProposal> {
    if was_truncated {
        return None;
    }
    let lines: Vec<&str> = turn_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let candidate_raw = lines.last()?.trim();

    // Structural disqualifiers on the RAW line.
    if candidate_raw.starts_with('#') || candidate_raw.contains('|') {
        return None;
    }
    if candidate_raw.chars().any(|c| QUOTE_CHARS.contains(&c)) {
        return None;
    }
    // A list prefix ANYWHERE in the turn is a real enumeration.
    if lines.iter().any(|l| is_list_prefix_line(l)) {
        return None;
    }

    let lower = candidate_raw.to_lowercase();

    // Word enumeration markers, on the candidate line only, tokenised so
    // "corridor" is not "or".
    let candidate_tokens = continuation_tokens(candidate_raw);
    if candidate_tokens
        .iter()
        .any(|t| ENUM_WORD_MARKERS.contains(&t.as_str()))
    {
        return None;
    }
    // A rejection marker on the candidate line kills it — reusing the one set
    // rather than inventing a second negation vocabulary. This is what rejects
    // "Next: do not amend it".
    if candidate_tokens
        .iter()
        .any(|t| REJECTION_MARKERS.contains(&t.as_str()))
    {
        return None;
    }

    // The form must consume the WHOLE line, and exactly one form may match.
    let mut matched: Option<String> = None;
    let mut match_count = 0usize;
    for (prefix, suffix) in PROPOSAL_FORMS {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let body = if suffix.is_empty() {
                if rest.ends_with('?') {
                    continue;
                }
                rest
            } else {
                match rest.strip_suffix(suffix) {
                    Some(b) => b,
                    None => continue,
                }
            };
            let body = body.trim();
            if body.is_empty() {
                continue;
            }
            match_count += 1;
            if matched.is_none() {
                let start = candidate_raw.len() - rest.len();
                let end = start + body.len();
                matched = Some(candidate_raw[start..end].trim().to_string());
            }
        }
    }
    // "I'd <X> next" — the trailing-marker form, handled separately so the
    // whole-line rule still holds.
    if let Some(rest) = lower.strip_prefix("i'd ") {
        if let Some(body) = rest.strip_suffix(" next") {
            let body = body.trim();
            if !body.is_empty() && !body.starts_with("take ") {
                match_count += 1;
                if matched.is_none() {
                    let start = candidate_raw.len() - rest.len();
                    matched = Some(candidate_raw[start..start + body.len()].trim().to_string());
                }
            }
        }
    }

    // Count matching LINES, not matching forms: "I'd take telemetry next"
    // matches two forms on ONE line and is therefore one match, not two.
    if match_count == 0 {
        return None;
    }
    matched.map(|text| PriorProposal {
        text,
        was_truncated: false,
    })
}

// ===========================================================================
// The continuation decision procedure (continuation_recognition, spec r12).
//
// ONE ordered procedure, first match wins. The order IS the design: it was wrong
// twice across the review series in ways that defeated the track's own premise,
// and both fixes are load-bearing.
//   - New intent precedes rejection, so "Actually start a track for the parser"
//     routes on its own text and is NOT counted as a rejected continuation.
//   - Proposal comparison precedes the bare-token path, so "yes" answering
//     "Shall I record this as a decision?" reaches the DECISION rather than
//     resuming the open track.
//
// Predicates overlap by design; precedence resolves them. What matters is that
// the procedure is TOTAL (step 8 catches everything) and DETERMINISTIC.
// ===========================================================================

/// What the procedure decided, and the `resume_source` each outcome records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationOutcome {
    /// Not a continuation turn — route normally (steps 0, 1, 2, 3, 8).
    RouteNormally,
    /// A rejection or deferral declined the contextual path. Routing proceeds
    /// normally; only SUBSTITUTION is suppressed.
    Rejected,
    /// Resume the open run because the proposal names its kind (step 5).
    ResumeContextual,
    /// Route to a DIFFERENT kind the assistant proposed (step 5).
    Redirect { kind: String },
    /// Today's exact-token resume (step 6).
    Resume,
    /// Resume on affirmative evidence with no usable proposal (step 7).
    ResumeWidened,
}

impl ContinuationOutcome {
    pub fn resume_source(&self) -> &'static str {
        match self {
            ContinuationOutcome::RouteNormally => "",
            ContinuationOutcome::Rejected => "rejected",
            ContinuationOutcome::ResumeContextual => "prior_proposal",
            ContinuationOutcome::Redirect { .. } => "prior_proposal_redirect",
            ContinuationOutcome::Resume => "continuation_token",
            ContinuationOutcome::ResumeWidened => "widened_no_proposal",
        }
    }
}

/// Everything the procedure needs, gathered by the caller so this stays pure.
pub struct ContinuationInputs<'a> {
    pub message: &'a str,
    /// Whether the conversation has an open (begun, non-terminal) playbook —
    /// from the engine's own marker lookup, never a transcript inference.
    pub has_open_run: bool,
    /// The open run's kind, when there is one.
    pub open_kind: &'a str,
    /// Does the ORIGINAL message match a trigger in the granted set? The
    /// engine's own matcher answers this; a second matcher would drift.
    pub has_new_intent: bool,
    /// The hook looked at a transcript tail (presence is load-bearing: it is a
    /// different fact from having no tail at all).
    pub has_recent_context: bool,
    /// The proposal's resolved kind, when the shape matched AND the engine's
    /// matcher resolved `<X>` to EXACTLY ONE granted kind. `None` for zero or
    /// several — ambiguity declines, it never picks.
    pub proposal_kind: Option<&'a str>,
}

/// Evaluate the ordered procedure. Steps are numbered as in the spec so a
/// failing test names the branch it exercises.
pub fn resolve_continuation(input: &ContinuationInputs<'_>) -> ContinuationOutcome {
    // 0. EMPTY — an empty message is not short, it is empty.
    let tokens = continuation_tokens(input.message);
    if tokens.is_empty() {
        return ContinuationOutcome::RouteNormally;
    }
    // 1. SHORT — never sufficient alone.
    if tokens.len() > CONTINUATION_WORD_BOUND {
        return ContinuationOutcome::RouteNormally;
    }
    // 2. OPEN PLAYBOOK — no open run, nothing to continue.
    if !input.has_open_run {
        return ContinuationOutcome::RouteNormally;
    }
    // 3. NEW INTENT — routes on its OWN text. Before rejection, so a message
    //    like "Actually start a track for X" is not miscounted as a rejection.
    if input.has_new_intent {
        return ContinuationOutcome::RouteNormally;
    }
    // 4. REJECTION — suppresses SUBSTITUTION only; the caller still routes the
    //    message normally.
    if is_rejection(input.message) {
        return ContinuationOutcome::Rejected;
    }
    // 5. PROPOSAL — only PURE CONSENT may be preempted by a proposal, and only
    //    when the message neither asserts its own resumption nor defers.
    if let Some(kind) = input.proposal_kind {
        if !has_resumption_token(input.message)
            && !is_deferral(input.message)
            && !is_question(input.message)
            && is_pure_consent(input.message)
        {
            return if kind == input.open_kind {
                ContinuationOutcome::ResumeContextual
            } else {
                ContinuationOutcome::Redirect {
                    kind: kind.to_string(),
                }
            };
        }
    }
    // 6. BARE TOKEN — today's path, unchanged.
    if is_continuation_token(input.message) {
        return ContinuationOutcome::Resume;
    }
    // 7. AFFIRMATIVE — positive evidence only, and only when the hook actually
    //    looked at a tail. Without the context guard, transcript-less surfaces
    //    would resume where they do not today.
    //
    //    A RESUMPTION token overrides the deferral guard, because in
    //    "Later—keep going" the deferral applies to the PROPOSAL and the
    //    resumption applies to the work: the human deferred the offer and asked
    //    to carry on. A question still blocks unconditionally — "Pause, then
    //    continue?" is asking, not directing.
    if input.has_recent_context && !is_question(input.message) {
        if has_resumption_token(input.message) || is_affirmative(input.message) {
            return ContinuationOutcome::ResumeWidened;
        }
    }
    // 8. Total.
    ContinuationOutcome::RouteNormally
}
