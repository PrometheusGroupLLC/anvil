use anvil_test_support::{
    checkin,
    engine,
    harness,
    hearth,
    snapshot,
};
use anvil_mcp_steps::{
    backlog_item_mcp,
    kit_build_script,
    kit_manifest,
    kit_playbook_source,
    mcp,
    mcp_claimed_evidence,
    orchestrate,
};

// In-crate step module (§0.3 row 3: anvil-mcp features are served by in-crate
// steps). Registered below — an unregistered step module fails the feature on
// undefined steps, it does not silently skip.
#[path = "steps/canonical_playbook_language.rs"]
mod canonical_playbook_language;

fn main() {
    harness::ensure_binary("anvil-engine");
    harness::ensure_binary("anvil-mcp");
    let domain_steps = vec![
        (
            "tests/steps/backlog_item_mcp.rs",
            backlog_item_mcp::steps(),
        ),
        ("tests/steps/mcp.rs", mcp::steps()),
        (
            "tests/steps/mcp_claimed_evidence.rs",
            mcp_claimed_evidence::steps(),
        ),
        ("tests/steps/orchestrate.rs", orchestrate::steps()),
        ("tests/steps/hearth.rs", hearth::steps()),
        ("tests/steps/engine.rs", engine::steps()),
        ("tests/steps/checkin.rs", checkin::steps()),
        ("tests/steps/snapshot.rs", snapshot::steps()),
        ("tests/steps/kit_manifest.rs", kit_manifest::steps()),
        (
            "tests/steps/kit_playbook_source.rs",
            kit_playbook_source::steps(),
        ),
        ("tests/steps/kit_build_script.rs", kit_build_script::steps()),
        (
            "tests/steps/canonical_playbook_language.rs",
            canonical_playbook_language::steps(),
        ),
    ];
    harness::run_main_with_default_features(
        domain_steps,
        harness::default_features_from_workspace_patterns(&["anvil-mcp/features/**/*.feature"]),
    );
}
