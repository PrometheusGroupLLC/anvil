//! Step definitions for BP2: the pure `AmendCommandHandler::execute`.
//!
//! Drives the handler with an `InMemoryQueryAdapter` (seeding BOTH kind AND
//! state, MEDIUM-4, plus an optional prior op log) and a `SeedPlaybookRegistry`.
//! In BP2 the seed track machine has no `completed → amend` edge, so
//! `TransitionRecorded` is never emitted.

use anvil_core::domain::amend::{AmendCommandHandler, AmendError, AmendOutcome, AmendRequest};
use anvil_core::domain::amend_events::AmendEvent;
use anvil_core::domain::amendment::{AmendmentOp, OpKind, OpLog};
use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

type AmendDomainResult = Result<AmendOutcome, AmendError>;

fn parse_op_kind(s: &str) -> Result<OpKind, String> {
    match s {
        "add" => Ok(OpKind::Add),
        "revise" => Ok(OpKind::Revise),
        "retire" => Ok(OpKind::Retire),
        "reorder" => Ok(OpKind::Reorder),
        other => Err(format!("Unknown op_kind '{}'", other)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an in-memory amend query adapter with artifact {string} kind {string} state {string}",
            &[],
            &[("amend_qp", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_artifact(&id, &kind, &state);
                let mut out = Context::new();
                out.set("amend_qp", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the amend query adapter has op log for {string} document {string} with op_id {string} accepted_at {string} seq {int} target_id {string} op_kind {string} new_kind {string} body {string}",
            &[("amend_qp", "InMemoryQueryAdapter")],
            &[("amend_qp", "InMemoryQueryAdapter")],
            |mut ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let op_id = params.get_string(2).ok_or("Expected op_id")?.to_string();
                let accepted_at = params.get_string(3).ok_or("Expected accepted_at")?.to_string();
                let _seq = params.get_int(4).ok_or("Expected seq")?;
                let target_id = params.get_string(5).ok_or("Expected target_id")?.to_string();
                let op_kind = parse_op_kind(params.get_string(6).ok_or("Expected op_kind")?)?;
                let new_kind = params.get_string(7).map(|s| s.to_string()).filter(|s| !s.is_empty());
                let body = params.get_string(8).map(|s| s.to_string()).filter(|s| !s.is_empty());
                let mut adapter = ctx.take::<InMemoryQueryAdapter>("amend_qp").ok_or("No amend_qp")?;
                let mut log = adapter.read_op_log_or_empty(&artifact, &document);
                let op = AmendmentOp { target_id, kind: op_kind, body, new_kind, anchor: None };
                log.push(op_id, accepted_at, op);
                adapter.with_op_log(&artifact, &document, log);
                let mut out = Context::new();
                out.set("amend_qp", adapter);
                Ok(out)
            },
        ),
        step_def(
            "amend is called via the seed registry with:",
            &[("amend_qp", "InMemoryQueryAdapter")],
            &[("amend_qp", "InMemoryQueryAdapter"), ("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut request = AmendRequest::default();
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].trim().to_string(), table.headers[1].trim().to_string()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "artifact_path" => request.artifact_path = value,
                        "kind" => request.kind = value,
                        "target_document" => request.target_document = value,
                        "target_id" => request.target_id = value,
                        "op_kind" => request.op_kind = value,
                        "body" => request.body = value,
                        "new_kind" => request.new_kind = value,
                        "anchor" => request.anchor = value,
                        "actor_name" => request.actor_name = value,
                        "actor_type" => request.actor_type = value,
                        "actor_model" => request.actor_model = value,
                        "actor_provider" => request.actor_provider = value,
                        "at" => request.at = value,
                        other => return Err(format!("Unknown amend request key: '{other}'")),
                    }
                }
                let adapter = ctx.get::<InMemoryQueryAdapter>("amend_qp").ok_or("No amend_qp")?.clone();
                let result = AmendCommandHandler::execute(&adapter, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("amend_qp", adapter);
                out.set("amend_result", result);
                Ok(out)
            },
        ),

        // Compact single-line Add driver (for Scenario Outline placeholder use,
        // where data-table cells do not receive Examples substitution).
        step_def(
            "amend Add is called for artifact {string} kind {string} document {string} target_id {string} new_kind {string} body {string} at {string}",
            &[("amend_qp", "InMemoryQueryAdapter")],
            &[("amend_qp", "InMemoryQueryAdapter"), ("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let request = AmendRequest {
                    artifact_path: params.get_string(0).ok_or("Expected artifact")?.to_string(),
                    kind: params.get_string(1).ok_or("Expected kind")?.to_string(),
                    target_document: params.get_string(2).ok_or("Expected document")?.to_string(),
                    target_id: params.get_string(3).ok_or("Expected target_id")?.to_string(),
                    op_kind: "add".to_string(),
                    new_kind: params.get_string(4).ok_or("Expected new_kind")?.to_string(),
                    body: params.get_string(5).ok_or("Expected body")?.to_string(),
                    at: params.get_string(6).ok_or("Expected at")?.to_string(),
                    actor_name: "Doer-300001".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "claude-opus-4-7".to_string(),
                    actor_provider: "anthropic".to_string(),
                    ..Default::default()
                };
                let adapter = ctx.get::<InMemoryQueryAdapter>("amend_qp").ok_or("No amend_qp")?.clone();
                let result = AmendCommandHandler::execute(&adapter, &SeedPlaybookRegistry, request);
                let mut out = Context::new();
                out.set("amend_qp", adapter);
                out.set("amend_result", result);
                Ok(out)
            },
        ),

        // ===================== assertions =====================
        check_def(
            "the amend outcome is successful",
            &[("amend_result", "AmendDomainResult")],
            |ctx, _params| match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            },
        ),
        check_def(
            "the amend outcome is an AmendError containing {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains(needle) { Ok(()) } else { Err(format!("Expected error containing '{}', got: {}", needle, msg)) }
                    }
                    Ok(_) => Err(format!("Expected error containing '{}', got success", needle)),
                }
            },
        ),
        check_def(
            "the amend outcome has {int} events",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Ok(o) if o.events.len() == expected => Ok(()),
                    Ok(o) => Err(format!("Expected {} events, got {}", expected, o.events.len())),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the amend outcome event {int} is OpRecorded",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| event_variant(ctx, params, |e| e.is_op_recorded(), "OpRecorded"),
        ),
        check_def(
            "the amend outcome event {int} is ActorUpserted",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| event_variant(ctx, params, |e| e.is_actor_upserted(), "ActorUpserted"),
        ),
        check_def(
            "the amend outcome event {int} is TransitionRecorded",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| event_variant(ctx, params, |e| e.is_transition_recorded(), "TransitionRecorded"),
        ),
        check_def(
            "the amend outcome op_id is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected op_id")?;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Ok(o) if o.result.op_id == *expected => Ok(()),
                    Ok(o) => Err(format!("Expected op_id '{}', got '{}'", expected, o.result.op_id)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the amend outcome new_state is absent",
            &[("amend_result", "AmendDomainResult")],
            |ctx, _params| match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                Ok(o) if o.result.new_state.is_none() => Ok(()),
                Ok(o) => Err(format!("Expected new_state absent, got {:?}", o.result.new_state)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            },
        ),
        check_def(
            "the amend outcome new_state is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected new_state")?;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Ok(o) => match &o.result.new_state {
                        Some(s) if s == expected => Ok(()),
                        Some(s) => Err(format!("Expected new_state '{}', got '{}'", expected, s)),
                        None => Err(format!("Expected new_state '{}', got absent", expected)),
                    },
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the amend outcome OpRecorded entry op_id is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected op_id")?;
                with_op_recorded(ctx, |ap, _td, entry| {
                    let _ = ap;
                    if entry.op_id == *expected { Ok(()) } else { Err(format!("Expected entry op_id '{}', got '{}'", expected, entry.op_id)) }
                })
            },
        ),
        check_def(
            "the amend outcome OpRecorded entry target_id is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected target_id")?;
                with_op_recorded(ctx, |_ap, _td, entry| {
                    if entry.op.target_id == *expected { Ok(()) } else { Err(format!("Expected entry target_id '{}', got '{}'", expected, entry.op.target_id)) }
                })
            },
        ),
        check_def(
            "the amend outcome OpRecorded target_document is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected target_document")?;
                with_op_recorded(ctx, |_ap, td, _entry| {
                    if td == expected { Ok(()) } else { Err(format!("Expected target_document '{}', got '{}'", expected, td)) }
                })
            },
        ),
        check_def(
            "the amend outcome ActorUpserted actor_name is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor_name")?;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Ok(o) => {
                        for ev in &o.events {
                            if let AmendEvent::ActorUpserted { identity, .. } = ev {
                                return if identity.name == *expected { Ok(()) } else { Err(format!("Expected actor_name '{}', got '{}'", expected, identity.name)) };
                            }
                        }
                        Err("No ActorUpserted event".to_string())
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the amend outcome TransitionRecorded to_state is {string}",
            &[("amend_result", "AmendDomainResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected to_state")?;
                match ctx.get::<AmendDomainResult>("amend_result").ok_or("No amend_result")? {
                    Ok(o) => {
                        for ev in &o.events {
                            if let AmendEvent::TransitionRecorded { to_state, .. } = ev {
                                return if to_state == expected { Ok(()) } else { Err(format!("Expected to_state '{}', got '{}'", expected, to_state)) };
                            }
                        }
                        Err("No TransitionRecorded event".to_string())
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
    ]
}

fn event_variant(
    ctx: Context,
    params: &brine_runner_rust::registry::Params,
    pred: impl Fn(&AmendEvent) -> bool,
    name: &str,
) -> Result<(), String> {
    let idx = params.get_int(0).ok_or("Expected index")? as usize;
    match ctx
        .get::<AmendDomainResult>("amend_result")
        .ok_or("No amend_result")?
    {
        Ok(o) => {
            let ev = o
                .events
                .get(idx)
                .ok_or_else(|| format!("No event at index {}", idx))?;
            if pred(ev) {
                Ok(())
            } else {
                Err(format!("Expected {} at index {}, got {:?}", name, idx, ev))
            }
        }
        Err(e) => Err(format!("Expected success, got error: {}", e)),
    }
}

fn with_op_recorded(
    ctx: Context,
    f: impl Fn(&str, &str, &anvil_core::domain::amendment::OpLogEntry) -> Result<(), String>,
) -> Result<(), String> {
    match ctx
        .get::<AmendDomainResult>("amend_result")
        .ok_or("No amend_result")?
    {
        Ok(o) => {
            for ev in &o.events {
                if let AmendEvent::OpRecorded {
                    artifact_path,
                    target_document,
                    entry,
                } = ev
                {
                    return f(artifact_path, target_document, entry);
                }
            }
            Err("No OpRecorded event".to_string())
        }
        Err(e) => Err(format!("Expected success, got error: {}", e)),
    }
}

// Re-export so the step body can read the current log without depending on the
// adapter's private internals. Provided by InMemoryQueryAdapter (see below).
trait ReadOpLogOrEmpty {
    fn read_op_log_or_empty(&self, artifact: &str, document: &str) -> OpLog;
}

impl ReadOpLogOrEmpty for InMemoryQueryAdapter {
    fn read_op_log_or_empty(&self, artifact: &str, document: &str) -> OpLog {
        use anvil_core::ports::query_port::QueryPort;
        self.read_op_log(artifact, document).unwrap_or_default()
    }
}
