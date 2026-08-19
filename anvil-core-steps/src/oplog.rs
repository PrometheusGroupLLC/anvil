//! Step definitions for BP1: the `OpLogWritePort` append seam (fs + in-memory)
//! and `QueryPort::read_op_log`. Real adapters against a Fixture-style temp
//! hearth for the fs scenarios; the in-memory op-log adapter for the parity
//! scenario; the in-memory query adapter for the read-back-empty/seeded cases.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::amendment::{AmendmentOp, OpKind, OpLog, OpLogEntry};
use anvil_core_hearth::fs_op_log_adapter::FileSystemOpLogAdapter;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::in_memory_op_log_adapter::InMemoryOpLogAdapter;
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use anvil_core::ports::op_log_write_port::{op_log_file_name, OpLogWritePort};
use anvil_core::ports::query_port::QueryPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::path::PathBuf;
use std::sync::Arc;

/// Parse an op_kind string to an `OpKind`.
fn parse_op_kind(s: &str) -> Result<OpKind, String> {
    match s {
        "add" => Ok(OpKind::Add),
        "revise" => Ok(OpKind::Revise),
        "retire" => Ok(OpKind::Retire),
        "reorder" => Ok(OpKind::Reorder),
        other => Err(format!("Unknown op_kind '{}'", other)),
    }
}

/// Build an `OpLogEntry` from the positional params used by the append/seed
/// steps: op_id(0) accepted_at(1) seq(2) target_id(3) op_kind(4) new_kind(5)
/// body(6). Empty new_kind/body become `None`.
fn entry_from_params(params: &Params, base: usize) -> Result<OpLogEntry, String> {
    let op_id = params.get_string(base).ok_or("Expected op_id")?.to_string();
    let accepted_at = params
        .get_string(base + 1)
        .ok_or("Expected accepted_at")?
        .to_string();
    let seq = params.get_int(base + 2).ok_or("Expected seq")? as u64;
    let target_id = params
        .get_string(base + 3)
        .ok_or("Expected target_id")?
        .to_string();
    let op_kind = parse_op_kind(params.get_string(base + 4).ok_or("Expected op_kind")?)?;
    let new_kind = params.get_string(base + 5).map(|s| s.to_string());
    let body = params.get_string(base + 6).map(|s| s.to_string());
    let op = AmendmentOp {
        target_id,
        kind: op_kind,
        body: body.filter(|s| !s.is_empty()),
        new_kind: new_kind.filter(|s| !s.is_empty()),
        anchor: None,
    };
    Ok(OpLogEntry {
        op_id,
        accepted_at,
        seq,
        op,
    })
}

type ReadOpLogOutcome = OpLog;

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===================== fs write adapter =====================
        step_def(
            "an op-log write fs hearth at {string}",
            &[],
            &[
                ("oplog_fs_hearth", "PathBuf"),
                ("oplog_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("oplog_fs_artifact", "String"),
            ],
            |_ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-oplog-write-")?;
                std::fs::create_dir_all(tmp.join(&artifact))
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                let mut out = Context::new();
                out.set("oplog_fs_hearth", tmp);
                out.set("oplog_fs_hearth_handle", handle);
                out.set("oplog_fs_artifact", artifact);
                Ok(out)
            },
        ),
        step_def(
            "append_op on fs is called for {string} document {string} with op_id {string} accepted_at {string} seq {int} target_id {string} op_kind {string} new_kind {string} body {string}",
            &[("oplog_fs_hearth", "PathBuf")],
            &[
                ("oplog_fs_hearth", "PathBuf"),
                ("oplog_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let entry = entry_from_params(&params, 2)?;
                let hearth = ctx
                    .get::<PathBuf>("oplog_fs_hearth")
                    .ok_or("No oplog_fs_hearth")?
                    .clone();
                let adapter = FileSystemOpLogAdapter::new(hearth.clone());
                adapter
                    .append_op(&artifact, &document, &entry)
                    .map_err(|e| format!("append_op failed: {}", e))?;
                let mut out = Context::new();
                out.set("oplog_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "oplog_fs_hearth_handle");
                Ok(out)
            },
        ),
        check_def(
            "the fs op-log file at {string} document {string} deserializes to a log of length {int}",
            &[("oplog_fs_hearth", "PathBuf")],
            |ctx, params| {
                let log = read_fs_log(&ctx, &params)?;
                let expected = params.get_int(2).ok_or("Expected length")? as usize;
                if log.len() == expected {
                    Ok(())
                } else {
                    Err(format!("Expected length {}, got {}", expected, log.len()))
                }
            },
        ),
        check_def(
            "the fs op-log file at {string} document {string} entry {int} has op_id {string}",
            &[("oplog_fs_hearth", "PathBuf")],
            |ctx, params| {
                let log = read_fs_log(&ctx, &params)?;
                let idx = params.get_int(2).ok_or("Expected index")? as usize;
                let expected = params.get_string(3).ok_or("Expected op_id")?;
                let entry = log.entries().get(idx).ok_or_else(|| format!("No entry {}", idx))?;
                if entry.op_id == expected {
                    Ok(())
                } else {
                    Err(format!("Expected op_id '{}', got '{}'", expected, entry.op_id))
                }
            },
        ),
        check_def(
            "the fs op-log file at {string} document {string} entry {int} has target_id {string}",
            &[("oplog_fs_hearth", "PathBuf")],
            |ctx, params| {
                let log = read_fs_log(&ctx, &params)?;
                let idx = params.get_int(2).ok_or("Expected index")? as usize;
                let expected = params.get_string(3).ok_or("Expected target_id")?;
                let entry = log.entries().get(idx).ok_or_else(|| format!("No entry {}", idx))?;
                if entry.op.target_id == expected {
                    Ok(())
                } else {
                    Err(format!("Expected target_id '{}', got '{}'", expected, entry.op.target_id))
                }
            },
        ),
        check_def(
            "the fs op-log file at {string} document {string} ordered op_id sequence is {string}",
            &[("oplog_fs_hearth", "PathBuf")],
            |ctx, params| {
                let log = read_fs_log(&ctx, &params)?;
                let expected = params.get_string(2).ok_or("Expected sequence")?;
                let actual: Vec<String> =
                    log.ordered().into_iter().map(|e| e.op_id).collect();
                if actual.join(",") == *expected {
                    Ok(())
                } else {
                    Err(format!("Expected ordered '{}', got '{}'", expected, actual.join(",")))
                }
            },
        ),

        // ===================== in-memory write adapter =====================
        step_def(
            "an in-memory op-log adapter",
            &[],
            &[("oplog_mem_adapter", "Arc<InMemoryOpLogAdapter>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set::<Arc<InMemoryOpLogAdapter>>(
                    "oplog_mem_adapter",
                    Arc::new(InMemoryOpLogAdapter::new()),
                );
                Ok(out)
            },
        ),
        step_def(
            "append_op in memory is called for {string} document {string} with op_id {string} accepted_at {string} seq {int} target_id {string} op_kind {string} new_kind {string} body {string}",
            &[("oplog_mem_adapter", "Arc<InMemoryOpLogAdapter>")],
            &[("oplog_mem_adapter", "Arc<InMemoryOpLogAdapter>")],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let entry = entry_from_params(&params, 2)?;
                let adapter = ctx
                    .get::<Arc<InMemoryOpLogAdapter>>("oplog_mem_adapter")
                    .ok_or("No oplog_mem_adapter")?
                    .clone();
                adapter
                    .append_op(&artifact, &document, &entry)
                    .map_err(|e| format!("append_op failed: {}", e))?;
                let mut out = Context::new();
                out.set::<Arc<InMemoryOpLogAdapter>>("oplog_mem_adapter", adapter);
                Ok(out)
            },
        ),
        check_def(
            "the in-memory op-log for {string} document {string} has length {int}",
            &[("oplog_mem_adapter", "Arc<InMemoryOpLogAdapter>")],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?;
                let document = params.get_string(1).ok_or("Expected document")?;
                let expected = params.get_int(2).ok_or("Expected length")? as usize;
                let adapter = ctx
                    .get::<Arc<InMemoryOpLogAdapter>>("oplog_mem_adapter")
                    .ok_or("No oplog_mem_adapter")?;
                let log = adapter.op_log(artifact, document);
                if log.len() == expected {
                    Ok(())
                } else {
                    Err(format!("Expected length {}, got {}", expected, log.len()))
                }
            },
        ),
        check_def(
            "the in-memory op-log for {string} document {string} ordered op_id sequence is {string}",
            &[("oplog_mem_adapter", "Arc<InMemoryOpLogAdapter>")],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?;
                let document = params.get_string(1).ok_or("Expected document")?;
                let expected = params.get_string(2).ok_or("Expected sequence")?;
                let adapter = ctx
                    .get::<Arc<InMemoryOpLogAdapter>>("oplog_mem_adapter")
                    .ok_or("No oplog_mem_adapter")?;
                let actual: Vec<String> =
                    adapter.op_log(artifact, document).ordered().into_iter().map(|e| e.op_id).collect();
                if actual.join(",") == *expected {
                    Ok(())
                } else {
                    Err(format!("Expected ordered '{}', got '{}'", expected, actual.join(",")))
                }
            },
        ),

        // ===================== fs read (QueryPort::read_op_log) =====================
        step_def(
            "an op-log read fs hearth at {string}",
            &[],
            &[
                ("oplog_read_fs_hearth", "PathBuf"),
                ("oplog_read_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-oplog-read-")?;
                std::fs::create_dir_all(tmp.join(&artifact))
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                let mut out = Context::new();
                out.set("oplog_read_fs_hearth", tmp);
                out.set("oplog_read_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the fs op-log for {string} document {string} has been seeded with op_id {string} accepted_at {string} seq {int} target_id {string} op_kind {string} new_kind {string} body {string}",
            &[("oplog_read_fs_hearth", "PathBuf")],
            &[
                ("oplog_read_fs_hearth", "PathBuf"),
                ("oplog_read_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let entry = entry_from_params(&params, 2)?;
                let hearth = ctx
                    .get::<PathBuf>("oplog_read_fs_hearth")
                    .ok_or("No oplog_read_fs_hearth")?
                    .clone();
                let mut log = OpLog::new();
                log.push_entry(entry);
                let serialized = serde_yaml::to_string(&log)
                    .map_err(|e| format!("serialize failed: {}", e))?;
                let file = hearth.join(&artifact).join(op_log_file_name(&document));
                std::fs::write(&file, serialized).map_err(|e| format!("write failed: {}", e))?;
                let mut out = Context::new();
                out.set("oplog_read_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "oplog_read_fs_hearth_handle");
                Ok(out)
            },
        ),
        step_def(
            "read_op_log on fs is called for {string} document {string}",
            &[("oplog_read_fs_hearth", "PathBuf")],
            &[
                ("oplog_read_fs_hearth", "PathBuf"),
                ("oplog_read_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("read_op_log_result", "ReadOpLogOutcome"),
            ],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("oplog_read_fs_hearth")
                    .ok_or("No oplog_read_fs_hearth")?
                    .clone();
                let adapter = FileSystemQueryAdapter::new(hearth.clone());
                let log = adapter
                    .read_op_log(&artifact, &document)
                    .map_err(|e| format!("read_op_log failed: {}", e))?;
                let mut out = Context::new();
                out.set("oplog_read_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "oplog_read_fs_hearth_handle");
                out.set::<ReadOpLogOutcome>("read_op_log_result", log);
                Ok(out)
            },
        ),
        check_def(
            "the read op log is empty",
            &[("read_op_log_result", "ReadOpLogOutcome")],
            |ctx, _params| {
                let log = ctx.get::<ReadOpLogOutcome>("read_op_log_result").ok_or("No read_op_log_result")?;
                if log.is_empty() { Ok(()) } else { Err(format!("Expected empty, got length {}", log.len())) }
            },
        ),
        check_def(
            "the read op log has length {int}",
            &[("read_op_log_result", "ReadOpLogOutcome")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected length")? as usize;
                let log = ctx.get::<ReadOpLogOutcome>("read_op_log_result").ok_or("No read_op_log_result")?;
                if log.len() == expected { Ok(()) } else { Err(format!("Expected length {}, got {}", expected, log.len())) }
            },
        ),
        check_def(
            "the read op log entry {int} has op_id {string}",
            &[("read_op_log_result", "ReadOpLogOutcome")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected index")? as usize;
                let expected = params.get_string(1).ok_or("Expected op_id")?;
                let log = ctx.get::<ReadOpLogOutcome>("read_op_log_result").ok_or("No read_op_log_result")?;
                let entry = log.entries().get(idx).ok_or_else(|| format!("No entry {}", idx))?;
                if entry.op_id == expected { Ok(()) } else { Err(format!("Expected op_id '{}', got '{}'", expected, entry.op_id)) }
            },
        ),

        // ===================== in-memory read (QueryPort::read_op_log) =====================
        step_def(
            "an in-memory query adapter seeded with op log for artifact {string} document {string} with op_id {string} accepted_at {string} seq {int} target_id {string} op_kind {string} new_kind {string} body {string}",
            &[],
            &[("qp_adapter", "InMemoryQueryAdapter")],
            |_ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let entry = entry_from_params(&params, 2)?;
                let mut log = OpLog::new();
                log.push_entry(entry);
                let mut adapter = InMemoryQueryAdapter::new();
                adapter.with_op_log(&artifact, &document, log);
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "read_op_log in memory is called for {string} document {string}",
            &[("qp_adapter", "InMemoryQueryAdapter")],
            &[("qp_adapter", "InMemoryQueryAdapter"), ("mem_read_op_log_result", "ReadOpLogOutcome")],
            |mut ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let document = params.get_string(1).ok_or("Expected document")?.to_string();
                let adapter = ctx.take::<InMemoryQueryAdapter>("qp_adapter").ok_or("No qp_adapter")?;
                let log = adapter
                    .read_op_log(&artifact, &document)
                    .map_err(|e| format!("read_op_log failed: {}", e))?;
                let mut out = Context::new();
                out.set("qp_adapter", adapter);
                out.set::<ReadOpLogOutcome>("mem_read_op_log_result", log);
                Ok(out)
            },
        ),
        check_def(
            "the in-memory read op log is empty",
            &[("mem_read_op_log_result", "ReadOpLogOutcome")],
            |ctx, _params| {
                let log = ctx.get::<ReadOpLogOutcome>("mem_read_op_log_result").ok_or("No mem_read_op_log_result")?;
                if log.is_empty() { Ok(()) } else { Err(format!("Expected empty, got length {}", log.len())) }
            },
        ),
        check_def(
            "the in-memory read op log has length {int}",
            &[("mem_read_op_log_result", "ReadOpLogOutcome")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected length")? as usize;
                let log = ctx.get::<ReadOpLogOutcome>("mem_read_op_log_result").ok_or("No mem_read_op_log_result")?;
                if log.len() == expected { Ok(()) } else { Err(format!("Expected length {}, got {}", expected, log.len())) }
            },
        ),
        check_def(
            "the in-memory read op log entry {int} has op_id {string}",
            &[("mem_read_op_log_result", "ReadOpLogOutcome")],
            |ctx, params| {
                let idx = params.get_int(0).ok_or("Expected index")? as usize;
                let expected = params.get_string(1).ok_or("Expected op_id")?;
                let log = ctx.get::<ReadOpLogOutcome>("mem_read_op_log_result").ok_or("No mem_read_op_log_result")?;
                let entry = log.entries().get(idx).ok_or_else(|| format!("No entry {}", idx))?;
                if entry.op_id == expected { Ok(()) } else { Err(format!("Expected op_id '{}', got '{}'", expected, entry.op_id)) }
            },
        ),
    ]
}

fn read_fs_log(ctx: &Context, params: &Params) -> Result<OpLog, String> {
    let artifact = params.get_string(0).ok_or("Expected artifact")?;
    let document = params.get_string(1).ok_or("Expected document")?;
    let hearth = ctx
        .get::<PathBuf>("oplog_fs_hearth")
        .ok_or("No oplog_fs_hearth")?;
    let file = hearth.join(artifact).join(op_log_file_name(document));
    let content = std::fs::read_to_string(&file).map_err(|e| format!("read failed: {}", e))?;
    serde_yaml::from_str(&content).map_err(|e| format!("deserialize failed: {}", e))
}
