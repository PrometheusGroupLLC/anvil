//! Step module for `playbook_activity_query.feature` and
//! `routing_activity_sink.feature`.
//!
//! The query scenarios exercise the pure `ArtifactActivityQuery` over a real
//! in-memory `PlaybookRegistry` + an in-memory `OwnerResolver` map + a recorded
//! `RoutingActivityRecord` stream. The sink scenarios exercise the real
//! `FileSystemRoutingActivityAdapter` against a temp hearth, proving the
//! append-only + redacted contract.

use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::playbook::types::PlaybookMachine;
use anvil_core::domain::artifact_activity::{
    OwnerResolver, ArtifactActivityQuery, ArtifactActivityResult,
};
use anvil_core_hearth::fs_routing_activity_adapter::FileSystemRoutingActivityAdapter;
use anvil_core::ports::routing_activity_port::{
    RoutingActivityReadPort, RoutingActivityRecord, RoutingActivityWritePort,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;
use std::path::PathBuf;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const MACHINES_KEY: &str = "wa_machines";
const OWNERS_KEY: &str = "wa_owners";
const RECORDS_KEY: &str = "wa_records";
const RESULT_KEY: &str = "wa_result";

// Sink scenario keys
const SINK_HEARTH_KEY: &str = "wa_sink_hearth";
const SINK_HANDLE_KEY: &str = "wa_sink_handle";
const SINK_READ_KEY: &str = "wa_sink_read";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

/// Ordered in-memory registry double for the activity query scenarios.
struct VecRegistry {
    order: Vec<String>,
    map: HashMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for VecRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.map.get(kind).map(|_| format!("{}_dir", kind))
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.order.iter().filter_map(|k| self.map.get(k)).collect()
    }
}

/// In-memory owner map double. Empty value → absent (None).
struct MapOwnerResolver {
    owners: HashMap<String, String>,
}

impl OwnerResolver for MapOwnerResolver {
    fn owner_for(&self, kind: &str) -> Option<String> {
        self.owners
            .get(kind)
            .filter(|v| !v.trim().is_empty())
            .cloned()
    }
}

fn machine(kind: &str, description: &str) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        description: description.to_string(),
        ..Default::default()
    }
}

/// Carry the registry-side inputs (machines + owners) forward across a step
/// that only adds the records. Brine only carries declared inputs+outputs, so
/// every intermediate step in the Given chain must re-emit them.
fn carry_query_inputs(ctx: &Context, out: &mut Context) {
    if let Some(m) = ctx.get::<Vec<PlaybookMachine>>(MACHINES_KEY) {
        out.set(MACHINES_KEY, m.clone());
    }
    if let Some(o) = ctx.get::<HashMap<String, String>>(OWNERS_KEY) {
        out.set(OWNERS_KEY, o.clone());
    }
}

fn run_query(ctx: &Context) -> Result<ArtifactActivityResult, String> {
    let machines = ctx
        .get::<Vec<PlaybookMachine>>(MACHINES_KEY)
        .ok_or("No wa_machines")?
        .clone();
    let owners = ctx
        .get::<HashMap<String, String>>(OWNERS_KEY)
        .cloned()
        .unwrap_or_default();
    let records = ctx
        .get::<Vec<RoutingActivityRecord>>(RECORDS_KEY)
        .cloned()
        .unwrap_or_default();

    let order: Vec<String> = machines.iter().map(|m| m.kind.clone()).collect();
    let map: HashMap<String, PlaybookMachine> =
        machines.into_iter().map(|m| (m.kind.clone(), m)).collect();
    let registry = VecRegistry { order, map };
    let resolver = MapOwnerResolver { owners };
    Ok(ArtifactActivityQuery::execute(
        &registry, &resolver, &records,
    ))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Domain query scenarios =====
        step_def(
            "a playbook activity registry with:",
            &[],
            &[(MACHINES_KEY, "Vec<PlaybookMachine>"), (OWNERS_KEY, "HashMap<String, String>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let kind_col = column_index(table, "kind")?;
                let owner_col = column_index(table, "owner")?;
                let desc_col = column_index(table, "description")?;

                let mut machines = Vec::new();
                let mut owners: HashMap<String, String> = HashMap::new();
                for row in &table.rows {
                    let kind = row[kind_col].trim().to_string();
                    let owner = row[owner_col].trim().to_string();
                    let desc = row[desc_col].trim().to_string();
                    machines.push(machine(&kind, &desc));
                    if !owner.is_empty() {
                        owners.insert(kind, owner);
                    }
                }
                let mut out = Context::new();
                out.set(MACHINES_KEY, machines);
                out.set(OWNERS_KEY, owners);
                Ok(out)
            },
        ),
        step_def(
            "no routing activity has been recorded",
            &[(MACHINES_KEY, "Vec<PlaybookMachine>"), (OWNERS_KEY, "HashMap<String, String>")],
            &[
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (OWNERS_KEY, "HashMap<String, String>"),
                (RECORDS_KEY, "Vec<RoutingActivityRecord>"),
            ],
            |ctx, _params| {
                let mut out = Context::new();
                carry_query_inputs(&ctx, &mut out);
                out.set(RECORDS_KEY, Vec::<RoutingActivityRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "routing activity has been recorded:",
            &[(MACHINES_KEY, "Vec<PlaybookMachine>"), (OWNERS_KEY, "HashMap<String, String>")],
            &[
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (OWNERS_KEY, "HashMap<String, String>"),
                (RECORDS_KEY, "Vec<RoutingActivityRecord>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let kind_col = column_index(table, "kind")?;
                let outcome_col = column_index(table, "outcome")?;
                let at_col = column_index(table, "at")?;
                let mut records = Vec::new();
                for row in &table.rows {
                    records.push(RoutingActivityRecord {
                        kind: row[kind_col].trim().to_string(),
                        outcome: row[outcome_col].trim().to_string(),
                        at: row[at_col].trim().to_string(),
                        conversation_hash: None,
                        project_label: None,
                    });
                }
                let mut out = Context::new();
                carry_query_inputs(&ctx, &mut out);
                out.set(RECORDS_KEY, records);
                Ok(out)
            },
        ),
        step_def(
            "the playbook activity query is executed",
            &[
                (MACHINES_KEY, "Vec<PlaybookMachine>"),
                (OWNERS_KEY, "HashMap<String, String>"),
                (RECORDS_KEY, "Vec<RoutingActivityRecord>"),
            ],
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, _params| {
                let result = run_query(&ctx)?;
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the playbook activity result groups owner {string} with kinds {string}",
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let owner = params.get_string(0).ok_or("Expected owner")?.to_string();
                let expected_kinds: Vec<String> = params
                    .get_string(1)
                    .ok_or("Expected kinds")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let result = ctx
                    .get::<ArtifactActivityResult>(RESULT_KEY)
                    .ok_or("No wa_result")?;
                let group = result
                    .groups
                    .iter()
                    .find(|g| g.owner == owner)
                    .ok_or_else(|| format!("No group for owner '{}'", owner))?;
                let actual: Vec<String> = group.entries.iter().map(|e| e.kind.clone()).collect();
                if actual == expected_kinds {
                    Ok(())
                } else {
                    Err(format!(
                        "Owner '{}': expected kinds {:?}, got {:?}",
                        owner, expected_kinds, actual
                    ))
                }
            },
        ),
        check_def(
            "the playbook activity entry for kind {string} has description {string}",
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_string(1).ok_or("Expected description")?.to_string();
                let result = ctx
                    .get::<ArtifactActivityResult>(RESULT_KEY)
                    .ok_or("No wa_result")?;
                let entry = result
                    .entry_for(&kind)
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                if entry.description == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected description '{}', got '{}'",
                        kind, expected, entry.description
                    ))
                }
            },
        ),
        check_def(
            "the playbook activity entry for kind {string} has owner {string}",
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_string(1).ok_or("Expected owner")?.to_string();
                let result = ctx
                    .get::<ArtifactActivityResult>(RESULT_KEY)
                    .ok_or("No wa_result")?;
                let entry = result
                    .entry_for(&kind)
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                if entry.owner == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected owner '{}', got '{}'",
                        kind, expected, entry.owner
                    ))
                }
            },
        ),
        check_def(
            "the playbook activity entry for kind {string} has call count {int}",
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<ArtifactActivityResult>(RESULT_KEY)
                    .ok_or("No wa_result")?;
                let entry = result
                    .entry_for(&kind)
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                if entry.call_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected call count {}, got {}",
                        kind, expected, entry.call_count
                    ))
                }
            },
        ),
        check_def(
            "the playbook activity result has no entry for kind {string}",
            &[(RESULT_KEY, "ArtifactActivityResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let result = ctx
                    .get::<ArtifactActivityResult>(RESULT_KEY)
                    .ok_or("No wa_result")?;
                if result.entry_for(&kind).is_none() {
                    Ok(())
                } else {
                    Err(format!("Unexpected entry for kind '{}'", kind))
                }
            },
        ),
        // ===== Sink adapter scenarios =====
        step_def(
            "a routing activity hearth",
            &[],
            &[(SINK_HEARTH_KEY, "PathBuf"), (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-routing-activity-")?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, tmp);
                out.set::<RetainedTempDir>(SINK_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a routing activity record is appended with kind {string}, outcome {string}, at {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[(SINK_HEARTH_KEY, "PathBuf"), (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>")],
            |ctx, params| {
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let outcome = params.get_string(1).ok_or("Expected outcome")?.to_string();
                let at = params.get_string(2).ok_or("Expected at")?.to_string();
                let adapter = FileSystemRoutingActivityAdapter::new(&hearth);
                adapter
                    .append_routing_activity(&RoutingActivityRecord {
                        kind,
                        outcome,
                        at,
                        conversation_hash: None,
                        project_label: None,
                    })
                    .map_err(|e| format!("append failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>(SINK_HANDLE_KEY) {
                    out.set::<RetainedTempDir>(SINK_HANDLE_KEY, std::sync::Arc::clone(h));
                }
                Ok(out)
            },
        ),
        step_def(
            "a routing activity record is appended with kind {string}, outcome {string}, at {string}, conversation_hash {string}, project_label {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[(SINK_HEARTH_KEY, "PathBuf"), (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>")],
            |ctx, params| {
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let outcome = params.get_string(1).ok_or("Expected outcome")?.to_string();
                let at = params.get_string(2).ok_or("Expected at")?.to_string();
                let conversation_hash = params
                    .get_string(3)
                    .ok_or("Expected conversation_hash")?
                    .to_string();
                let project_label = params.get_string(4).ok_or("Expected project_label")?.to_string();
                let adapter = FileSystemRoutingActivityAdapter::new(&hearth);
                adapter
                    .append_routing_activity(&RoutingActivityRecord {
                        kind,
                        outcome,
                        at,
                        conversation_hash: Some(conversation_hash),
                        project_label: Some(project_label),
                    })
                    .map_err(|e| format!("append failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>(SINK_HANDLE_KEY) {
                    out.set::<RetainedTempDir>(SINK_HANDLE_KEY, std::sync::Arc::clone(h));
                }
                Ok(out)
            },
        ),
        step_def(
            "the routing activity sink is read without any append",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[(SINK_HEARTH_KEY, "PathBuf"), (SINK_READ_KEY, "Vec<RoutingActivityRecord>")],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let adapter = FileSystemRoutingActivityAdapter::new(&hearth);
                let records = adapter
                    .read_routing_activity()
                    .map_err(|e| format!("read failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                out.set(SINK_READ_KEY, records);
                Ok(out)
            },
        ),
        check_def(
            "the routing activity sink file contains {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let path = hearth.join("routing-activity.jsonl");
                let contents = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read sink file: {}", e))?;
                if contents.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("Sink file does not contain '{}'. Contents:\n{}", needle, contents))
                }
            },
        ),
        check_def(
            "the routing activity sink file does not contain {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let path = hearth.join("routing-activity.jsonl");
                let contents = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read sink file: {}", e))?;
                if contents.contains(&needle) {
                    Err(format!("Sink file unexpectedly contains '{}'. Contents:\n{}", needle, contents))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "reading the routing activity sink returns {int} records",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                // The read-without-append step stores its result; otherwise read live.
                let records = if let Some(r) = ctx.get::<Vec<RoutingActivityRecord>>(SINK_READ_KEY) {
                    r.clone()
                } else {
                    let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                    FileSystemRoutingActivityAdapter::new(&hearth)
                        .read_routing_activity()
                        .map_err(|e| format!("read failed: {}", e))?
                };
                if records.len() == expected {
                    Ok(())
                } else {
                    Err(format!("Expected {} records, got {}", expected, records.len()))
                }
            },
        ),
        check_def(
            "the routing activity records contain a record for kind {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx.get::<PathBuf>(SINK_HEARTH_KEY).ok_or("No sink hearth")?.clone();
                let records = FileSystemRoutingActivityAdapter::new(&hearth)
                    .read_routing_activity()
                    .map_err(|e| format!("read failed: {}", e))?;
                if records.iter().any(|r| r.kind == kind) {
                    Ok(())
                } else {
                    Err(format!("No record for kind '{}'", kind))
                }
            },
        ),
    ]
}
