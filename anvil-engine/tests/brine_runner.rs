use anvil_test_support::{
    checkin,
    engine,
    harness,
    hearth,
    snapshot,
    spark_lifecycle,
};
use anvil_engine_steps::{
    abstention_ledger,
    panel_playbooks,
    activity_log_rpc,
    actor_activity_rpc,
    anvil_hooks_bearer,
    anvil_hooks_claimed_evidence,
    anvil_hooks_installer,
    backlog_item_rpc,
    change_record_engine,
    claimed_evidence_rpc,
    command_seam,
    completion_merge_check,
    engine_flags,
    generated_wire_boundary,
    hook_manifest_rpc,
    join_coverage_rpc,
    list_instance_artifacts_rpc,
    live_instances_rpc,
    playbook_atlas_rpc,
    read_instance_artifact_rpc,
    run_detail_rpc,
    self_install_hooks,
    semantic_route_rpc,
    session_cache,
    step0_stream,
    step_measurement_evidence,
    step_measurement_evidence_fail_open,
    telemetry_egress,
    usage_timeseries_rpc,
    playbook_activity_rpc,
    ws_bridge,
};

// Compile the production binary implementation as a private test module so
// P2b can construct the real private AnvilServer with an injected durable
// writer. This keeps the production server surface private and avoids a fake
// tonic implementation in the behavior test.
#[path = "steps/all_kinds_measurement_matrix.rs"]
mod all_kinds_measurement_matrix;
#[path = "steps/crate_size_ratchet.rs"]
mod crate_size_ratchet;
#[path = "../src/main.rs"]
mod production_engine;
#[path = "steps/step_measurement_evidence_fail_open_in_process.rs"]
mod step_measurement_evidence_fail_open_in_process;
#[path = "steps/playbook_measurement_coverage_gap.rs"]
mod playbook_measurement_coverage_gap;
#[path = "steps/canonical_playbook_language_wire.rs"]
mod canonical_playbook_language_wire;

fn main() {
    // The startup self-install behavior is covered against temp roots by
    // engine_self_installs_hooks.feature. All other engine subprocess scenarios
    // opt out so Brine never mutates the developer's real harness configs.
    std::env::set_var("ANVIL_SKIP_HOOK_INSTALL", "1");
    harness::ensure_binary("anvil-engine");
    harness::ensure_binary("anvil-hooks");
    let domain_steps = vec![
        (
            "tests/steps/backlog_item_rpc.rs",
            backlog_item_rpc::steps(),
        ),
        (
            "tests/steps/change_record_engine.rs",
            change_record_engine::steps(),
        ),
        (
            "tests/steps/anvil_hooks_installer.rs",
            anvil_hooks_installer::steps(),
        ),
        (
            "tests/steps/anvil_hooks_claimed_evidence.rs",
            anvil_hooks_claimed_evidence::steps(),
        ),
        (
            "tests/steps/anvil_hooks_bearer.rs",
            anvil_hooks_bearer::steps(),
        ),
        (
            "tests/steps/self_install_hooks.rs",
            self_install_hooks::steps(),
        ),
        ("tests/steps/hearth.rs", hearth::steps()),
        ("tests/steps/checkin.rs", checkin::steps()),
        ("tests/steps/engine.rs", engine::steps()),
        (
            "tests/steps/command_seam.rs",
            command_seam::steps(),
        ),
        (
            "tests/steps/claimed_evidence_rpc.rs",
            claimed_evidence_rpc::steps(),
        ),
        (
            "tests/steps/completion_merge_check.rs",
            completion_merge_check::steps(),
        ),
        ("tests/steps/spark_lifecycle.rs", spark_lifecycle::steps()),
        ("tests/steps/snapshot.rs", snapshot::steps()),
        ("tests/steps/session_cache.rs", session_cache::steps()),
        (
            "tests/steps/playbook_activity_rpc.rs",
            playbook_activity_rpc::steps(),
        ),
        ("tests/steps/panel_playbooks.rs", panel_playbooks::steps()),
        ("tests/steps/ws_bridge.rs", ws_bridge::steps()),
        (
            "tests/steps/hook_manifest_rpc.rs",
            hook_manifest_rpc::steps(),
        ),
        (
            "tests/steps/playbook_atlas_rpc.rs",
            playbook_atlas_rpc::steps(),
        ),
        (
            "tests/steps/live_instances_rpc.rs",
            live_instances_rpc::steps(),
        ),
        (
            "tests/steps/join_coverage_rpc.rs",
            join_coverage_rpc::steps(),
        ),
        (
            "tests/steps/list_instance_artifacts_rpc.rs",
            list_instance_artifacts_rpc::steps(),
        ),
        (
            "tests/steps/read_instance_artifact_rpc.rs",
            read_instance_artifact_rpc::steps(),
        ),
        ("tests/steps/run_detail_rpc.rs", run_detail_rpc::steps()),
        (
            "tests/steps/usage_timeseries_rpc.rs",
            usage_timeseries_rpc::steps(),
        ),
        (
            "tests/steps/actor_activity_rpc.rs",
            actor_activity_rpc::steps(),
        ),
        ("tests/steps/activity_log_rpc.rs", activity_log_rpc::steps()),
        ("tests/steps/step0_stream.rs", step0_stream::steps()),
        (
            "tests/steps/step_measurement_evidence.rs",
            step_measurement_evidence::steps(),
        ),
        (
            "tests/steps/step_measurement_evidence_fail_open.rs",
            step_measurement_evidence_fail_open::steps(),
        ),
        (
            "tests/steps/step_measurement_evidence_fail_open_in_process.rs",
            step_measurement_evidence_fail_open_in_process::steps(),
        ),
        (
            "tests/steps/playbook_measurement_coverage_gap.rs",
            playbook_measurement_coverage_gap::steps(),
        ),
        (
            "tests/steps/all_kinds_measurement_matrix.rs",
            all_kinds_measurement_matrix::steps(),
        ),
        (
            "tests/steps/crate_size_ratchet.rs",
            crate_size_ratchet::steps(),
        ),
        (
            "tests/steps/semantic_route_rpc.rs",
            semantic_route_rpc::steps(),
        ),
        (
            "tests/steps/abstention_ledger.rs",
            abstention_ledger::steps(),
        ),
        ("tests/steps/engine_flags.rs", engine_flags::steps()),
        (
            "tests/steps/telemetry_egress.rs",
            telemetry_egress::steps(),
        ),
        (
            "tests/steps/generated_wire_boundary.rs",
            generated_wire_boundary::steps(),
        ),
        (
            "tests/steps/canonical_playbook_language_wire.rs",
            canonical_playbook_language_wire::steps(),
        ),
    ];
    harness::run_main_with_default_features(
        domain_steps,
        harness::default_features_from_workspace_patterns(&["anvil-engine/features/**/*.feature"]),
    );
}
