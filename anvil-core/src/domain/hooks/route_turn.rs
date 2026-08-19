//! The route-turn DECISION + formatting — the pure half of the `anvil-hooks
//! route-turn` subcommand.
//!
//! Every user turn is shipped into the engine's `route` RPC (which records the
//! routing decision — single / candidates / no_match — via the activity log +
//! routing-activity sink). This module is the pure, testable core of the
//! harness-side turn hook: given a [`RouteTurnOutcome`] (distilled from the
//! engine's `RouteResponse`), it produces the guidance text the harness injects
//! back to the agent. The stdin parsing + gRPC plumbing live in the binary; the
//! decision lives here so it is unit/brine-testable without a process or a
//! socket.
//!
//! ## Outcome → stdout contract
//! - [`RouteTurnOutcome::Single`] → a SPOON-FED nudge: the playbook's name + its
//!   one-line purpose, the EXACT begin call (`anvil_orchestrate(selection:
//!   "<kind>")`), and the fields it will need — enough for the model to ACT,
//!   because skills are retired and the model only knows what Anvil tells it.
//! - [`RouteTurnOutcome::Candidates`] → each candidate as `- kind: purpose`, plus
//!   its exact begin call and required fields, so the model can CHOOSE and ACT.
//! - [`RouteTurnOutcome::NoMatch`] → empty string (silent — most turns are not
//!   playbook intents; we never inject noise).
//!
//! Fail-open is the binary's job (any stdin/gRPC failure → no output, exit 0);
//! this module only renders a known outcome.

use serde::{Deserialize, Serialize};

/// One candidate playbook distilled for the "pick one" guidance: its kind and a
/// one-line purpose (already truncated by the binary to the first sentence), plus
/// the route_response_mirrors_begin annotations (intent, step_outline, why_fits)
/// so the model can choose deliberately.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CandidateBrief {
    /// The playbook kind the model would select.
    pub kind: String,
    /// A one-line purpose (first sentence of the playbook's description).
    pub description: String,
    /// Authored route trigger phrases from the playbook machine. These are
    /// action-verb hints for the local LLM router, not a deterministic selector.
    #[serde(default)]
    pub route_triggers: Vec<String>,
    /// The fields the playbook will require to run.
    pub required_fields: Vec<String>,
    /// The (initial_state, doer) intent for this candidate (annotation).
    pub intent: String,
    /// The machine's state names in order (annotation).
    pub step_outline: Vec<String>,
    /// The concrete match signal that produced this candidate (annotation).
    pub why_fits: String,
}

/// The distilled routing outcome for a single user turn — the harness-agnostic
/// projection of the engine's `RouteResponse` the formatter consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTurnOutcome {
    /// The route auto-resolved to exactly one playbook kind. Carries the spoon-feed
    /// payload: the kind's one-line purpose and the fields `anvil_orchestrate` needs.
    /// route_response_mirrors_begin: when `guidance` is non-empty it is the
    /// begin-equivalent first-step body the engine served — rendered in full so the
    /// model can ACT immediately; an empty `guidance` degrades to the thin one-line
    /// nudge (fail-open).
    Single {
        /// The resolved playbook kind.
        kind: String,
        /// A one-line purpose (first sentence of the playbook's description).
        description: String,
        /// The router's one-line reason for why this playbook fits this turn.
        /// Empty means the route response did not carry a reason; render the
        /// existing purpose-based nudge instead.
        why: String,
        /// The fields the playbook will require to run.
        required_fields: Vec<String>,
        /// The begin-equivalent guidance body served by the engine (empty when the
        /// engine degraded to a thin route — render the thin nudge then).
        guidance: String,
    },
    /// Several driven candidates matched; the agent must pick one. Each carries its
    /// one-line purpose so the model can choose the best fit.
    Candidates { candidates: Vec<CandidateBrief> },
    /// The conversation has an open playbook and the user sent a continuation
    /// token (resume_aware_routing): point the agent BACK at the in-progress
    /// playbook's current step rather than offering new candidates. Carries the
    /// open artifact's id/kind/state, the current-step guidance, and the
    /// supported advance action.
    Resume {
        artifact_id: String,
        kind: String,
        state: String,
        /// The current `(state, role)` next-step guidance (engine-served). Empty
        /// when the state declares no hook.
        guidance: String,
        /// The supported advance action for the current state (e.g.
        /// "complete → spec_review (role: reviewer)"). Empty for terminal.
        advance_action: String,
    },
    /// No playbook intent — stay silent.
    NoMatch,
}

impl RouteTurnOutcome {
    /// Project a gRPC-shaped route response into a [`RouteTurnOutcome`].
    /// `resolution_outcome` is the engine's `"single" | "candidates" | "no_match"`
    /// label; anything unrecognized (or a `single` with an empty selected kind)
    /// falls back to `NoMatch` so the hook stays silent rather than guessing.
    ///
    /// `candidates` carries each candidate's full projection
    /// ([`RouteCandidateWire`]) straight from the engine's `RouteResponse` —
    /// kind, description, required_fields, and the annotations (intent,
    /// step_outline, why_fits). `guidance` is the engine-served begin-equivalent
    /// body for a `single` (empty for candidates / no_match, or when the engine
    /// degraded to a thin route).
    pub fn from_resolution(
        resolution_outcome: &str,
        selected_kind: &str,
        guidance: &str,
        candidates: &[RouteCandidateWire],
    ) -> RouteTurnOutcome {
        let matching_candidates: Vec<String> = candidates
            .iter()
            .map(|c| c.kind.trim().to_string())
            .filter(|kind| !kind.is_empty())
            .collect();
        Self::from_route_response(
            resolution_outcome,
            selected_kind,
            guidance,
            candidates,
            &matching_candidates,
            &ResumeWire::default(),
        )
    }

    /// Project a gRPC `RouteResponse` (including the resume_* fields) into a
    /// [`RouteTurnOutcome`]. When `resolution_outcome == "resume"` and the
    /// resume wire carries a non-empty artifact id, this yields
    /// [`RouteTurnOutcome::Resume`]; otherwise it falls through to the
    /// single/candidates/no_match projection. resume_aware_routing.
    pub fn from_route_response(
        resolution_outcome: &str,
        selected_kind: &str,
        guidance: &str,
        candidates: &[RouteCandidateWire],
        matching_candidates: &[String],
        resume: &ResumeWire,
    ) -> RouteTurnOutcome {
        Self::from_route_response_with_why(
            resolution_outcome,
            selected_kind,
            guidance,
            "",
            candidates,
            matching_candidates,
            resume,
        )
    }

    /// Project a gRPC `RouteResponse` into a [`RouteTurnOutcome`], carrying the
    /// hook-layer router's reason for a `single` when one is available.
    pub fn from_route_response_with_why(
        resolution_outcome: &str,
        selected_kind: &str,
        guidance: &str,
        route_why: &str,
        candidates: &[RouteCandidateWire],
        matching_candidates: &[String],
        resume: &ResumeWire,
    ) -> RouteTurnOutcome {
        if resolution_outcome == "resume" && !resume.artifact_id.trim().is_empty() {
            return RouteTurnOutcome::Resume {
                artifact_id: resume.artifact_id.clone(),
                kind: resume.kind.clone(),
                state: resume.state.clone(),
                guidance: resume.guidance.clone(),
                advance_action: resume.advance_action.clone(),
            };
        }
        match resolution_outcome {
            "single" if !selected_kind.trim().is_empty() => {
                let kind = selected_kind.trim().to_string();
                // Look up the selected kind's metadata in the candidate set so the
                // nudge can name the purpose + required fields. Absent → empty.
                let (description, required_fields) = candidates
                    .iter()
                    .find(|c| c.kind.trim() == kind)
                    .map(|c| (c.description.clone(), c.required_fields.clone()))
                    .unwrap_or_default();
                RouteTurnOutcome::Single {
                    kind,
                    description,
                    why: route_why.trim().to_string(),
                    required_fields,
                    guidance: guidance.to_string(),
                }
            }
            "candidates" => {
                let briefs: Vec<CandidateBrief> = matching_candidates
                    .iter()
                    .filter_map(|c| {
                        let kind = c.trim();
                        let candidate = candidates
                            .iter()
                            .find(|candidate| !kind.is_empty() && candidate.kind.trim() == kind)?;
                        Some(CandidateBrief {
                            kind: kind.to_string(),
                            description: candidate.description.trim().to_string(),
                            route_triggers: candidate.route_triggers.clone(),
                            required_fields: candidate.required_fields.clone(),
                            intent: candidate.intent.trim().to_string(),
                            step_outline: candidate.step_outline.clone(),
                            why_fits: candidate.why_fits.trim().to_string(),
                        })
                    })
                    .collect();
                if briefs.is_empty() {
                    RouteTurnOutcome::NoMatch
                } else {
                    RouteTurnOutcome::Candidates { candidates: briefs }
                }
            }
            _ => RouteTurnOutcome::NoMatch,
        }
    }
}

/// The full per-candidate wire projection the binary hands `from_resolution` —
/// the engine's `RouteResponse.CandidateMeta` carried verbatim. The router
/// prompt applies the description cap after this projection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteCandidateWire {
    pub kind: String,
    pub description: String,
    pub route_triggers: Vec<String>,
    pub required_fields: Vec<String>,
    pub intent: String,
    pub step_outline: Vec<String>,
    pub why_fits: String,
}

/// The resume_* projection of the engine's `RouteResponse` the binary hands
/// `from_route_response` (resume_aware_routing). All empty for a non-resume
/// outcome; populated only when `resolution_outcome == "resume"`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResumeWire {
    pub artifact_id: String,
    pub kind: String,
    pub state: String,
    pub guidance: String,
    pub advance_action: String,
}

/// The hook-layer LLM router's verdict for a candidate set. The LLM call itself
/// lives in the binary (I/O, timeout-bound); this enum is the PURE projection of
/// its result so pick / abstain / fallback mapping stays unit/brine-testable
/// without a process or socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterVerdict {
    /// The router committed to a kind (already JSON-parsed). Honored only when it
    /// names one of the shortlist's candidates; otherwise the caller abstains.
    Pick { kind: String, why: String },
    /// The router judged that NONE of the candidates fit — stay silent. This is
    /// the precision win: don't push a playbook that doesn't belong.
    Abstain,
    /// The router was unreachable / timed out / unparseable. Product behavior is
    /// caller-owned; the live hook treats this as an abstention.
    Fallback,
}

/// Narrow a routing outcome with a router verdict. This remains as the pure
/// pick/abstain/fallback mapper used by tests; the live hook may pass a granted
/// candidate set even when the engine's deterministic outcome was `single` or
/// `no_match`.
///
/// For a `Candidates` shortlist:
/// - [`RouterVerdict::Pick`] naming a real candidate → collapse to `Single`
///   (spoon-fed downstream), carrying that candidate's purpose and required
///   fields. The spoon-feed names the kind, purpose, begin call, and needs.
/// - [`RouterVerdict::Pick`] naming a NON-candidate → `NoMatch` (never invent a
///   kind the engine didn't grant, and never fall back to keyword rendering).
/// - [`RouterVerdict::Abstain`] → `NoMatch` (silent — the precision win).
/// - [`RouterVerdict::Fallback`] → `NoMatch` (silent — a wrong nudge is worse
///   than none).
pub fn narrow_candidates(outcome: RouteTurnOutcome, verdict: &RouterVerdict) -> RouteTurnOutcome {
    let RouteTurnOutcome::Candidates { candidates } = &outcome else {
        // Single / NoMatch never reach the LLM — pass through untouched.
        return outcome;
    };
    match verdict {
        RouterVerdict::Pick { kind, why } => {
            let kind = kind.trim();
            match candidates.iter().find(|c| c.kind == kind) {
                Some(chosen) => RouteTurnOutcome::Single {
                    kind: chosen.kind.clone(),
                    description: chosen.description.clone(),
                    why: why.trim().to_string(),
                    required_fields: chosen.required_fields.clone(),
                    // The narrowed single carries no engine-served guidance body
                    // (the LLM picked from the shortlist, not a single resolution);
                    // the thin nudge still names kind, purpose, and begin call.
                    guidance: String::new(),
                },
                // The LLM named something outside the granted shortlist. Never
                // fabricate a kind, and never keyword-fall-back to the shortlist.
                None => RouteTurnOutcome::NoMatch,
            }
        }
        RouterVerdict::Abstain => RouteTurnOutcome::NoMatch,
        RouterVerdict::Fallback => RouteTurnOutcome::NoMatch,
    }
}

/// A transcript-grounded read of whether playbook work is already underway in the
/// conversation (context_aware_routing). The hook derives this by scanning the
/// recent transcript tail for anvil lifecycle tool_uses; the selector uses it to
/// bias its pick/abstain decision. Degrades to [`InProgressSignal::None`] whenever
/// the transcript is absent or carries no lifecycle signal, so routing is
/// unchanged in the no-signal case.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InProgressSignal {
    /// No playbook-in-progress signal found in the transcript tail.
    #[default]
    None,
    /// A `begin` of `kind` has no later `complete`/terminal — a playbook is OPEN.
    /// The selector should prefer letting the agent CONTINUE / check in on it
    /// rather than routing to a NEW playbook.
    OpenPlaybookRun { kind: String },
    /// Playbook-shaped work (forge-skill / spec-plan-implement / other lifecycle
    /// activity) is happening with NO begin at all. The selector should prefer
    /// PICKING the best-fit playbook so the work gets properly begun and measured.
    WorkWithoutBegin,
}

/// The context distilled from a harness transcript tail (context_aware_routing):
/// a compact digest of recent turns plus the in-progress playbook signal. The
/// hook reads the (bounded) transcript tail from disk and calls
/// [`extract_transcript_context`] to build this; both fields degrade to empty /
/// `None` when the transcript is missing or unparseable, so routing stays
/// fail-open and unchanged in the no-transcript case.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranscriptContext {
    /// A compact, bounded digest of the last few user turns + short assistant
    /// intents. Empty when nothing could be distilled.
    pub recent_context: String,
    /// The transcript-grounded in-progress playbook signal.
    pub in_progress: InProgressSignal,
    /// continuation_recognition: the LAST assistant turn, untruncated, for the
    /// proposal recogniser. Kept separate from the digest because the digest is
    /// capped at 200 chars per turn and a cut proposal is not a proposal.
    pub last_assistant_text: String,
    /// Whether that turn was cut — recorded AT CUT TIME. The digest's `truncate`
    /// appends an ellipsis indistinguishable from an author's own, and the tail
    /// window leaves no trace at all, so this cannot be inferred afterwards.
    pub last_assistant_truncated: bool,
}

/// Build the local-LLM router's prompt for a granted candidate set: a zero-shot
/// intent router that outputs ONLY compact JSON `{"kind":"...","why":"..."}`,
/// constrained to the granted set, and ABSTAINS when none clearly fits. The
/// caller supplies only access-scoped candidates, so the prompt never offers an
/// unauthorized kind.
///
/// context_aware_routing: `recent_context` (a compact digest of prior turns) and
/// `in_progress` (a transcript-grounded read of whether a playbook is already
/// underway) are woven in so the selector can (a) interpret a SHORT continuation
/// ("go", "continue", "yes", "fix it") as continuing the prior intent instead of
/// abstaining, (b) defer to the open playbook's check-in/continue when one is
/// OPEN, and (c) lean toward a PICK when playbook-shaped work is happening with no
/// begin. All context is additive: with empty `recent_context` and
/// `InProgressSignal::None` the prompt is the original no-context router prompt.
pub fn build_router_prompt(
    message: &str,
    recent_context: &str,
    in_progress: &InProgressSignal,
    candidates: &[CandidateBrief],
) -> String {
    let list = candidates
        .iter()
        .map(|c| {
            // Route descriptions are authored as concise routing instructions,
            // including NOT-this contrasts. Keep them intact and apply one cap.
            const DESCRIPTION_CAP: usize = 220;
            let concise: String = c.description.trim().chars().take(DESCRIPTION_CAP).collect();
            let concise = concise.trim();
            // Triggers are NOT appended: the experiment showed appending the
            // trigger list bloats the prompt and LOWERS local-model accuracy
            // (Qwen3-8B 0.85 clean -> 0.75 with triggers). The per-playbook
            // route.description carries the clean routing signal on its own.
            let _ = &c.route_triggers;
            if concise.is_empty() {
                format!("- {}", c.kind)
            } else {
                format!("- {}: {}", c.kind, concise)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // context_aware_routing: the recent-turns digest + the current turn, so a
    // short continuation is read against what came before instead of in isolation.
    // Gated on the digest being present: with no transcript digest the block is
    // omitted entirely, so the no-context prompt is byte-for-byte the original
    // (the current message is already the router's user turn — no need to restate
    // it, and the recall instruction should not weaken abstention when there is no
    // prior context to recall against).
    let context_block = {
        let digest = recent_context.trim();
        let current = message.trim();
        if digest.is_empty() {
            String::new()
        } else {
            let mut block = String::from(
                "Recent conversation context (most recent last) — use it to interpret SHORT or \
continuation messages (\"go\", \"continue\", \"yes\", \"do it\", \"fix it\") as CONTINUING the \
work already underway. Do NOT abstain merely because THIS turn is short when the context shows \
real work in progress:\n",
            );
            block.push_str(digest);
            block.push('\n');
            if !current.is_empty() {
                block.push_str(&format!("\nCurrent turn: {}\n", current));
            }
            block.push('\n');
            block
        }
    };

    // context_aware_routing: the in-progress playbook signal steers pick vs abstain.
    let in_progress_block = match in_progress {
        InProgressSignal::None => String::new(),
        InProgressSignal::OpenPlaybookRun { kind } => format!(
            "A \"{kind}\" playbook is already IN PROGRESS in this conversation (a begin with no \
matching complete). Prefer letting the agent CONTINUE or check in on that in-progress playbook — \
abstain rather than routing to a NEW playbook, unless THIS turn clearly starts different new work.\n\n",
            kind = kind
        ),
        InProgressSignal::WorkWithoutBegin => String::from(
            "Playbook-shaped work is already happening in this conversation but WITHOUT a begin. \
Prefer PICKING the best-fit playbook so the work gets properly begun and measured from the start — \
lean toward a pick rather than abstaining.\n\n",
        ),
    };

    format!(
        "You are an intent router for a developer playbook engine. Given a user \
message, output ONLY compact JSON: {{\"kind\":\"<kind-or-abstain>\",\"why\":\"<one short sentence: why THIS playbook fits THIS turn and what it'll do for the work>\"}}.\n\n\
{context_block}{in_progress_block}\
Route to one of these playbooks ONLY when the message clearly asks to do that thing:\n\
{list}\n\n\
Output \"abstain\" when NONE clearly applies. ALWAYS abstain for: requests to \
create a decision/proposal/milestone/spark/learning (these are recorded directly, \
not routed); general questions, debugging, status checks, architecture discussion, \
or chit-chat.",
        context_block = context_block,
        in_progress_block = in_progress_block,
        list = list
    )
}

/// Parse the local-LLM router's reply into a chosen kind — mirrors the offline
/// experiment's parse: slice the first `{`…last `}`, JSON-decode, read `kind`.
/// Returns the trimmed kind string (which may be `"abstain"`); `None` when the
/// reply carries no parseable JSON object or no `kind` field, so the caller can
/// abstain. The Pick/Abstain mapping is the caller's job (it knows the candidate
/// set).
pub fn parse_router_kind(raw: &str) -> Option<String> {
    parse_router_decision(raw).map(|decision| decision.kind)
}

/// Parsed local-router decision. `why` is optional in the wire JSON: absent or
/// non-string values degrade to an empty reason so old router stubs remain valid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouterDecision {
    pub kind: String,
    pub why: String,
}

/// Parse the local-LLM router's reply into the chosen kind plus its one-line
/// reason. `kind` remains required; `why` is optional and never a parse error.
pub fn parse_router_decision(raw: &str) -> Option<RouterDecision> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    let slice = &raw[start..=end];
    let json: serde_json::Value = serde_json::from_str(slice).ok()?;
    let kind = json.get("kind")?.as_str()?.trim().to_string();
    if kind.is_empty() {
        None
    } else {
        let why = json
            .get("why")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        Some(RouterDecision { kind, why })
    }
}

/// Shape the route-turn guidance for the ORIGINATING harness's UserPromptSubmit
/// stdout contract, so the guidance actually reaches the model.
///
/// Most harnesses (Claude Code, Codex, Grok, Hermes) inject a UserPromptSubmit
/// hook's RAW stdout verbatim as additional context, so we print the guidance as
/// plain text. Kiln is STRICTER: its `kiln-session` UserPromptSubmit reader parses
/// the hook's stdout as JSON and injects ONLY the top-level `additionalContext`
/// string field — raw text fails `serde_json::from_str` and is silently DROPPED.
/// For `--source kiln` we therefore wrap the guidance as
/// `{"additionalContext":"<guidance>"}` (serde handles all escaping). Any other /
/// absent source keeps the verbatim text, so no existing harness regresses.
///
/// Empty guidance is passed through unchanged — the caller prints nothing on empty
/// so the fail-open silence is preserved for every harness (never emits an empty
/// `{"additionalContext":""}` envelope).
pub fn shape_guidance_for_source(guidance: &str, source: &str) -> String {
    if guidance.is_empty() || source != "kiln" {
        return guidance.to_string();
    }
    serde_json::json!({ "additionalContext": guidance }).to_string()
}

/// The playbook kind this outcome actually DELIVERED to the model — what the
/// delivery row records so a begin can be attributed to the suggestion that
/// caused it.
///
/// `Single` and `Resume` each put exactly one kind in front of the model.
/// `Candidates` and `NoMatch` delivered none: picking a candidate here would
/// fabricate a single delivery that never happened.
pub fn guidance_kind_of(outcome: &RouteTurnOutcome) -> &str {
    match outcome {
        RouteTurnOutcome::Single { kind, .. } => kind,
        RouteTurnOutcome::Resume { kind, .. } => kind,
        RouteTurnOutcome::Candidates { .. } | RouteTurnOutcome::NoMatch => "",
    }
}

/// Render the guidance text for an outcome. `NoMatch` returns the empty string
/// (the binary then prints nothing). Single + Candidates SPOON-FEED the model:
/// the playbook's purpose, the exact begin call, and the fields it will need.
pub fn format_guidance(outcome: &RouteTurnOutcome, conversation_id: &str) -> String {
    match outcome {
        RouteTurnOutcome::Single {
            kind,
            description,
            why,
            required_fields,
            guidance,
        } => {
            let reason = why.trim();
            let purpose = if reason.is_empty() {
                purpose_clause(description)
            } else {
                format!(" — {}", reason)
            };
            let call = format_orchestrate_call(kind, conversation_id);
            let fields = if required_fields.is_empty() {
                "no required fields".to_string()
            } else {
                required_fields.join(", ")
            };
            let begin_cli = format_begin_cli(kind, required_fields, conversation_id);
            let header = format!(
                "▶ Anvil routed this turn to the `{kind}` playbook{purpose}. BEGIN IT NOW: {call}; it needs {fields}. Or begin in ONE shell call (no MCP round-trip needed): `{begin_cli}`. Beginning gives you a scoped first step, automatic measurement, and a resumable record — one call now beats redoing this ad-hoc. This is the front door for this work — begin it, don't do it ad-hoc. A task is often BOTH a concrete deliverable AND this playbook's purpose (e.g. both writing a doc AND capturing a memory worth keeping) — do NOT skip on a narrow 'this is X, not Y' distinction; if the playbook's purpose is any part of what the user wants, begin it. Skip only if the playbook is genuinely IRRELEVANT to the intent (e.g. a bare acknowledgement or a background notification). Work outside a matched playbook is unguided and unmeasured.",
                kind = kind,
                purpose = purpose,
                call = call,
                fields = fields,
                begin_cli = begin_cli
            );
            // route_response_mirrors_begin: when the engine served the
            // begin-equivalent guidance body, render it IN FULL after the nudge so
            // the model can ACT immediately (advisory — it still calls begin). An
            // empty guidance degrades to the thin nudge (fail-open).
            let body = guidance.trim();
            if body.is_empty() {
                header
            } else {
                format!(
                    "{header}\n\nFirst-step guidance (begin-equivalent):\n{body}",
                    header = header,
                    body = body
                )
            }
        }
        RouteTurnOutcome::Candidates { candidates } => {
            let list = candidates
                .iter()
                .map(|c| format_candidate_brief(c, conversation_id))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "▶ Anvil matched several playbooks to this turn — pick the best fit and BEGIN IT NOW (don't proceed ad-hoc); a task can be both a deliverable and a playbook's purpose — skip only if NONE is genuinely relevant, e.g. a bare ack or notification:\n{list}",
                list = list
            )
        }
        RouteTurnOutcome::Resume {
            artifact_id,
            kind,
            state,
            guidance,
            advance_action,
        } => {
            // resume_aware_routing: the conversation has an open playbook and the
            // user sent a continuation token — point them BACK at the in-progress
            // step rather than offering new candidates. Name the open artifact,
            // its current state, and the supported advance action; render the
            // current-step guidance in full when present.
            let action = advance_action.trim();
            let action_clause = if action.is_empty() {
                String::new()
            } else {
                format!("; continue via {}", action)
            };
            let header = format!(
                "▶ Resume {kind} ({state}) — playbook {artifact_id} is in progress{action_clause}.",
                kind = kind,
                state = state,
                artifact_id = artifact_id,
                action_clause = action_clause
            );
            let body = guidance.trim();
            if body.is_empty() {
                header
            } else {
                format!(
                    "{header}\n\nCurrent-step guidance:\n{body}",
                    header = header,
                    body = body
                )
            }
        }
        RouteTurnOutcome::NoMatch => String::new(),
    }
}

/// Render the PARK HINT that accompanies a SUPPRESSED resume (resume-signal
/// context-awareness). The engine surfaces this ONCE for an open playbook the
/// agent has moved on from (N consecutive unrelated turns): name the dangling
/// artifact, its state, and the machine-declared park action so the agent can
/// abandon it instead of being nagged to resume. Rendered ALONGSIDE the normal
/// turn guidance (it is not a `RouteTurnOutcome` — the turn still routed
/// normally). Empty `artifact_id` yields the empty string (nothing to surface).
pub fn format_park_hint(artifact_id: &str, kind: &str, state: &str, park_action: &str) -> String {
    if artifact_id.trim().is_empty() {
        return String::new();
    }
    let action = park_action.trim();
    let action_clause = if action.is_empty() {
        String::new()
    } else {
        format!(" — to park it, {}", action)
    };
    format!(
        "⏸ Anvil: the open {kind} playbook {artifact_id} ({state}) has been idle while you worked on other things. If it is no longer needed, PARK it{action_clause}; otherwise resume it. Either way this reminder will stop.",
        kind = kind,
        artifact_id = artifact_id,
        state = state,
        action_clause = action_clause
    )
}

/// Render one candidate's annotated brief: kind + purpose, then its intent,
/// step_outline, and why_fits when present (route_response_mirrors_begin), so the
/// model chooses deliberately rather than from a kind name alone.
fn format_candidate_brief(c: &CandidateBrief, conversation_id: &str) -> String {
    let purpose = c.description.trim();
    let mut head = if purpose.is_empty() {
        format!("- {}", c.kind)
    } else {
        format!("- {}: {}", c.kind, purpose)
    };
    head.push_str(&format!(
        "\n    call: {}",
        format_orchestrate_call(&c.kind, conversation_id)
    ));
    head.push_str(&format!(
        "\n    or one-call: {}",
        format_begin_cli(&c.kind, &c.required_fields, conversation_id)
    ));
    let fields = if c.required_fields.is_empty() {
        "no required fields".to_string()
    } else {
        c.required_fields.join(", ")
    };
    head.push_str(&format!("\n    needs: {}", fields));
    let intent = c.intent.trim();
    if !intent.is_empty() {
        head.push_str(&format!("\n    intent: {}", intent));
    }
    if !c.step_outline.is_empty() {
        head.push_str(&format!("\n    steps: {}", c.step_outline.join(" → ")));
    }
    let why = c.why_fits.trim();
    if !why.is_empty() {
        head.push_str(&format!("\n    why it fits: {}", why));
    }
    head
}

fn format_orchestrate_call(kind: &str, conversation_id: &str) -> String {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() {
        format!("anvil_orchestrate(selection: \"{}\")", kind)
    } else {
        format!(
            "anvil_orchestrate(selection: \"{}\", conversation_id: \"{}\")",
            kind, conversation_id
        )
    }
}

/// The structured begin flag for a known builtin field, or `None` for a generic
/// machine-declared field (which the begin CLI takes via `--field key=value`).
fn begin_flag_for(field: &str) -> Option<&'static str> {
    match field {
        "name" | "track_name" => Some("--name"),
        "parent_id" => Some("--parent-id"),
        "approver" => Some("--approver"),
        "playbook_name" => Some("--playbook-name"),
        "target_owner" => Some("--target-owner"),
        _ => None,
    }
}

/// A ready-to-run, copy-pasteable ONE-CALL begin command for the routed kind: the
/// `anvil-hooks begin` path, which reaches the engine without the MCP server.
/// (It is NOT broker-independent — a Foundry-mode engine requires a bearer and
/// the broker mints it — but the MCP tool channel is the part that flaps.)
/// Each required field is
/// emitted as its structured flag (builtins) or `--field key=<value>` (generic),
/// with `<value>` placeholders the model fills in. The conversation id is stamped
/// so the begin record joins this turn's route leg (the adoption-join key). This
/// collapses the multi-step `anvil_orchestrate`→`begin` ceremony into one shell call.
fn format_begin_cli(kind: &str, required_fields: &[String], conversation_id: &str) -> String {
    let mut cmd = format!("anvil-hooks begin --artifact-type {}", kind);
    for f in required_fields {
        match begin_flag_for(f) {
            Some(flag) => cmd.push_str(&format!(" {} <{}>", flag, f)),
            None => cmd.push_str(&format!(" --field {}=<value>", f)),
        }
    }
    let cid = conversation_id.trim();
    if !cid.is_empty() {
        cmd.push_str(&format!(" --conversation-id {}", cid));
    }
    cmd
}

/// Build the " — <purpose>" clause for the single nudge from a one-line
/// description. Empty description → empty clause (the nudge still names the kind +
/// begin call). The leading " — " keeps the sentence tight when a purpose exists.
fn purpose_clause(description: &str) -> String {
    let purpose = description.trim().trim_end_matches('.');
    if purpose.is_empty() {
        String::new()
    } else {
        format!(" — {}", purpose)
    }
}

/// Extract the user's prompt text from a harness's user-prompt event JSON.
///
/// Handles the supported shapes defensively (harnesses + hook events vary),
/// returning the first non-empty match:
/// - Claude Code **UserPromptSubmit**: `{ "prompt": "..." }` (also `user_prompt`).
/// - Claude Code **PreToolUse(Task)** — a SUBAGENT dispatch: the subagent's
///   mission is under `tool_input.prompt` (fall back to `tool_input.description`).
///   Routing this makes every subagent turn reach the router for a decision, not
///   just top-level user prompts.
/// - Hermes **pre_llm_call**: the message text under `message` / `prompt` /
///   `input`, or the last user turn in a `messages` array
///   (`messages[].content` with `role == "user"`).
///
/// Returns `None` when no message can be found, so the binary fails open.
pub fn extract_user_message(json: &serde_json::Value) -> Option<String> {
    // Direct scalar fields (Claude Code UserPromptSubmit + Hermes simple shape).
    for key in ["prompt", "user_prompt", "message", "input"] {
        if let Some(s) = json.get(key).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.to_string());
            }
        }
    }
    // Subagent dispatch (PreToolUse for the Task tool): the subagent's mission
    // lives under `tool_input.prompt` / `tool_input.description`.
    if let Some(tool_input) = json.get("tool_input") {
        for key in ["prompt", "description"] {
            if let Some(s) = tool_input.get(key).and_then(|v| v.as_str()) {
                if !s.trim().is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    // Hermes transcript shape: the last user message in `messages`.
    if let Some(messages) = json.get("messages").and_then(|v| v.as_array()) {
        for msg in messages.iter().rev() {
            let is_user = msg
                .get("role")
                .and_then(|v| v.as_str())
                .map(|r| r == "user")
                .unwrap_or(false);
            if is_user {
                if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                    if !content.trim().is_empty() {
                        return Some(content.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extract the conversation / session identifier from a harness's user-prompt
/// event JSON (resume_aware_routing H2). The id lets the engine bridge a
/// continuation message back to this conversation's open playbook — without it
/// the engine's resume pre-check (which looks up by `conversation_id`) always
/// gets empty and never resumes.
///
/// Handles the supported shapes defensively, returning the first non-empty match:
/// - Claude Code **UserPromptSubmit / PreToolUse**: `{ "session_id": "..." }`
///   (Claude Code stamps every hook event with the session id).
/// - Hermes / generic: `conversation_id`, plus the camelCase variants
///   (`sessionId` / `conversationId`) some harnesses emit.
///
/// Returns `None` when no id is present, so the binary leaves `conversation_id`
/// empty and degrades gracefully (no resume, no error).
pub fn extract_conversation_id(json: &serde_json::Value) -> Option<String> {
    for key in [
        "session_id",
        "conversation_id",
        "sessionId",
        "conversationId",
    ] {
        if let Some(s) = json.get(key).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
    }
    None
}

/// Extract the transcript file path from a harness's user-prompt event JSON
/// (context_aware_routing). Claude Code stamps every hook event with
/// `transcript_path` — the JSONL conversation log the hook tails to derive recent
/// context + the in-progress signal. Other harnesses omit it; `None` then, and the
/// hook degrades to no context (routing unchanged).
pub fn extract_transcript_path(json: &serde_json::Value) -> Option<String> {
    for key in ["transcript_path", "transcriptPath"] {
        if let Some(s) = json.get(key).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
    }
    None
}

/// Number of digest lines (user turns + assistant intents) kept in the recent
/// context. Bounded so a long transcript tail can't bloat the router prompt.
const MAX_DIGEST_LINES: usize = 10;
/// Per-turn character cap for the recent-context digest.
const DIGEST_TURN_CAP: usize = 200;
/// Overall character cap for the recent-context digest (belt-and-suspenders atop
/// the line cap).
const DIGEST_TOTAL_CAP: usize = 1600;

/// Parse a bounded transcript tail (Claude Code JSONL — one event per line) into
/// a [`TranscriptContext`]: a compact recent-turns digest + the in-progress
/// playbook signal (context_aware_routing). PURE — the file I/O (seek + tail-read)
/// lives in the hook binary; this is the testable distillation.
///
/// Recent context: the last few real user turns (string content, not tool_result
/// echoes) and short assistant text intents, in order, each truncated.
///
/// In-progress signal (transcript-grounded — no conversation_id join):
/// - a `begin` (anvil lifecycle tool_use / `anvil_orchestrate` / `anvil-hooks
///   begin`) with NO later `complete`/`snapshot` terminal ⇒ OPEN playbook (kind
///   carried from the begin);
/// - playbook-shaped activity (forge-skill / spec-plan-implement file work) with
///   NO begin at all ⇒ work-happening-without-a-begin;
/// - otherwise ⇒ `None`.
///
/// Any unparseable line is skipped; an empty / signal-free tail yields the default
/// (empty digest, `InProgressSignal::None`).
pub fn extract_transcript_context(tail: &str) -> TranscriptContext {
    let mut digest_lines: Vec<String> = Vec::new();
    let mut last_assistant_text = String::new();
    let mut last_assistant_truncated = false;
    // In-progress scan bookkeeping. We track only the presence + relative order of
    // the LAST begin and LAST terminal, which is enough to answer "open begin?".
    let mut last_begin_kind: Option<String> = None;
    let mut last_begin_idx: Option<usize> = None;
    let mut last_terminal_idx: Option<usize> = None;
    let mut saw_playbook_shaped = false;

    for (idx, line) in tail.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let message = event.get("message").unwrap_or(&event);
        let role = message
            .get("role")
            .and_then(|r| r.as_str())
            .or_else(|| event.get("type").and_then(|t| t.as_str()))
            .unwrap_or("");
        let content = message.get("content").or_else(|| event.get("content"));

        // --- recent-context digest ---
        if role == "user" {
            if let Some(text) = user_text(content) {
                let text = text.trim();
                if !text.is_empty() {
                    digest_lines.push(format!("user: {}", truncate(text, DIGEST_TURN_CAP)));
                }
            }
        } else if role == "assistant" {
            if let Some(text) = assistant_text(content) {
                let text = text.trim();
                if !text.is_empty() {
                    // Capture the FULL turn before the digest cap touches it, and
                    // record whether the cap would cut it. Both are decided here,
                    // where the truth is known.
                    last_assistant_text = text.to_string();
                    last_assistant_truncated = text.chars().count() > DIGEST_TURN_CAP;
                    digest_lines
                        .push(format!("assistant: {}", truncate(text, DIGEST_TURN_CAP)));
                }
            }
        }

        // --- in-progress scan over tool_use blocks ---
        for (name, input) in tool_uses(content) {
            match classify_tool(&name, &input) {
                ToolClass::Begin { kind } => {
                    last_begin_idx = Some(idx);
                    last_begin_kind = kind;
                    saw_playbook_shaped = true;
                }
                ToolClass::Terminal => {
                    last_terminal_idx = Some(idx);
                    saw_playbook_shaped = true;
                }
                ToolClass::PlaybookShaped => {
                    saw_playbook_shaped = true;
                }
                ToolClass::Other => {}
            }
        }
    }

    // Keep only the most recent lines, then apply the total cap.
    let start = digest_lines.len().saturating_sub(MAX_DIGEST_LINES);
    let mut recent_context = digest_lines[start..].join("\n");
    if recent_context.len() > DIGEST_TOTAL_CAP {
        recent_context = truncate(&recent_context, DIGEST_TOTAL_CAP);
    }

    let open_begin = match (last_begin_idx, last_terminal_idx) {
        (Some(b), Some(t)) => b > t,
        (Some(_), None) => true,
        _ => false,
    };
    let in_progress = if open_begin {
        InProgressSignal::OpenPlaybookRun {
            kind: last_begin_kind.unwrap_or_default(),
        }
    } else if saw_playbook_shaped && last_begin_idx.is_none() {
        InProgressSignal::WorkWithoutBegin
    } else {
        InProgressSignal::None
    };

    TranscriptContext {
        last_assistant_text,
        last_assistant_truncated,
        recent_context,
        in_progress,
    }
}

/// A tool_use's lifecycle classification for the in-progress scan.
enum ToolClass {
    /// An anvil `begin` / `anvil_orchestrate` / `anvil-hooks begin` — opens a
    /// playbook of the carried kind (when parseable).
    Begin { kind: Option<String> },
    /// A `complete` / `snapshot` — a terminal/advance that closes the open begin.
    Terminal,
    /// Playbook-shaped work without being a begin: a forge lifecycle skill or a
    /// spec/plan/impl/reflection file edit.
    PlaybookShaped,
    /// Not a playbook signal.
    Other,
}

/// Classify a tool_use by name + input for the in-progress scan. Matching is
/// deliberately loose (substring on the tool name, plus Bash-command / Skill /
/// file-path inspection) so it survives MCP tool-name prefixes
/// (`mcp__anvil__begin`), the `anvil_orchestrate` wrapper, and CLI invocations.
fn classify_tool(name: &str, input: &serde_json::Value) -> ToolClass {
    let lname = name.to_lowercase();

    // Bash: inspect the command string for anvil CLI lifecycle calls.
    if lname == "bash" || lname.ends_with("__bash") {
        let cmd = input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_lowercase();
        if cmd.contains("anvil-hooks complete")
            || cmd.contains("anvil-hooks snapshot")
            || cmd.contains(" complete ") && cmd.contains("anvil")
        {
            return ToolClass::Terminal;
        }
        if cmd.contains("anvil-hooks begin")
            || cmd.contains("anvil_orchestrate")
            || cmd.contains("begin --artifact-type")
        {
            return ToolClass::Begin {
                kind: kind_from_command(&cmd),
            };
        }
        return ToolClass::Other;
    }

    // Skill: forge lifecycle skills are playbook-shaped activity.
    if lname == "skill" || lname.ends_with("__skill") {
        let skill = input
            .get("skill")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_lowercase();
        if skill.starts_with("forge:") {
            return ToolClass::PlaybookShaped;
        }
        return ToolClass::Other;
    }

    // Write / Edit into a track's lifecycle artifact is playbook-shaped.
    if lname == "write" || lname == "edit" || lname.ends_with("__write") || lname.ends_with("__edit")
    {
        let path = input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_lowercase();
        if path.contains("/forge/tracks/")
            && (path.ends_with("/spec.md")
                || path.ends_with("/plan.md")
                || path.ends_with("/impl.review.md")
                || path.ends_with("/reflection.md"))
        {
            return ToolClass::PlaybookShaped;
        }
        return ToolClass::Other;
    }

    // MCP / native lifecycle tool names.
    if lname.contains("complete") || lname.contains("snapshot") {
        return ToolClass::Terminal;
    }
    if lname.contains("begin") || lname.contains("orchestrate") {
        let kind = input
            .get("artifact_type")
            .or_else(|| input.get("selection"))
            .or_else(|| input.get("kind"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return ToolClass::Begin { kind };
    }
    ToolClass::Other
}

/// Parse the playbook kind from an anvil begin CLI command string
/// (`--artifact-type <kind>` or `selection: "<kind>"`). `None` when absent.
fn kind_from_command(cmd: &str) -> Option<String> {
    if let Some(rest) = cmd.split("--artifact-type").nth(1) {
        if let Some(tok) = rest.split_whitespace().next() {
            let tok = tok.trim_matches(|c| c == '"' || c == '\'');
            if !tok.is_empty() {
                return Some(tok.to_string());
            }
        }
    }
    if let Some(rest) = cmd.split("selection").nth(1) {
        // selection: "kind" / selection=kind
        let rest = rest.trim_start_matches([':', '=', ' ']);
        let tok = rest
            .trim_start_matches(['"', '\''])
            .split(['"', '\'', ' ', ',', ')'])
            .next()
            .unwrap_or("");
        if !tok.is_empty() {
            return Some(tok.to_string());
        }
    }
    None
}

/// Pull a user turn's plain text out of a transcript content value. Handles the
/// string shape (`content: "..."`) and skips array content that is purely a
/// tool_result echo (not a real user message); a text block in an array is
/// returned. `None` when no user-authored text is present.
fn user_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let mut out = String::new();
            for block in blocks {
                let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if btype == "text" {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        out.push_str(t);
                        out.push(' ');
                    }
                }
            }
            let out = out.trim().to_string();
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        _ => None,
    }
}

/// Pull an assistant turn's short text intent out of a transcript content value.
/// Concatenates the assistant's text blocks (ignoring tool_use blocks). `None`
/// when the turn is tool-only.
fn assistant_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let mut out = String::new();
            for block in blocks {
                let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if btype == "text" {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        out.push_str(t);
                        out.push(' ');
                    }
                }
            }
            let out = out.trim().to_string();
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        _ => None,
    }
}

/// Collect the `(name, input)` of every tool_use block in a content value. The
/// input is cloned (bounded by the transcript tail) so callers needn't juggle a
/// borrow tied to the per-line parse.
fn tool_uses(content: Option<&serde_json::Value>) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    if let Some(serde_json::Value::Array(blocks)) = content {
        for block in blocks {
            let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if btype == "tool_use" {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                out.push((name, input));
            }
        }
    }
    out
}

/// Truncate a string to at most `cap` characters on a char boundary, appending an
/// ellipsis when it was cut.
fn truncate(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(cap.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
