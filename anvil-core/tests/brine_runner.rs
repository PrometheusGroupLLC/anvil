use anvil_core_steps::codex_plugin_package;
use anvil_test_support::{
    backlog_item,
    builder,
    checkin,
    harness,
    hearth,
    query_port,
    snapshot,
    spark_lifecycle,
};
use anvil_core_steps::{
    activity_summary,
    router_degradation,
    join_episode,
    activity_write,
    actor_activity,
    actor_write,
    agent_hook_paths_resolve,
    hearth_fail_loud_lint,
    hearth_port_reachability,
    hearth_port_syscall_coverage,
    hearth_write_refusal,
    kit_playbook_bijection,
    playbook_registry_projection,
    residual_token_allowlist,
    internal_identifier_boundary,
    semantic_identifier_families,
    amend_handler,
    amendment_apply,
    amendment_conflict,
    amendment_kinds,
    amendment_plan,
    amendment_reverse,
    amendment_schema,
    amendment_serde,
    artifact_containment,
    artifact_kind_dual_read,
    atomic_write,
    candidate_playbook_generate,
    change_record,
    complete,
    decision_lifecycle,
    describe_available_actions,
    describe_fs,
    enforcement_bundle_check,
    evidence_assessment,
    merge_check_classification,
    evidence_obligation_gate,
    evidence_obligation_predicate,
    evidence_obligation_schema,
    exemplar_schema,
    hearth_discovery,
    hearth_locks,
    hook_manifest_fold,
    hook_serve_seam,
    hooks_adapter,
    hooks_gate_check,
    hooks_mcp_registration,
    hooks_route_turn,
    initiative_lifecycle,
    learning_lifecycle,
    live_instances_fold,
    loader_measurement_enforcement,
    milestone_lifecycle,
    oplog,
    outcome_predicate_fold,
    persist_playbook_adapter,
    persist_playbook_handler,
    playbook_seed_yaml_equivalence,
    playbook_seeds,
    playbook_version,
    proposal_lifecycle,
    quality_rubric,
    registry_enumeration,
    resolved_state,
    artifact_of_record,
    transition_evidence_fields,
    continuation_lexicons,
    resume_routing,
    route_call_state,
    route_handler,
    route_resolution,
    run_detail,
    autonomy_evidence,
    transition_verdict_fold,
    scorecard_agg,
    seed_source_parity,
    session,
    status_header,
    step0_stream_adapter,
    step_exemplar_schema,
    step_measurement_evidence_reader,
    step_measurement_sink,
    step_two_by_two,
    survivor_outcome,
    telemetry_salt,
    track_lifecycle_evidence_obligation,
    transition_event_store,
    transition_log,
    usage_timeseries,
    playbook_activity,
    playbook_composite_registry,
    playbook_fidelity,
    playbook_hearth_registry,
    playbook_hook_declaration_selection,
    playbook_interpreter,
    playbook_loader,
    playbook_measurement_selection,
    playbook_next_step,
    playbook_register_field,
    playbook_registry,
    playbook_skills_ownership_language,
};

fn main() {
    // C-d.1 round 8 — the syscall-coverage instrument re-enters this binary as
    // its own probe child, with the filesystem recorder injected by the dynamic
    // loader. Re-entry (rather than a second build target) is what makes the
    // probe run the SAME adapter code the suite runs, in a process the loader
    // is free to interpose. See `hearth_port_syscall_coverage`.
    if let Ok(hearth) = std::env::var(hearth_port_syscall_coverage::PROBE_ENV) {
        // C-d.1 round 9, M-2: the probe now sweeps the readable control AND
        // every degraded fixture the matrix builds, so the tape carries the
        // ERROR-BRANCH reads too. The variants root is required — a probe that
        // silently ran the control alone would restore exactly the bound this
        // closes.
        let variants = match std::env::var(hearth_port_syscall_coverage::VARIANTS_ENV) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "fstrace probe: {} is unset ({e}). The probe measures the degraded arms as \
                     well as the control and will not fall back to the control alone.",
                    hearth_port_syscall_coverage::VARIANTS_ENV
                );
                std::process::exit(2);
            }
        };
        if let Err(e) = hearth_port_syscall_coverage::run_probe(
            std::path::Path::new(&hearth),
            std::path::Path::new(&variants),
        ) {
            eprintln!("fstrace probe failed: {e}");
            std::process::exit(2);
        }
        return;
    }
    let domain_steps = vec![
        ("tests/steps/backlog_item.rs", backlog_item::steps()),
        ("tests/steps/builder.rs", builder::steps()),
        (
            "tests/steps/artifact_containment.rs",
            artifact_containment::steps(),
        ),
        (
            "tests/steps/router_degradation.rs",
            router_degradation::steps(),
        ),
        ("tests/steps/join_episode.rs", join_episode::steps()),
        (
            "tests/steps/artifact_kind_dual_read.rs",
            artifact_kind_dual_read::steps(),
        ),
        (
            "tests/steps/candidate_playbook_generate.rs",
            candidate_playbook_generate::steps(),
        ),
        ("tests/steps/change_record.rs", change_record::steps()),
        ("tests/steps/amendment_schema.rs", amendment_schema::steps()),
        ("tests/steps/amendment_apply.rs", amendment_apply::steps()),
        (
            "tests/steps/amendment_conflict.rs",
            amendment_conflict::steps(),
        ),
        (
            "tests/steps/amendment_reverse.rs",
            amendment_reverse::steps(),
        ),
        ("tests/steps/amendment_serde.rs", amendment_serde::steps()),
        ("tests/steps/amendment_kinds.rs", amendment_kinds::steps()),
        ("tests/steps/amendment_plan.rs", amendment_plan::steps()),
        ("tests/steps/hearth.rs", hearth::steps()),
        ("tests/steps/hearth_discovery.rs", hearth_discovery::steps()),
        (
            "tests/steps/hook_manifest_fold.rs",
            hook_manifest_fold::steps(),
        ),
        ("tests/steps/hook_serve_seam.rs", hook_serve_seam::steps()),
        ("tests/steps/hooks_adapter.rs", hooks_adapter::steps()),
        (
            "tests/steps/codex_plugin_package.rs",
            codex_plugin_package::steps(),
        ),
        ("tests/steps/hooks_gate_check.rs", hooks_gate_check::steps()),
        (
            "tests/steps/hooks_mcp_registration.rs",
            hooks_mcp_registration::steps(),
        ),
        ("tests/steps/hooks_route_turn.rs", hooks_route_turn::steps()),
        ("tests/steps/checkin.rs", checkin::steps()),
        ("tests/steps/snapshot.rs", snapshot::steps()),
        ("tests/steps/spark_lifecycle.rs", spark_lifecycle::steps()),
        (
            "tests/steps/step_measurement_sink.rs",
            step_measurement_sink::steps(),
        ),
        (
            "tests/steps/step_measurement_evidence_reader.rs",
            step_measurement_evidence_reader::steps(),
        ),
        ("tests/steps/atomic_write.rs", atomic_write::steps()),
        ("tests/steps/hearth_locks.rs", hearth_locks::steps()),
        ("tests/steps/actor_write.rs", actor_write::steps()),
        ("tests/steps/actor_activity.rs", actor_activity::steps()),
        ("tests/steps/activity_write.rs", activity_write::steps()),
        ("tests/steps/complete.rs", complete::steps()),
        (
            "tests/steps/decision_lifecycle.rs",
            decision_lifecycle::steps(),
        ),
        (
            "tests/steps/initiative_lifecycle.rs",
            initiative_lifecycle::steps(),
        ),
        (
            "tests/steps/learning_lifecycle.rs",
            learning_lifecycle::steps(),
        ),
        (
            "tests/steps/milestone_lifecycle.rs",
            milestone_lifecycle::steps(),
        ),
        (
            "tests/steps/proposal_lifecycle.rs",
            proposal_lifecycle::steps(),
        ),
        ("tests/steps/amend_handler.rs", amend_handler::steps()),
        ("tests/steps/oplog.rs", oplog::steps()),
        (
            "tests/steps/persist_playbook_adapter.rs",
            persist_playbook_adapter::steps(),
        ),
        (
            "tests/steps/persist_playbook_handler.rs",
            persist_playbook_handler::steps(),
        ),
        ("tests/steps/playbook_version.rs", playbook_version::steps()),
        ("tests/steps/quality_rubric.rs", quality_rubric::steps()),
        ("tests/steps/exemplar_schema.rs", exemplar_schema::steps()),
        ("tests/steps/query_port.rs", query_port::steps()),
        (
            "tests/steps/registry_enumeration.rs",
            registry_enumeration::steps(),
        ),
        ("tests/steps/resolved_state.rs", resolved_state::steps()),
        (
            "tests/steps/transition_event_store.rs",
            transition_event_store::steps(),
        ),
        ("tests/steps/transition_log.rs", transition_log::steps()),
        (
            "tests/steps/transition_evidence_fields.rs",
            transition_evidence_fields::steps(),
        ),
        (
            "tests/steps/artifact_of_record.rs",
            artifact_of_record::steps(),
        ),
        (
            "tests/steps/continuation_lexicons.rs",
            continuation_lexicons::steps(),
        ),
        ("tests/steps/resume_routing.rs", resume_routing::steps()),
        ("tests/steps/route_call_state.rs", route_call_state::steps()),
        ("tests/steps/route_handler.rs", route_handler::steps()),
        ("tests/steps/route_resolution.rs", route_resolution::steps()),
        ("tests/steps/run_detail.rs", run_detail::steps()),
        (
            "tests/steps/autonomy_evidence.rs",
            autonomy_evidence::steps(),
        ),
        (
            "tests/steps/transition_verdict_fold.rs",
            transition_verdict_fold::steps(),
        ),
        ("tests/steps/playbook_loader.rs", playbook_loader::steps()),
        (
            "tests/steps/loader_measurement_enforcement.rs",
            loader_measurement_enforcement::steps(),
        ),
        (
            "tests/steps/playbook_interpreter.rs",
            playbook_interpreter::steps(),
        ),
        (
            "tests/steps/playbook_registry.rs",
            playbook_registry::steps(),
        ),
        (
            "tests/steps/playbook_composite_registry.rs",
            playbook_composite_registry::steps(),
        ),
        (
            "tests/steps/playbook_fidelity.rs",
            playbook_fidelity::steps(),
        ),
        (
            "tests/steps/playbook_hearth_registry.rs",
            playbook_hearth_registry::steps(),
        ),
        (
            "tests/steps/hearth_fail_loud_lint.rs",
            hearth_fail_loud_lint::steps(),
        ),
        (
            "tests/steps/playbook_registry_projection.rs",
            playbook_registry_projection::steps(),
        ),
        (
            "tests/steps/playbook_hook_declaration_selection.rs",
            playbook_hook_declaration_selection::steps(),
        ),
        (
            "tests/steps/playbook_measurement_selection.rs",
            playbook_measurement_selection::steps(),
        ),
        (
            "tests/steps/playbook_next_step.rs",
            playbook_next_step::steps(),
        ),
        (
            "tests/steps/playbook_register_field.rs",
            playbook_register_field::steps(),
        ),
        (
            "tests/steps/playbook_seed_yaml_equivalence.rs",
            playbook_seed_yaml_equivalence::steps(),
        ),
        ("tests/steps/playbook_seeds.rs", playbook_seeds::steps()),
        (
            "tests/steps/playbook_skills_ownership_language.rs",
            playbook_skills_ownership_language::steps(),
        ),
        (
            "tests/steps/describe_available_actions.rs",
            describe_available_actions::steps(),
        ),
        ("tests/steps/describe_fs.rs", describe_fs::steps()),
        (
            "tests/steps/enforcement_bundle_check.rs",
            enforcement_bundle_check::steps(),
        ),
        (
            "tests/steps/evidence_obligation_predicate.rs",
            evidence_obligation_predicate::steps(),
        ),
        (
            "tests/steps/evidence_assessment.rs",
            evidence_assessment::steps(),
        ),
        (
            "tests/steps/merge_check_classification.rs",
            merge_check_classification::steps(),
        ),
        (
            "tests/steps/evidence_obligation_gate.rs",
            evidence_obligation_gate::steps(),
        ),
        (
            "tests/steps/evidence_obligation_schema.rs",
            evidence_obligation_schema::steps(),
        ),
        (
            "tests/steps/seed_source_parity.rs",
            seed_source_parity::steps(),
        ),
        ("tests/steps/session.rs", session::steps()),
        (
            "tests/steps/status_header.rs",
            status_header::steps(),
        ),
        (
            "tests/steps/playbook_activity.rs",
            playbook_activity::steps(),
        ),
        ("tests/steps/usage_timeseries.rs", usage_timeseries::steps()),
        ("tests/steps/activity_summary.rs", activity_summary::steps()),
        ("tests/steps/telemetry_salt.rs", telemetry_salt::steps()),
        (
            "tests/steps/step0_stream_adapter.rs",
            step0_stream_adapter::steps(),
        ),
        (
            "tests/steps/step_exemplar_schema.rs",
            step_exemplar_schema::steps(),
        ),
        ("tests/steps/step_two_by_two.rs", step_two_by_two::steps()),
        (
            "tests/steps/track_lifecycle_evidence_obligation.rs",
            track_lifecycle_evidence_obligation::steps(),
        ),
        ("tests/steps/survivor_outcome.rs", survivor_outcome::steps()),
        (
            "tests/steps/outcome_predicate_fold.rs",
            outcome_predicate_fold::steps(),
        ),
        ("tests/steps/scorecard_agg.rs", scorecard_agg::steps()),
        (
            "tests/steps/live_instances_fold.rs",
            live_instances_fold::steps(),
        ),
        (
            "tests/steps/agent_hook_paths_resolve.rs",
            agent_hook_paths_resolve::steps(),
        ),
        (
            "tests/steps/kit_playbook_bijection.rs",
            kit_playbook_bijection::steps(),
        ),
        (
            "tests/steps/residual_token_allowlist.rs",
            residual_token_allowlist::steps(),
        ),
        (
            "tests/steps/internal_identifier_boundary.rs",
            internal_identifier_boundary::steps(),
        ),
        (
            "tests/steps/semantic_identifier_families.rs",
            semantic_identifier_families::steps(),
        ),
        (
            "tests/steps/hearth_port_reachability.rs",
            hearth_port_reachability::steps(),
        ),
        (
            "tests/steps/hearth_port_syscall_coverage.rs",
            hearth_port_syscall_coverage::steps(),
        ),
        (
            "tests/steps/hearth_write_refusal.rs",
            hearth_write_refusal::steps(),
        ),
    ];
    harness::run_main_with_default_features(
        domain_steps,
        harness::default_features_from_workspace_patterns(&["anvil-core/features/**/*.feature"]),
    );
}
