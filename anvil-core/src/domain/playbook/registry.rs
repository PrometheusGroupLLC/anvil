//! `PlaybookRegistry` port — resolution boundary for kind-name → `PlaybookMachine`.
//!
//! This port lives at the **consumer boundary**, not inside the interpreter.
//! The interpreter (`interpreter::outgoing_transitions`) takes `&PlaybookMachine`
//! directly — it stays pure and unaware of registries. The registry is the
//! seam a consumer (e.g., `describe::available_actions`) uses to resolve a
//! kind-name before calling the interpreter.
//!
//! This CQRS separation means Track #14's hearth-loader adapter can swap in
//! without touching any consumer: the consumer holds `&dyn PlaybookRegistry`,
//! calls `.machine_for("track")`, passes the result to the interpreter.
//!
//! ## Lifetime note
//!
//! `SeedPlaybookRegistry::machine_for` returns `Option<&'static PlaybookMachine>`
//! because the compiled-in seeds are `&'static`. The trait method's lifetime is
//! tied to `&self` (`Option<&'a PlaybookMachine>`) so future adapters backed by
//! owned data can return references with the adapter's lifetime. The `'static`
//! seed lifetime satisfies `'a` at any call site.

use crate::domain::playbook::seeds::{
    backlog_item_seed, decision_seed, initiative_seed, learning_seed, milestone_seed,
    playbook_seed, proposal_seed, spark_seed, track_seed,
};
use crate::domain::playbook::types::{Access, PlaybookMachine};
use crate::domain::shared_types::RequestContext;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookSource {
    pub hearth: Option<PathBuf>,
    pub playbook_id: String,
}

/// Canonical kind for the playbook-authoring meta-playbook.
///
/// Renamed from the legacy `workflow_generation` (non-breaking: the on-disk
/// `directory: workflow_generations` / `registry: workflow_generations.md`
/// contract is deliberately UNCHANGED — a2a51cf protected it — so no storage
/// splits and existing artifacts stay put). The two names denote the SAME
/// playbook; see [`playbook_generation_aliases`].
pub const PLAYBOOK_GENERATION_KIND: &str = "playbook_generation";

/// The set of kind strings that denote the playbook-generation playbook, for a
/// given `kind`. Resolution is BIDIRECTIONAL and order-stable (canonical first):
/// a hearth's `machine.yaml` may carry either name on its `kind:`, and existing
/// on-disk artifacts carry `playbook_generation` on their `status.yaml`. Asking
/// for either name must resolve the machine whichever name the hearth stored it
/// under — so both renamed and not-yet-renamed hearths keep working. Returns
/// empty for every other kind.
pub fn playbook_generation_aliases(kind: &str) -> &'static [&'static str] {
    match kind {
        "playbook_generation" | "workflow_generation" => {
            &["playbook_generation", "workflow_generation"]
        }
        _ => &[],
    }
}

/// True if `kind` names the playbook-generation playbook under EITHER name
/// (canonical `playbook_generation` or legacy `workflow_generation`).
pub fn is_playbook_generation_kind(kind: &str) -> bool {
    matches!(kind, "playbook_generation" | "workflow_generation")
}

/// Port: resolves an artifact kind name to its compiled-in or loaded
/// `PlaybookMachine`.
///
/// Consumers depend on this trait; adapters implement it. Today's adapter is
/// `SeedPlaybookRegistry`. Track #14 will introduce a hearth-loader adapter
/// that reads on-disk `machine.yaml` files.
pub trait PlaybookRegistry {
    /// Return the `PlaybookMachine` for `kind`, or `None` if unknown.
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a;

    /// Return the on-disk playbook artifact id for `kind`, or `None` if unknown.
    ///
    /// The id is the directory name under `{hearth}/playbooks/` that holds the
    /// machine's `machine.yaml` + `hooks/`. Consumers use it as the
    /// `playbook_id` argument to `QueryPort::read_playbook_hook_body` when
    /// serving a state's declared hook body. Returned owned (not a borrow) so
    /// the trait stays object-safe across adapters that synthesize the id.
    fn playbook_id_for(&self, kind: &str) -> Option<String>;

    fn source_for(&self, kind: &str) -> Option<PlaybookSource> {
        self.playbook_id_for(kind)
            .map(|playbook_id| PlaybookSource {
                hearth: None,
                playbook_id,
            })
    }

    /// Enumerate every machine this registry can resolve.
    ///
    /// This is the candidate-set data source the in-engine router consumes
    /// (`playbook_routing_layer` BP0). For the hearth adapter, only
    /// successfully-loaded machines are returned — a malformed `machine.yaml`
    /// never enters the map and is therefore absent from this enumeration
    /// (this is what makes "active candidacy = registry-resolvability" hold).
    ///
    /// The default returns an empty list so test doubles that do not exercise
    /// enumeration need no boilerplate; the production adapters override it.
    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        Vec::new()
    }

    /// Enumerate the kinds this registry can resolve.
    ///
    /// Provided in terms of `all_machines` so adapters implement only the
    /// reference-returning method. The order mirrors `all_machines`.
    fn kinds(&self) -> Vec<String> {
        self.all_machines().iter().map(|m| m.kind.clone()).collect()
    }

    /// Why WRITING a definition into this registry's hearth is unsafe, if it is.
    ///
    /// C9's `registration_blocked()` reached this port so a WRITER can ask
    /// before it writes. It previously existed only on the hearth adapter's own
    /// inherent impl, where nothing in production called it — which made "the
    /// loader reports registration as blocked" a report rather than a block, and
    /// reporting a failure is not blocking a writer.
    ///
    /// `Some(detail)` means the canonical root is not the root this hearth's
    /// definitions will be read from (a failed top-level move, or both roots
    /// present), so a write here produces an artifact nothing serves. A single
    /// malformed `machine.yaml` is NOT this: that artifact is invalid, the
    /// hearth is fine.
    ///
    /// The default is `None` — an in-memory or compiled-in registry has no
    /// hearth directory to be wrong about — so test doubles need no boilerplate.
    fn registration_blocked_detail(&self) -> Option<String> {
        None
    }
}

/// Filter a registry's enumeration to its driven candidates.
///
/// The in-engine router (`playbook_routing_layer`) excludes `register: free`
/// machines (spark/decision/learning) from its candidate set — they are
/// invocable out-of-band, never a route target. This is the single seam the
/// router and the `anvil_orchestrate` begin-mode driven-kind guard both use.
pub fn driven_candidates(registry: &dyn PlaybookRegistry) -> Vec<&PlaybookMachine> {
    registry
        .all_machines()
        .into_iter()
        .filter(|m| m.is_driven())
        .collect()
}

/// Pure four-axis access predicate for playbook candidate scoping.
pub fn granted(ctx: &RequestContext, access: &Access) -> bool {
    let org_granted = access.org.is_empty() || access.org == "Foundation" || ctx.org == access.org;
    let role_granted = ctx.role >= access.min_role;
    let sensitivity_granted = ctx.clearance >= access.sensitivity;
    let space_granted = access.space.as_ref().map_or(true, |space| {
        ctx.space
            .as_ref()
            .is_some_and(|ctx_space| ctx_space == space)
    });

    org_granted && role_granted && sensitivity_granted && space_granted
}

/// Driven candidates filtered by the pure access predicate.
pub fn granted_driven_candidates<'a>(
    registry: &'a dyn PlaybookRegistry,
    ctx: &RequestContext,
) -> Vec<&'a PlaybookMachine> {
    driven_candidates(registry)
        .into_iter()
        .filter(|m| granted(ctx, &m.access))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteResolution {
    pub granted_candidates: Vec<String>,
    /// Lexical ranking inputs for every granted candidate, before trigger-regime
    /// filtering and shortlist capping.
    pub full_granted_signals: Vec<RouteCandidateSignal>,
    /// Why lexical routing abstained. Hard safety abstentions must never be
    /// reopened by later recall experiments; floor misses may be.
    pub abstention_reason: Option<RouteAbstentionReason>,
    /// Candidates tied at the winning trigger specificity, sorted by kind.
    pub matching_candidates: Vec<String>,
    pub selected_kind: Option<String>,
    pub outcome: RouteOutcome,
    /// Per-matching-candidate concrete match signal (H1, route_response_mirrors_begin
    /// Phase 2): the matched trigger phrase (trigger regime) or the
    /// description-overlap term(s) (description regime) that produced the match.
    /// Keyed by kind; only matching candidates appear. `why_fits` cites this so the
    /// annotation is checkable, not free prose. Empty in the no-match / free-kind
    /// abstain paths (no matching candidate carries a signal).
    pub match_signals: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAbstentionReason {
    HardAbstain,
    FloorMiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCandidateSignal {
    pub kind: String,
    pub trigger_tier: usize,
    pub content_overlap: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOutcome {
    Single,
    Candidates,
    NoMatch,
}

/// Maximum matching candidates surfaced for a non-single route decision.
const CANDIDATE_CAP: usize = 3;

/// Minimum count of distinct shared content tokens for a candidate to be relevant
/// in the description regime (no trigger matched any granted candidate). A single
/// global integer floor (per spec out-of-scope: per-machine tuning is future work).
///
/// router_precision (adoption Track A, fix #3): raised from 1 → 2 so a single
/// shared COMMON token no longer qualifies a kind. A lone token can only survive
/// via the distinctive-single-token exception in [`clears_description_floor`]
/// (long, non-low-signal), which holds recall for genuine single-word intents
/// while cutting the description-overlap flood on incidental one-token coincidence.
/// Trigger matches (tier > 0) are unaffected — they never consult this floor.
const RELEVANCE_FLOOR: usize = 2;

/// Length at which a lone shared description token is treated as DISTINCTIVE
/// enough to clear the floor on its own (router_precision fix #3). Shorter
/// single-token overlaps are too likely to be incidental to justify surfacing a
/// candidate; longer ones (e.g. "recap", "topic", "invoice") carry real signal.
const DISTINCTIVE_SINGLE_TOKEN_LEN: usize = 5;

/// Minimum content-overlap for a lone description-regime candidate to AUTO-RESOLVE as
/// `Single` (which bypasses surface-LLM selection). A single weak (1-token) overlap is
/// surfaced as `Candidates` for the LLM to confirm rather than silently auto-routed —
/// precision guard for the most dangerous outcome. Trigger matches (tier > 0) always
/// auto-resolve regardless of this floor.
const SINGLE_SELECT_FLOOR: usize = 2;

/// Function words that MUST NOT contribute to description-overlap scoring (spec R10).
/// Kept deliberately small — articles, prepositions, conjunctions, common
/// auxiliaries/pronouns, and contentless question words.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "you", "your", "with", "this", "that", "from", "into", "our", "are",
    "was", "can", "will", "what", "whats", "how", "when", "where", "why", "who", "but", "not",
    "all", "any", "get", "got", "let", "lets", "its", "their", "them", "they", "have", "has",
    "had", "out", "off", "over", "than", "then", "there", "here", "about", "onto", "per", "via",
    "etc", "some",
];

/// Single-token coincidences too generic to make a candidate relevant by
/// themselves. Exact trigger phrases still route these intents; this only guards
/// the candidates branch from flooding on weak description overlap.
///
/// router_precision variant #1 (return 1-3 correct options): the dominant
/// real-trace failure is candidate-flooding on a lone GENERIC Lore-domain noun
/// that appears across many workflow descriptions — "topic"/"topics" span 6 Lore
/// workflows; "evidence"/"sources"/"lore" span several more. A single such token
/// is not a routing signal on its own; without this guard the resolver returns a
/// capped-but-useless 3-candidate menu the agent ignores. Adding them here
/// converts those weak single-token turns to floor-driven abstention (raising
/// abstention_correct) while leaving recall intact: a DISTINCTIVE single token
/// (e.g. "answer" -> lore_query) or any TWO-plus token overlap still clears the
/// floor and surfaces the correct option(s). Trigger phrases (tier > 0) route
/// these intents regardless of this list.
///
/// Deliberately NOT included: "knowledge". It is a genuine sole signal for the
/// knowledge_lifecycle workflow (a whole domain names itself with it), so
/// blacklisting it drops real matches — a recall regression, not a flood fix.
///
/// The floor (`RELEVANCE_FLOOR`) and this list are the tunable levers a variant
/// sweeps without a resolver rewrite.
const LOW_SIGNAL_DESCRIPTION_TOKENS: &[&str] = &[
    "across",
    "lifecycle",
    "new",
    "proposal",
    "run",
    // Generic Lore-domain nouns that flood the description regime on their own.
    "topic",
    "topics",
    "evidence",
    "sources",
    "lore",
];

/// Resolve a user message to a route outcome over the granted driven candidates.
///
/// Scoring (spec R1): each granted candidate gets an integer lexicographic tuple
/// `(trigger_tier, content_overlap)`. `trigger_tier` is the length of the longest
/// declared trigger phrase that substring-matches the message (0 if none);
/// `content_overlap` is the count of distinct content tokens shared between the
/// message and the candidate's `description` + `kind`.
///
/// Two regimes:
/// - **Trigger regime** (some candidate has `trigger_tier > 0`): the relevant set is
///   the candidates at the winning (max) trigger tier, ranked by trigger tier,
///   description overlap, then kind, and capped to the shortlist limit.
/// - **Description regime** (all `trigger_tier == 0`): the relevant set is the
///   candidates whose `content_overlap >= RELEVANCE_FLOOR`, ordered by overlap
///   descending then kind and capped to the shortlist limit (recall, R2).
///
/// Outcome: empty granted set → `NoMatch`; empty relevant set → `NoMatch`
/// (floor-driven abstention even when granted is non-empty, R3); one relevant →
/// `Single`; many relevant → `Candidates` carrying only the relevant set (narrowing,
/// R4). All scores are integers, so ordering and floor membership are deterministic
/// (R11).
pub fn resolve_route(
    registry: &dyn PlaybookRegistry,
    ctx: &RequestContext,
    input: &str,
) -> RouteResolution {
    let mut candidates: Vec<&PlaybookMachine> = granted_driven_candidates(registry, ctx)
        .into_iter()
        .filter(|m| !m.description.trim().is_empty())
        .collect();
    candidates.sort_by(|a, b| a.kind.cmp(&b.kind));

    let granted_candidates: Vec<String> = candidates.iter().map(|m| m.kind.clone()).collect();

    let message_tokens = content_tokens(input);

    // (kind, trigger_tier, content_overlap, signal) — integer tuple plus the
    // concrete, citable match signal (H1). In the trigger regime the signal is
    // the matched (longest, normalized) trigger phrase; in the description regime
    // it is the comma-joined sorted overlap terms. Empty when nothing matched.
    let scored: Vec<(String, usize, usize, String)> = candidates
        .iter()
        .map(|m| {
            let best_trigger = m
                .route
                .triggers
                .iter()
                .filter_map(|trigger| {
                    normalized_phrase_match_len(input, trigger)
                        .map(|len| (len, normalize_route_text(trigger)))
                })
                .max_by(|a, b| a.0.cmp(&b.0));
            let (trigger_tier, trigger_phrase) = match best_trigger {
                Some((len, phrase)) => (len, phrase),
                None => (0, String::new()),
            };
            let mut candidate_tokens = content_tokens(&m.description);
            candidate_tokens.extend(content_tokens(&m.kind));
            let overlap_terms: Vec<String> = message_tokens
                .iter()
                .filter(|token| candidate_tokens.contains(*token))
                .cloned()
                .collect();
            let content_overlap = overlap_terms.len();
            // The signal honestly reflects which regime matched, populated below
            // once the winning regime is known.
            let _ = &trigger_phrase;
            (m.kind.clone(), trigger_tier, content_overlap, {
                // Stash both candidate signals as "trigger|overlap" so the regime
                // selector can pick the right one without recomputing.
                format!("{}\u{1}{}", trigger_phrase, overlap_terms.join(", "))
            })
        })
        .collect();

    let mut full_granted_signals: Vec<RouteCandidateSignal> = scored
        .iter()
        .map(
            |(kind, trigger_tier, content_overlap, _)| RouteCandidateSignal {
                kind: kind.clone(),
                trigger_tier: *trigger_tier,
                content_overlap: *content_overlap,
            },
        )
        .collect();
    full_granted_signals.sort_by(|a, b| {
        b.trigger_tier
            .cmp(&a.trigger_tier)
            .then_with(|| b.content_overlap.cmp(&a.content_overlap))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    // NO-TASK ABSTENTION (router_precision adoption Track A, fix #1 — the biggest
    // lever). The real why-not judge (n=55) found 47/50 justified over-matches were
    // NO-TASK turns: the router firing on system-reminder / idle / task-complete /
    // settled turns that carry no genuine user request. Such a turn must abstain
    // (no_match) rather than description-overlap matching against injected context.
    // Conservative by construction (see `has_no_genuine_task`): it fires ONLY on
    // clearly-no-task turns, never on a turn with a real task token — recall is not
    // at risk. Granted candidates are still reported (telemetry parity); matching is
    // empty so the live hook renders silence.
    if has_no_genuine_task(input) {
        return RouteResolution {
            granted_candidates,
            full_granted_signals,
            abstention_reason: Some(RouteAbstentionReason::HardAbstain),
            matching_candidates: Vec::new(),
            selected_kind: None,
            outcome: RouteOutcome::NoMatch,
            match_signals: std::collections::BTreeMap::new(),
        };
    }

    // Abstain gate (experiment #002, ground-truth confirmed): a message that asks to
    // create a FREE artifact kind (decision / proposal / milestone / spark / learning)
    // is begin-able directly and must NOT be routed to a driven workflow. These requests
    // carry strong incidental description-overlap with driven workflows, so without this
    // gate the ranker over-routes them. Action-scoped patterns only (verb+kind), so a
    // track like "implement the decision engine" is not caught. Measured on the live
    // engine over a 70-case judge-labeled corpus: abstention_correct 0.33 → 0.67,
    // misroutes 22 → 11, with recall and selection_precision unchanged.
    if is_free_kind_request(input) {
        return RouteResolution {
            granted_candidates,
            full_granted_signals,
            abstention_reason: Some(RouteAbstentionReason::HardAbstain),
            matching_candidates: Vec::new(),
            selected_kind: None,
            outcome: RouteOutcome::NoMatch,
            match_signals: std::collections::BTreeMap::new(),
        };
    }

    let max_trigger_tier = scored
        .iter()
        .map(|(_, tier, _, _)| *tier)
        .max()
        .unwrap_or(0);

    // Resolve the citable signal for a scored row given the winning regime.
    let signal_for = |raw: &str, trigger_regime: bool| -> String {
        let mut parts = raw.splitn(2, '\u{1}');
        let trigger = parts.next().unwrap_or("");
        let overlap = parts.next().unwrap_or("");
        if trigger_regime {
            format!("matched trigger \"{}\"", trigger)
        } else if overlap.is_empty() {
            String::new()
        } else {
            format!("description overlap on {}", overlap)
        }
    };

    let trigger_regime = max_trigger_tier > 0;
    let mut relevant: Vec<(String, usize, usize, String)> = if trigger_regime {
        // Trigger regime: candidates at the winning tier are scored first by the
        // trigger hit and then by description overlap, with a hard output cap.
        let mut at_tier: Vec<(String, usize, usize, String)> = scored
            .iter()
            .filter(|(_, tier, _, _)| *tier == max_trigger_tier)
            .cloned()
            .collect();
        at_tier.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        at_tier
    } else {
        // Description regime: overlap >= floor, ordered by overlap desc then kind asc.
        let mut over_floor: Vec<(String, usize, usize, String)> = scored
            .iter()
            .filter(|(_, _, overlap, raw)| clears_description_floor(*overlap, raw))
            .cloned()
            .collect();
        over_floor.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        over_floor
    };
    relevant.truncate(CANDIDATE_CAP);

    let matching_candidates: Vec<String> = relevant
        .iter()
        .map(|(kind, _, _, _)| kind.clone())
        .collect();

    let match_signals: std::collections::BTreeMap<String, String> = relevant
        .iter()
        .map(|(kind, _, _, raw)| (kind.clone(), signal_for(raw, trigger_regime)))
        .collect();

    let (selected_kind, outcome) = if granted_candidates.is_empty() {
        (None, RouteOutcome::NoMatch)
    } else {
        match relevant.as_slice() {
            [] => (None, RouteOutcome::NoMatch),
            [(kind, trigger_tier, content_overlap, _)] => {
                let _ = (trigger_tier, content_overlap);
                (Some(kind.clone()), RouteOutcome::Single)
            }
            [(kind, trigger_tier, content_overlap, _), rest @ ..] => {
                let (_, next_trigger_tier, next_content_overlap, _) = &rest[0];
                let clear_trigger_leader = *trigger_tier > 0
                    && (*trigger_tier > *next_trigger_tier
                        || *content_overlap > *next_content_overlap);
                let clear_description_leader = *trigger_tier == 0
                    && *content_overlap >= SINGLE_SELECT_FLOOR
                    && *content_overlap > *next_content_overlap;
                if clear_trigger_leader || clear_description_leader {
                    (Some(kind.clone()), RouteOutcome::Single)
                } else {
                    (None, RouteOutcome::Candidates)
                }
            }
        }
    };

    // A weak-description-overlap candidate can only survive the floor if its lone
    // token is not in LOW_SIGNAL_DESCRIPTION_TOKENS; every surviving singleton is
    // specific enough to auto-resolve.
    RouteResolution {
        granted_candidates,
        full_granted_signals,
        abstention_reason: if outcome == RouteOutcome::NoMatch {
            Some(RouteAbstentionReason::FloorMiss)
        } else {
            None
        },
        matching_candidates,
        selected_kind,
        outcome,
        match_signals,
    }
}

/// Action-scoped patterns that signal the message asks to create a FREE artifact
/// kind (decision/proposal/milestone/spark/learning) — begin-able directly, never
/// routed. Verb+kind scoped so driven-playbook requests that merely mention a kind
/// noun ("implement the decision engine") are not falsely abstained.
const FREE_KIND_PATTERNS: &[&str] = &[
    "record a decision",
    "record decision",
    "decision about",
    "decision:",
    "we need to decide",
    "let's decide",
    "decide whether",
    "decide between",
    "decide:",
    "write a proposal",
    "put together a proposal",
    "proposal for",
    "draft a proposal",
    "create a milestone",
    "milestone for",
    "capture this spark",
    "capture spark",
    "capture this idea",
    "capture the idea",
    "spark:",
    "i had an idea",
    "what if ",
    "record a learning",
    "log a learning",
    "capture a learning",
    "learning from",
    "learning:",
];

/// Whether the message is a free-kind creation request (abstain gate, experiment #002).
fn is_free_kind_request(input: &str) -> bool {
    let n = normalize_route_text(input);
    FREE_KIND_PATTERNS.iter().any(|p| n.contains(p))
}

/// Whole-message acknowledgment / settled turns — the user is signalling "we're
/// done here", not asking for new work (router_precision fix #1). Compared against
/// the punctuation-stripped, normalized message by EXACT match only, so a real
/// task that merely CONTAINS one of these words ("complete the spec", "finished
/// the migration") is never caught.
const SETTLED_MESSAGES: &[&str] = &[
    "thanks",
    "thank you",
    "thanks so much",
    "thank you so much",
    "thanks a lot",
    "ty",
    "done",
    "all done",
    "task complete",
    "task completed",
    "all set",
    "great",
    "great thanks",
    "thanks great",
    "perfect",
    "perfect thanks",
    "thanks perfect",
    "looks good",
    "looks great",
    "lgtm",
    "sounds good",
    "nice",
    "cool",
    "awesome",
    "got it",
    "makes sense",
    "understood",
    "no thanks",
    "never mind",
    "nevermind",
    "that works",
    "that helps",
    "ok thanks",
    "okay thanks",
    "thanks that helps",
];

/// Strip harness-injected, non-task content so a turn whose ONLY content is
/// injected context reduces to nothing (router_precision fix #1). Today this
/// removes every `<system-reminder ...> ... </system-reminder>` block
/// (case-insensitive; an unclosed opener strips to end-of-input) — the documented
/// NO-TASK flood source. Genuine user text outside such blocks is preserved
/// verbatim, so a real request accompanied by an injected reminder still routes.
fn strip_injected_context(input: &str) -> String {
    const OPEN_TAG: &str = "<system-reminder";
    const CLOSE_TAG: &str = "</system-reminder>";
    let lower = input.to_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find(OPEN_TAG) {
        let start = cursor + rel;
        out.push_str(&input[cursor..start]);
        match lower[start..].find(CLOSE_TAG) {
            Some(rel_close) => cursor = start + rel_close + CLOSE_TAG.len(),
            None => {
                cursor = input.len();
                break;
            }
        }
    }
    out.push_str(&input[cursor..]);
    out
}

/// Punctuation-insensitive normalization for the settled-message check: split on
/// non-alphanumerics, lowercase, single-space join. Turns "thanks!", "looks
/// good.", and "ok, thanks" into their bare word forms for exact comparison.
fn normalize_for_settled(input: &str) -> String {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the turn carries NO genuine user task — the no-task abstention test
/// (router_precision fix #1). CONSERVATIVE by design: it fires only when
/// (a) stripping harness-injected context leaves the turn EMPTY / whitespace-only
/// (an empty turn or a pure system-reminder-only turn), or (b) the entire
/// normalized message is a settled acknowledgment. Any turn that carries ANY real
/// user text survives — including short, low-signal continuations like "go", "do
/// it", "ok", "yes" that keep working on the prior task. The router sees only the
/// current turn (no history), so the gate deliberately abstains ONLY on
/// turn-locally-unambiguous no-task turns and never on a genuine request; it
/// therefore cannot regress recall.
///
/// NOTE (deliberately NOT `content_tokens(...).is_empty()`): `content_tokens`
/// drops words < 3 chars and stopwords, so it would classify real continuations
/// ("go", "do it") as no-task and suppress work-continuing turns. The emptiness
/// test is `trim().is_empty()` on the stripped text so only genuinely-empty turns
/// abstain via branch (a).
fn has_no_genuine_task(input: &str) -> bool {
    let stripped = strip_injected_context(input);
    if stripped.trim().is_empty() {
        return true;
    }
    SETTLED_MESSAGES.contains(&normalize_for_settled(input).as_str())
}

fn normalized_phrase_match(input: &str, phrase: &str) -> bool {
    let normalized_input = normalize_route_text(input);
    let normalized_phrase = normalize_route_text(phrase);
    !normalized_phrase.is_empty() && normalized_input.contains(&normalized_phrase)
}

fn normalized_phrase_match_len(input: &str, phrase: &str) -> Option<usize> {
    if normalized_phrase_match(input, phrase) {
        Some(normalize_route_text(phrase).len())
    } else {
        None
    }
}

fn clears_description_floor(overlap: usize, raw_signal: &str) -> bool {
    if overlap >= RELEVANCE_FLOOR {
        return true;
    }
    if overlap == 0 {
        return false;
    }

    // overlap == 1 (below the floor): a lone shared token qualifies ONLY when it
    // is DISTINCTIVE — long enough to be unlikely coincidental AND not a known
    // low-signal word. This is what makes "a single COMMON token no longer
    // qualifies" true while holding recall for genuine single-distinctive-token
    // intents (e.g. "recap", "invoice").
    let mut parts = raw_signal.splitn(2, '\u{1}');
    let _trigger = parts.next();
    let term = parts.next().unwrap_or("").trim();
    !LOW_SIGNAL_DESCRIPTION_TOKENS.contains(&term)
        && term.chars().count() >= DISTINCTIVE_SINGLE_TOKEN_LEN
}

fn normalize_route_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Content tokens of `text` (spec R10): lowercased alphanumeric words of length ≥ 3
/// that are not stopwords. A `BTreeSet` gives distinct tokens with deterministic
/// iteration order.
fn content_tokens(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.len() >= 3 && !STOPWORDS.contains(&word.as_str()))
        .collect()
}

/// `PlaybookRegistry` backed by the compiled-in seeds.
///
/// Recognises:
/// - `"track"` → `seeds::track_seed()`
/// - `"playbook"` → `seeds::playbook_seed()`
/// - `"decision"` → `seeds::decision_seed()`
/// - `"initiative"` → `seeds::initiative_seed()`
/// - `"milestone"` → `seeds::milestone_seed()`
/// - `"proposal"` → `seeds::proposal_seed()`
/// - `"spark"` → `seeds::spark_seed()`
/// - anything else → `None`
pub struct SeedPlaybookRegistry;

impl PlaybookRegistry for SeedPlaybookRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        match kind {
            "track" => Some(track_seed()),
            "playbook" => Some(playbook_seed()),
            "decision" => Some(decision_seed()),
            "initiative" => Some(initiative_seed()),
            "learning" => Some(learning_seed()),
            "milestone" => Some(milestone_seed()),
            "proposal" => Some(proposal_seed()),
            "spark" => Some(spark_seed()),
            "backlog_item" => Some(backlog_item_seed()),
            _ => None,
        }
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        // M2 (hook_content_serving P5): keep parity with `machine_for` — every
        // kind the seed resolves to a machine must also resolve to a playbook
        // id, so a seed-served (state, role) hook always has a `playbook_id` for
        // the body read. The `CompositePlaybookRegistry` consults the hearth
        // first, so an on-disk playbook dir, when present, still takes priority.
        match kind {
            "track" => Some("20260422T0000_track_lifecycle".to_string()),
            "playbook" => Some("playbook_lifecycle".to_string()),
            "decision" => Some("decision_lifecycle".to_string()),
            "initiative" => Some("initiative_lifecycle".to_string()),
            "learning" => Some("learning_lifecycle".to_string()),
            "milestone" => Some("milestone_lifecycle".to_string()),
            "proposal" => Some("proposal_lifecycle".to_string()),
            "spark" => Some("spark_lifecycle".to_string()),
            "backlog_item" => Some("backlog_item_lifecycle".to_string()),
            _ => None,
        }
    }

    fn source_for(&self, kind: &str) -> Option<PlaybookSource> {
        self.playbook_id_for(kind)
            .map(|playbook_id| PlaybookSource {
                hearth: None,
                playbook_id,
            })
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        vec![
            track_seed(),
            playbook_seed(),
            decision_seed(),
            initiative_seed(),
            learning_seed(),
            milestone_seed(),
            proposal_seed(),
            spark_seed(),
            backlog_item_seed(),
        ]
    }
}
