//! anvil-engine-steps
//!
//! Step modules used by exactly ONE brine runner (anvil-engine). Split out of
//! anvil-test-support so no single crate carries every step definition: rustc holds
//! a whole crate's IR at once, so the largest crate sets peak build memory.
//! Shared modules stay in `anvil_test_support`.

pub mod abstention_ledger;
pub mod activity_log_rpc;
pub mod actor_activity_rpc;
pub mod anvil_hooks_bearer;
pub mod anvil_hooks_claimed_evidence;
pub mod anvil_hooks_installer;
pub mod backlog_item_rpc;
pub mod change_record_engine;
pub mod claimed_evidence_rpc;
pub mod command_seam;
pub mod completion_merge_check;
pub mod engine_flags;
pub mod generated_wire_boundary;
pub mod hook_manifest_rpc;
pub mod join_coverage_rpc;
pub mod list_instance_artifacts_rpc;
pub mod live_instances_rpc;
pub mod playbook_activity_rpc;
pub mod playbook_atlas_rpc;
pub mod read_instance_artifact_rpc;
pub mod run_detail_rpc;
pub mod self_install_hooks;
pub mod semantic_route_rpc;
pub mod session_cache;
pub mod step0_stream;
pub mod step_measurement_evidence;
pub mod step_measurement_evidence_fail_open;
pub mod telemetry_egress;
pub mod usage_timeseries_rpc;
pub mod panel_playbooks;
pub mod ws_bridge;
