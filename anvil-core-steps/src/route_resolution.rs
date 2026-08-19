//! Step module for `route_resolution.feature` (playbook_router BP2).
//!
//! Exercises the pure core `resolve_route` function through real registry
//! adapters: `HearthPlaybookRegistry` loads the recap fixture machine.yaml
//! files from a temporary hearth and `SeedPlaybookRegistry` supplies the
//! Foundation driven playbooks. No engine process is spawned.

use anvil_core::domain::amendment::OpLog;
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{
    resolve_route, PlaybookRegistry, RouteOutcome, RouteResolution, SeedPlaybookRegistry,
};
use anvil_core::domain::playbook::types::{Role, Sensitivity};
use anvil_core::domain::route::{
    find_open_playbook_run_for_conversation, find_open_playbook_run_indexed, OpenPlaybookRun,
    CANDIDATE_PLAYBOOK_INTAKE,
};
use anvil_core::domain::shared_types::{ActivityEntry, RegistryEntry, RequestContext};
use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core::ports::query_port::{OriginTurnArtifact, QueryError, QueryPort};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const HEARTH_PATH_KEY: &str = "rr_hearth_path";
const HEARTH_HANDLE_KEY: &str = "rr_hearth_handle";
const REQUEST_CONTEXT_KEY: &str = "rh_request_context";
const RESOLUTION_KEY: &str = "rr_resolution";
const INPUT_KEY: &str = "rr_input";
const PENDING_PLAYBOOK_KEY: &str = "rr_pending_playbook";
const OPEN_LOOKUP_KEY: &str = "rr_open_lookup";
const INDEXED_LOOKUP_KEY: &str = "rr_indexed_lookup";
const ENUMERATED_KEY: &str = "rr_enumerated_all";

/// A `QueryPort` decorator over `FileSystemQueryAdapter` that COUNTS calls to
/// `list_artifacts`. The indexed open-playbook lookup must read only the
/// candidate artifacts and NEVER enumerate every artifact, so the index-lookup
/// step wraps the real fs adapter in this and asserts the counter stayed at 0.
struct ListCountingQueryAdapter {
    inner: FileSystemQueryAdapter,
    list_calls: Arc<AtomicUsize>,
}

impl ListCountingQueryAdapter {
    fn new(inner: FileSystemQueryAdapter) -> (Self, Arc<AtomicUsize>) {
        let list_calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner,
                list_calls: Arc::clone(&list_calls),
            },
            list_calls,
        )
    }
}

impl QueryPort for ListCountingQueryAdapter {
    fn list_artifacts(&self) -> Result<Vec<(String, String)>, QueryError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.list_artifacts()
    }
    fn read_artifact_kind(&self, artifact_id: &str) -> Result<String, QueryError> {
        self.inner.read_artifact_kind(artifact_id)
    }
    fn read_artifact_state(&self, artifact_id: &str) -> Result<String, QueryError> {
        self.inner.read_artifact_state(artifact_id)
    }
    fn read_artifact_status(&self, artifact_id: &str) -> Result<FullStatusYaml, QueryError> {
        self.inner.read_artifact_status(artifact_id)
    }
    fn find_artifact_by_kind_origin_turn(
        &self,
        kind: &str,
        origin_turn: &str,
    ) -> Result<Option<OriginTurnArtifact>, QueryError> {
        self.inner
            .find_artifact_by_kind_origin_turn(kind, origin_turn)
    }
    fn read_activity_entries(&self, artifact_id: &str) -> Result<Vec<ActivityEntry>, QueryError> {
        self.inner.read_activity_entries(artifact_id)
    }
    fn read_transitions(&self, artifact_id: &str) -> Result<Vec<StatusTransition>, QueryError> {
        self.inner.read_transitions(artifact_id)
    }
    fn read_artifact_text(&self, track_path: &str, filename: &str) -> Result<String, QueryError> {
        self.inner.read_artifact_text(track_path, filename)
    }
    fn read_op_log(&self, artifact_path: &str, target_document: &str) -> Result<OpLog, QueryError> {
        self.inner.read_op_log(artifact_path, target_document)
    }
    fn read_context_file(&self, relative_path: &str) -> Result<String, QueryError> {
        self.inner.read_context_file(relative_path)
    }
    fn read_playbook_hook_body(
        &self,
        playbook_id: &str,
        filename: &str,
    ) -> Result<String, QueryError> {
        self.inner.read_playbook_hook_body(playbook_id, filename)
    }
    fn read_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<RegistryEntry, QueryError> {
        self.inner.read_registry_entry(registry_file, artifact_id)
    }
    fn check_projection_row_unique(
        &self,
        projection_file: &str,
        track_name: &str,
        from_section: &str,
    ) -> Result<(), QueryError> {
        self.inner
            .check_projection_row_unique(projection_file, track_name, from_section)
    }
}

/// Seed an artifact directory under the routable-fixtures hearth with a
/// status.yaml carrying the given current `state` and a begin-marker activity
/// entry tagging `conversation_id` at `begun_at`. The fixture machines declare
/// `active` (non-terminal) and `completed` (terminal) states, so terminal-state
/// open-check behavior is exercisable by seeding state `completed`.
/// resume_aware_routing Phase 2.
fn seed_open_artifact(
    hearth: &Path,
    kind: &str,
    artifact_id: &str,
    state: &str,
    conversation_id: &str,
    begun_at: &str,
) -> Result<(), String> {
    // The fixture machines use `directory: playbooks`; per-instance artifacts
    // live in their own type directory. Use a generic `instances/` dir so the
    // registry-free enumeration scan (list_artifacts) discovers them.
    let dir = hearth.join("instances").join(artifact_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
    let status = format!(
        concat!(
            "version: 1\n",
            "kind: {kind}\n",
            "state: {state}\n",
            "activity:\n",
            "  - kind: begin\n",
            "    actor: Tester-000000\n",
            "    state: active\n",
            "    at: {begun_at}\n",
            "    conversation_id: {conversation_id}\n",
        ),
        kind = kind,
        state = state,
        begun_at = begun_at,
        conversation_id = conversation_id,
    );
    std::fs::write(dir.join("status.yaml"), status)
        .map_err(|e| format!("Failed to write status.yaml: {}", e))
}

fn fixture_playbook_dir(kind: &str) -> PathBuf {
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .join("fixtures")
        .join(kind)
}

fn copy_fixture_playbook(kind: &str, hearth: &Path) -> Result<(), String> {
    let source = fixture_playbook_dir(kind);
    let target = hearth.join("playbooks").join(kind);
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;

    let source_machine = source.join("machine.yaml");
    std::fs::copy(&source_machine, target.join("machine.yaml")).map_err(|e| {
        format!(
            "Failed to copy {} into temp hearth: {}",
            source_machine.display(),
            e
        )
    })?;

    let source_hooks = source.join("hooks");
    if source_hooks.exists() {
        let target_hooks = target.join("hooks");
        std::fs::create_dir_all(&target_hooks)
            .map_err(|e| format!("Failed to create {}: {}", target_hooks.display(), e))?;
        for entry in std::fs::read_dir(&source_hooks)
            .map_err(|e| format!("Failed to read {}: {}", source_hooks.display(), e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read hook entry: {}", e))?;
            if entry
                .file_type()
                .map_err(|e| format!("Failed to read hook file type: {}", e))?
                .is_file()
            {
                std::fs::copy(entry.path(), target_hooks.join(entry.file_name()))
                    .map_err(|e| format!("Failed to copy hook file: {}", e))?;
            }
        }
    }

    Ok(())
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization should not fail")
}

fn write_route_fixture_machine(
    hearth: &Path,
    artifact_id: &str,
    kind: &str,
    description: &str,
    route_description: Option<&str>,
    triggers: &[&str],
) -> Result<(), String> {
    let target = hearth.join("playbooks").join(artifact_id);
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;

    let route_block = if triggers.is_empty() && route_description.is_none() {
        String::new()
    } else {
        let description_line = route_description
            .map(|description| format!("  description: {}\n", yaml_string(description)))
            .unwrap_or_default();
        let trigger_lines = triggers
            .iter()
            .map(|trigger| format!("    - {}\n", yaml_string(trigger)))
            .collect::<String>();
        format!("route:\n{}  triggers:\n{}", description_line, trigger_lines)
    };

    let yaml = format!(
        concat!(
            "kind: {kind}\n",
            "directory: playbooks\n",
            "registry: workflows.md\n",
            "description: {description}\n",
            "{route_block}",
            "roles:\n",
            "  - doer\n",
            "  - reviewer\n",
            "states:\n",
            "  - name: active\n",
            "    role_filters: []\n",
            "    registry_section: active\n",
            "    projection_targets: []\n",
            "    is_review_gate: false\n",
            "    is_terminal: false\n",
            "  - name: completed\n",
            "    role_filters: []\n",
            "    registry_section: completed\n",
            "    projection_targets: []\n",
            "    is_review_gate: false\n",
            "    is_terminal: true\n",
            "transitions:\n",
            "  - from_state: active\n",
            "    to_state: completed\n",
            "    required_role: doer\n",
            "    required_satisfaction: ~\n",
            "    requires_approver: false\n",
        ),
        kind = kind,
        description = yaml_string(description),
        route_block = route_block,
    );

    std::fs::write(target.join("machine.yaml"), yaml)
        .map_err(|e| format!("Failed to write {} machine.yaml: {}", artifact_id, e))
}

fn write_routable_playbook_fixtures(hearth: &Path) -> Result<(), String> {
    let fixtures = [
        (
            "20260422T0000_track_lifecycle",
            "track",
            "Concrete implementation of a proposal slice.",
            Some("Route here when the user asks to IMPLEMENT, build, add, wire, fix, refactor, or ship a feature, code change, bug fix, or component — concrete engineering work to do now. NOT for authoring a new playbook definition (use playbook_generation), and NOT for questions, debugging-only, status, or discussion."),
            &[
                "new track",
                "start a track",
                "create a track",
                "begin work on a track",
                "implement a feature",
                "start a feature",
            ][..],
        ),
        (
            "20260528T2321_workflow_generation",
            "playbook_generation",
            "Author a new playbook machine from discovery through validation.",
            Some("Route here when the user wants to AUTHOR or GENERATE a brand-new PLAYBOOK DEFINITION itself (a new machine.yaml / lifecycle/state-machine for some activity). NOT for implementing a feature (use track)."),
            &[
                "create a playbook",
                // Legacy READ trigger, mirroring the production seed: it matches
                // what a PERSON types, not what Anvil calls the thing. A fixture
                // that dropped it would model a router production does not have.
                "create a workflow",
                "author a playbook",
                "new playbook",
                "build a playbook",
                "generate a playbook",
            ][..],
        ),
        (
            "20260528T2321_measurement_cycle",
            "measurement_cycle",
            "Run a Karpathy-loop measurement cycle from discovery through monitoring.",
            None,
            &[
                "measurement cycle",
                "run a measurement cycle",
                "measure and optimize",
                "karpathy loop",
            ][..],
        ),
        (
            "20260529T0004_tax_document_collection",
            "tax_document_collection",
            "Prepare a tax year by collecting, extracting, reconciling, and reviewing tax documents.",
            None,
            &[
                "prepare taxes",
                "tax prep",
                "do my taxes",
                "collect tax documents",
                "start tax year",
            ][..],
        ),
        (
            "20260529T0409_knowledge_lifecycle",
            "knowledge_lifecycle",
            "Govern domain knowledge from ingestion through publication.",
            None,
            &[
                "ingest knowledge",
                "knowledge lifecycle",
                "organize knowledge",
                "publish knowledge",
            ][..],
        ),
        (
            "20260529T0409_compile_topic",
            "compile_topic",
            "Synthesize a Lore topic's linked evidence into compiled knowledge.",
            None,
            &["compile a topic", "synthesize topic", "compile knowledge"][..],
        ),
        (
            "20260609T1322_intelligence_scan",
            "intelligence_scan",
            "Scan across topics for conflicts, connections, and synthesis insights.",
            None,
            &[
                "intelligence scan",
                "cross-topic scan",
                "find conflicts across topics",
                "scan for connections",
            ][..],
        ),
        (
            "20260609T1322_lore_categorize",
            "lore_categorize",
            "Categorize and auto-organize Lore captures against existing topics.",
            None,
            &[
                "categorize captures",
                "organize my captures",
                "auto-organize bookmarks",
                "match captures to topics",
            ][..],
        ),
        (
            "20260609T1322_lore_digest",
            "lore_digest",
            "Generate a Lore daily brief, weekly review, or knowledge digest.",
            None,
            &["daily brief", "weekly review", "knowledge digest", "lore digest"][..],
        ),
        (
            "20260609T1322_lore_gap_analysis",
            "lore_gap_analysis",
            "Analyze a Lore topic's evidence inventory for gaps and readiness.",
            None,
            &[
                "gap analysis",
                "analyze topic gaps",
                "topic readiness",
                "what's missing in a topic",
            ][..],
        ),
        (
            "20260609T1322_lore_query",
            "lore_query",
            "Answer natural-language questions from Lore topics and linked evidence.",
            None,
            &[
                "ask lore",
                "query my knowledge",
                "answer from my topics",
                "search my notes",
            ][..],
        ),
        (
            "20260609T1322_lore_source_research",
            "lore_source_research",
            "Discover and verify external sources referenced by a Lore topic.",
            None,
            &["research sources", "verify sources", "find sources for a topic"][..],
        ),
        (
            "20260609T1322_lore_vision",
            "lore_vision",
            "Analyze images with OCR, object, scene, mood, and searchable tag extraction.",
            None,
            &["tag images", "analyze images", "vision tag media", "ocr images"][..],
        ),
        (
            "20260609T1322_entity_research",
            "entity_research",
            "Research and enrich people or organization entities in the knowledge graph.",
            None,
            &["entity research", "research this person", "enrich entity"][..],
        ),
        (
            "20260601T1254_extract_document",
            "extract_document",
            "Queue-triggered extraction of typed fields from uploaded tax documents.",
            None,
            &[][..],
        ),
        (
            "20260601T1254_import_transaction_history",
            "import_transaction_history",
            "Queue-triggered ingestion and correlation of historical transaction CSV imports.",
            None,
            &[][..],
        ),
    ];

    for (artifact_id, kind, description, route_description, triggers) in fixtures {
        write_route_fixture_machine(
            hearth,
            artifact_id,
            kind,
            description,
            route_description,
            triggers,
        )?;
    }

    Ok(())
}

fn route_registry(
    hearth: PathBuf,
) -> CompositePlaybookRegistry<HearthPlaybookRegistry, SeedPlaybookRegistry> {
    CompositePlaybookRegistry::new(HearthPlaybookRegistry::new(hearth), SeedPlaybookRegistry)
}

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_role(value: &str) -> Result<Role, String> {
    match value {
        "read" => Ok(Role::Read),
        "write" => Ok(Role::Write),
        "admin" => Ok(Role::Admin),
        other => Err(format!("Unknown role '{}'", other)),
    }
}

fn parse_sensitivity(value: &str) -> Result<Sensitivity, String> {
    match value {
        "public" => Ok(Sensitivity::Public),
        "internal" => Ok(Sensitivity::Internal),
        "confidential" => Ok(Sensitivity::Confidential),
        "phi" => Ok(Sensitivity::Phi),
        other => Err(format!("Unknown sensitivity '{}'", other)),
    }
}

fn parse_space(value: &str) -> Option<String> {
    match value {
        "" | "~" | "none" => None,
        other => Some(other.to_string()),
    }
}

fn resolution(ctx: &Context) -> Result<&RouteResolution, String> {
    ctx.get::<RouteResolution>(RESOLUTION_KEY)
        .ok_or_else(|| "No route resolution".to_string())
}

fn expected_outcome(value: &str) -> Result<RouteOutcome, String> {
    match value {
        "Single" => Ok(RouteOutcome::Single),
        "Candidates" => Ok(RouteOutcome::Candidates),
        "NoMatch" => Ok(RouteOutcome::NoMatch),
        other => Err(format!("Unknown route outcome '{}'", other)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an empty full-signal route registry",
            &[],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir =
                    tempfile::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth = temp_dir.path().to_path_buf();
                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a full-signal request context",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            |ctx, _params| {
                let mut out = Context::new();
                out.set(
                    HEARTH_PATH_KEY,
                    ctx.get::<PathBuf>(HEARTH_PATH_KEY)
                        .ok_or("No full-signal hearth path")?
                        .clone(),
                );
                out.set(
                    HEARTH_HANDLE_KEY,
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                        .ok_or("No full-signal temp dir handle")?
                        .clone(),
                );
                out.set(
                    REQUEST_CONTEXT_KEY,
                    RequestContext {
                        org: "Foundation".to_string(),
                        role: Role::Read,
                        clearance: Sensitivity::Internal,
                        space: None,
                    },
                );
                Ok(out)
            },
        ),
        step_def(
            "a full-signal workflow {string} described as {string} with triggers {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected workflow kind")?;
                let description = params.get_string(1).ok_or("Expected description")?;
                let triggers_raw = params.get_string(2).ok_or("Expected triggers")?;
                let triggers = parse_csv(&triggers_raw);
                let trigger_refs: Vec<&str> = triggers.iter().map(String::as_str).collect();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No full-signal hearth path")?
                    .clone();
                write_route_fixture_machine(
                    &hearth,
                    &format!("fixture_{}", kind),
                    &kind,
                    &description,
                    None,
                    &trigger_refs,
                )?;

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(
                    HEARTH_HANDLE_KEY,
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                        .ok_or("No full-signal temp dir handle")?
                        .clone(),
                );
                out.set(
                    REQUEST_CONTEXT_KEY,
                    ctx.get::<RequestContext>(REQUEST_CONTEXT_KEY)
                        .ok_or("No full-signal request context")?
                        .clone(),
                );
                Ok(out)
            },
        ),
        step_def(
            "a full-signal workflow {string} described as {string} with no triggers",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected workflow kind")?;
                let description = params.get_string(1).ok_or("Expected description")?;
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No full-signal hearth path")?
                    .clone();
                write_route_fixture_machine(
                    &hearth,
                    &format!("fixture_{}", kind),
                    &kind,
                    &description,
                    None,
                    &[],
                )?;

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(
                    HEARTH_HANDLE_KEY,
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                        .ok_or("No full-signal temp dir handle")?
                        .clone(),
                );
                out.set(
                    REQUEST_CONTEXT_KEY,
                    ctx.get::<RequestContext>(REQUEST_CONTEXT_KEY)
                        .ok_or("No full-signal request context")?
                        .clone(),
                );
                Ok(out)
            },
        ),
        step_def(
            "the full-signal route resolver runs with input {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
                (RESOLUTION_KEY, "RouteResolution"),
            ],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No full-signal hearth path")?
                    .clone();
                let request_context = ctx
                    .get::<RequestContext>(REQUEST_CONTEXT_KEY)
                    .ok_or("No full-signal request context")?
                    .clone();
                let registry = HearthPlaybookRegistry::new(hearth.clone());
                if !registry.invalid_artifacts().is_empty() {
                    return Err(format!(
                        "Full-signal workflow fixture load errors: {:?}",
                        registry.invalid_artifacts()
                    ));
                }
                let result = resolve_route(
                    &registry,
                    &request_context,
                    &params.get_string(0).ok_or("Expected input")?,
                );

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(
                    HEARTH_HANDLE_KEY,
                    ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                        .ok_or("No full-signal temp dir handle")?
                        .clone(),
                );
                out.set(REQUEST_CONTEXT_KEY, request_context);
                out.set(RESOLUTION_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the full granted signals are {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let actual = resolution(&ctx)?
                    .full_granted_signals
                    .iter()
                    .map(|signal| {
                        format!(
                            "{}:{}:{}",
                            signal.kind, signal.trigger_tier, signal.content_overlap
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let expected = params.get_string(0).ok_or("Expected full signals")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected full signals {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the full granted signal count is {int}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let actual = resolution(&ctx)?.full_granted_signals.len() as i64;
                let expected = params.get_int(0).ok_or("Expected signal count")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected {} full signals, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "the full granted signals include {string} with trigger tier {int} and content overlap {int}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let trigger_tier = params.get_int(1).ok_or("Expected trigger tier")? as usize;
                let content_overlap =
                    params.get_int(2).ok_or("Expected content overlap")? as usize;
                if resolution(&ctx)?.full_granted_signals.iter().any(|signal| {
                    signal.kind == kind
                        && signal.trigger_tier == trigger_tier
                        && signal.content_overlap == content_overlap
                }) {
                    Ok(())
                } else {
                    Err(format!(
                        "No full signal for {} with tier {} and overlap {}",
                        kind, trigger_tier, content_overlap
                    ))
                }
            },
        ),
        check_def(
            "the full granted signal kinds are ordered {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let actual = resolution(&ctx)?
                    .full_granted_signals
                    .iter()
                    .map(|signal| signal.kind.clone())
                    .collect::<Vec<_>>();
                let expected = parse_csv(&params.get_string(0).ok_or("Expected ordered kinds")?);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected full signal order {:?}, got {:?}", expected, actual))
                }
            },
        ),
        step_def(
            "a core route registry with the daily_recap and weekly_recap fixtures plus seed playbooks",
            &[],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir =
                    tempfile::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth = temp_dir.path().to_path_buf();
                copy_fixture_playbook("daily_recap", &hearth)?;
                copy_fixture_playbook("weekly_recap", &hearth)?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a routable playbooks fixture registry with authored route triggers",
            &[],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            |_ctx, _params| {
                let temp_dir =
                    tempfile::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
                let hearth = temp_dir.path().to_path_buf();
                write_routable_playbook_fixtures(&hearth)?;
                let registry = HearthPlaybookRegistry::new(hearth.clone());
                if !registry.invalid_artifacts().is_empty() {
                    return Err(format!(
                        "Routable playbook fixture load errors: {:?}",
                        registry.invalid_artifacts()
                    ));
                }

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp_dir)));
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a route resolver request context with org {string} role {string} clearance {string} space {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                let org = params.get_string(0).ok_or("Expected org")?.to_string();
                let role = params.get_string(1).ok_or("Expected role")?.to_string();
                let clearance = params
                    .get_string(2)
                    .ok_or("Expected clearance")?
                    .to_string();
                let space = params.get_string(3).ok_or("Expected space")?.to_string();

                let request_ctx = RequestContext {
                    org,
                    role: parse_role(&role)?,
                    clearance: parse_sensitivity(&clearance)?,
                    space: parse_space(&space),
                };

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                out.set(REQUEST_CONTEXT_KEY, request_ctx);
                Ok(out)
            },
        ),
        step_def(
            "the pure route resolver runs with input {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
                (INPUT_KEY, "String"),
                (RESOLUTION_KEY, "RouteResolution"),
            ],
            |ctx, params| {
                let input = params.get_string(0).ok_or("Expected input")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                let request_ctx = ctx
                    .get::<RequestContext>(REQUEST_CONTEXT_KEY)
                    .ok_or("No request context")?
                    .clone();

                let registry = route_registry(hearth.clone());
                let result = resolve_route(&registry, &request_ctx, &input);

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                out.set(REQUEST_CONTEXT_KEY, request_ctx);
                out.set(INPUT_KEY, input);
                out.set(RESOLUTION_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            // Eval-harness-over-real-traces contract (router_precision Phase 3):
            // the router is a PURE, deterministic function of (registry, ctx,
            // input). Re-running it {int} times over the same input against the
            // same registry MUST yield a byte-identical resolution. This is the
            // re-runnability property the whole measurement loop depends on —
            // same corpus + same engine => same scorecard, so a score delta is a
            // real routing change, never resolver nondeterminism. No mocks: the
            // real resolver over the real fixture registry.
            "re-running the resolver {int} times over the same input yields an identical resolution",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (REQUEST_CONTEXT_KEY, "RequestContext"),
                (INPUT_KEY, "String"),
                (RESOLUTION_KEY, "RouteResolution"),
            ],
            |ctx, params| {
                let reruns = params.get_int(0).ok_or("Expected rerun count")? as usize;
                let first = resolution(&ctx)?.clone();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let request_ctx = ctx
                    .get::<RequestContext>(REQUEST_CONTEXT_KEY)
                    .ok_or("No request context")?
                    .clone();
                let input = ctx
                    .get::<String>(INPUT_KEY)
                    .ok_or("No resolver input recorded")?
                    .clone();
                // Rebuild the registry from scratch each pass so we also prove the
                // resolution is independent of registry-construction order.
                for pass in 0..reruns {
                    let registry = route_registry(hearth.clone());
                    let again = resolve_route(&registry, &request_ctx, &input);
                    if again != first {
                        return Err(format!(
                            "Resolver nondeterministic on pass {}: first {:?} != rerun {:?}",
                            pass, first, again
                        ));
                    }
                }
                Ok(())
            },
        ),
        step_def(
            "the playbook {string} is a pending_queue kind",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (PENDING_PLAYBOOK_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?.to_string();
                if !matches!(kind.as_str(), "extract_document" | "import_transaction_history") {
                    return Err(format!("Playbook {} is not a pending_queue fixture", kind));
                }

                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                let registry = route_registry(hearth.clone());
                if registry.machine_for(&kind).is_none() {
                    return Err(format!("No playbook machine for {}", kind));
                }

                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                out.set(PENDING_PLAYBOOK_KEY, kind);
                Ok(out)
            },
        ),
        check_def(
            "the route resolution outcome is {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = expected_outcome(params.get_string(0).ok_or("Expected outcome")?.as_ref())?;
                let actual = &resolution(&ctx)?.outcome;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected outcome {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the selected route kind is {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?.to_string();
                let actual = &resolution(&ctx)?.selected_kind;
                match actual {
                    Some(kind) if kind == &expected => Ok(()),
                    _ => Err(format!("Expected selected kind {:?}, got {:?}", expected, actual)),
                }
            },
        ),
        check_def(
            "the selected route source playbook id is {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (RESOLUTION_KEY, "RouteResolution"),
            ],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook id")?.to_string();
                let selected_kind = resolution(&ctx)?
                    .selected_kind
                    .as_ref()
                    .ok_or("No selected route kind")?;
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let registry = route_registry(hearth);
                let actual = registry
                    .source_for(selected_kind)
                    .map(|source| source.playbook_id)
                    .ok_or_else(|| format!("No source for selected kind {}", selected_kind))?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected source playbook id {}, got {}", expected, actual))
                }
            },
        ),
        check_def(
            "no route kind is selected",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, _params| {
                let actual = &resolution(&ctx)?.selected_kind;
                if actual.is_none() {
                    Ok(())
                } else {
                    Err(format!("Expected no selected kind, got {:?}", actual))
                }
            },
        ),
        check_def(
            "the matching route candidates include {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?.to_string();
                let actual = &resolution(&ctx)?.matching_candidates;
                if actual.contains(&expected) {
                    Ok(())
                } else {
                    Err(format!("Expected matching candidates {:?} to include {}", actual, expected))
                }
            },
        ),
        check_def(
            "the top-ranked matching route candidate is {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?.to_string();
                let actual = &resolution(&ctx)?.matching_candidates;
                match actual.first() {
                    Some(kind) if kind == &expected => Ok(()),
                    _ => Err(format!(
                        "Expected top-ranked matching candidate {}, got {:?}",
                        expected, actual
                    )),
                }
            },
        ),
        check_def(
            "the matching route candidate count is at most {int}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let limit = params.get_int(0).ok_or("Expected candidate count limit")? as usize;
                let actual = &resolution(&ctx)?.matching_candidates;
                if actual.len() <= limit {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected at most {} matching candidates, got {}: {:?}",
                        limit,
                        actual.len(),
                        actual
                    ))
                }
            },
        ),
        check_def(
            "the matching route candidate descriptions are returned for selection",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (RESOLUTION_KEY, "RouteResolution"),
            ],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let registry = route_registry(hearth);
                for kind in &resolution(&ctx)?.matching_candidates {
                    let description = registry
                        .machine_for(kind)
                        .map(|machine| machine.description.trim())
                        .ok_or_else(|| format!("No machine for matching candidate {}", kind))?;
                    if description.is_empty() {
                        return Err(format!("Candidate {} has an empty description", kind));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the no-match handoff is {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected handoff")?;
                match resolution(&ctx)?.outcome {
                    RouteOutcome::NoMatch
                        if expected.as_ref() as &str == CANDIDATE_PLAYBOOK_INTAKE =>
                    {
                        Ok(())
                    }
                    RouteOutcome::NoMatch => Err(format!(
                        "Expected handoff {}, got {}",
                        expected, CANDIDATE_PLAYBOOK_INTAKE
                    )),
                    ref other => Err(format!("Expected NoMatch handoff, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the granted route candidates are exactly {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = parse_csv(params.get_string(0).ok_or("Expected candidates")?.as_ref());
                let actual = &resolution(&ctx)?.granted_candidates;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected granted candidates {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "it declares no conversational route triggers",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (PENDING_PLAYBOOK_KEY, "String"),
            ],
            |ctx, _params| {
                let kind = ctx
                    .get::<String>(PENDING_PLAYBOOK_KEY)
                    .ok_or("No pending playbook kind")?
                    .clone();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let registry = route_registry(hearth);
                let machine = registry
                    .machine_for(&kind)
                    .ok_or_else(|| format!("No playbook machine for {}", kind))?;
                if machine.route.triggers.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} to have no triggers, got {:?}",
                        kind, machine.route.triggers
                    ))
                }
            },
        ),
        check_def(
            "it still declares a routing-grade description",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (PENDING_PLAYBOOK_KEY, "String"),
            ],
            |ctx, _params| {
                let kind = ctx
                    .get::<String>(PENDING_PLAYBOOK_KEY)
                    .ok_or("No pending playbook kind")?
                    .clone();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let registry = route_registry(hearth);
                let machine = registry
                    .machine_for(&kind)
                    .ok_or_else(|| format!("No playbook machine for {}", kind))?;
                if machine.description.trim().is_empty() {
                    Err(format!("Expected {} to have a non-empty description", kind))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the matching route candidates are exactly {string}",
            &[(RESOLUTION_KEY, "RouteResolution")],
            |ctx, params| {
                let expected = parse_csv(params.get_string(0).ok_or("Expected candidates")?.as_ref());
                let actual = &resolution(&ctx)?.matching_candidates;
                if actual == &expected {
                    Ok(())
                } else {
                    Err(format!("Expected matching candidates {:?}, got {:?}", expected, actual))
                }
            },
        ),
        // ============ resume_aware_routing Phase 2: open-playbook lookup ============
        step_def(
            "an open playbook {string} of kind {string} in state {string} begun for conversation {string} at {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let conversation_id = params.get_string(3).ok_or("Expected conversation")?.to_string();
                let begun_at = params.get_string(4).ok_or("Expected at")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                seed_open_artifact(&hearth, &kind, &artifact_id, &state, &conversation_id, &begun_at)?;
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the open-playbook lookup runs for conversation {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (OPEN_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
            ],
            |ctx, params| {
                let conversation_id = params.get_string(0).ok_or("Expected conversation")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                let registry = route_registry(hearth.clone());
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let found =
                    find_open_playbook_run_for_conversation(&query, &registry, &conversation_id)
                        .map_err(|e| format!("lookup failed: {}", e))?;
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                out.set(OPEN_LOOKUP_KEY, found);
                Ok(out)
            },
        ),
        check_def(
            "the open-playbook lookup resolves artifact {string} kind {string} state {string}",
            &[(OPEN_LOOKUP_KEY, "Option<OpenPlaybookRun>")],
            |ctx, params| {
                let want_id = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let want_kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let want_state = params.get_string(2).ok_or("Expected state")?.to_string();
                let found = ctx
                    .get::<Option<OpenPlaybookRun>>(OPEN_LOOKUP_KEY)
                    .ok_or("No open-playbook lookup result")?;
                match found {
                    Some(w)
                        if w.artifact_id == want_id
                            && w.kind == want_kind
                            && w.state == want_state =>
                    {
                        Ok(())
                    }
                    other => Err(format!(
                        "Expected open playbook ({}, {}, {}), got {:?}",
                        want_id, want_kind, want_state, other
                    )),
                }
            },
        ),
        check_def(
            "the open-playbook lookup resolves nothing",
            &[(OPEN_LOOKUP_KEY, "Option<OpenPlaybookRun>")],
            |ctx, _params| {
                let found = ctx
                    .get::<Option<OpenPlaybookRun>>(OPEN_LOOKUP_KEY)
                    .ok_or("No open-playbook lookup result")?;
                match found {
                    None => Ok(()),
                    Some(w) => Err(format!("Expected no open playbook, got {:?}", w)),
                }
            },
        ),
        // ============ open_marker_index Phase 1: indexed lookup ============
        // Seed many unrelated artifacts so the "did not enumerate all artifacts"
        // assertion has teeth (the indexed lookup must read ONLY its candidate,
        // not the whole hearth). The artifacts carry NO begin marker for the
        // conversation under test, so the full scan would never return them.
        step_def(
            "the hearth also has {int} unrelated artifacts in state {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            |ctx, params| {
                let count = params.get_int(0).ok_or("Expected count")? as usize;
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                for i in 0..count {
                    let id = format!("20260623T1{:03}_filler", i);
                    // Unrelated conversation "C-FILLER" so they never match the
                    // conversation under test.
                    seed_open_artifact(&hearth, "track", &id, &state, "C-FILLER", "2026-06-23T04:00:00Z")?;
                }
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the indexed open-playbook lookup runs for conversation {string} with candidates {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
                (ENUMERATED_KEY, "bool"),
            ],
            |ctx, params| {
                let conversation_id = params.get_string(0).ok_or("Expected conversation")?.to_string();
                let candidates = parse_csv(params.get_string(1).ok_or("Expected candidates")?);
                run_indexed_lookup(ctx, &conversation_id, &candidates)
            },
        ),
        step_def(
            "the indexed open-playbook lookup runs for conversation {string} with no candidates",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
                (ENUMERATED_KEY, "bool"),
            ],
            |ctx, params| {
                let conversation_id = params.get_string(0).ok_or("Expected conversation")?.to_string();
                run_indexed_lookup(ctx, &conversation_id, &[])
            },
        ),
        step_def(
            "the full-scan open-playbook lookup runs for conversation {string}",
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
            ],
            &[
                (HEARTH_PATH_KEY, "PathBuf"),
                (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<tempfile::TempDir>>>"),
                (INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
                (OPEN_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
            ],
            |ctx, params| {
                let conversation_id = params.get_string(0).ok_or("Expected conversation")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(HEARTH_PATH_KEY)
                    .ok_or("No route resolver hearth path")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
                    .ok_or("No route resolver temp dir handle")?
                    .clone();
                let indexed = ctx
                    .get::<Option<OpenPlaybookRun>>(INDEXED_LOOKUP_KEY)
                    .cloned()
                    .unwrap_or(None);
                let registry = route_registry(hearth.clone());
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let found =
                    find_open_playbook_run_for_conversation(&query, &registry, &conversation_id)
                        .map_err(|e| format!("scan lookup failed: {}", e))?;
                let mut out = Context::new();
                out.set(HEARTH_PATH_KEY, hearth);
                out.set(HEARTH_HANDLE_KEY, handle);
                out.set(INDEXED_LOOKUP_KEY, indexed);
                out.set(OPEN_LOOKUP_KEY, found);
                Ok(out)
            },
        ),
        check_def(
            "the indexed open-playbook lookup resolves artifact {string} kind {string} state {string}",
            &[(INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>")],
            |ctx, params| {
                let want_id = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let want_kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let want_state = params.get_string(2).ok_or("Expected state")?.to_string();
                let found = ctx
                    .get::<Option<OpenPlaybookRun>>(INDEXED_LOOKUP_KEY)
                    .ok_or("No indexed lookup result")?;
                match found {
                    Some(w)
                        if w.artifact_id == want_id
                            && w.kind == want_kind
                            && w.state == want_state =>
                    {
                        Ok(())
                    }
                    other => Err(format!(
                        "Expected indexed open playbook ({}, {}, {}), got {:?}",
                        want_id, want_kind, want_state, other
                    )),
                }
            },
        ),
        check_def(
            "the indexed open-playbook lookup resolves nothing",
            &[(INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>")],
            |ctx, _params| {
                let found = ctx
                    .get::<Option<OpenPlaybookRun>>(INDEXED_LOOKUP_KEY)
                    .ok_or("No indexed lookup result")?;
                match found {
                    None => Ok(()),
                    Some(w) => Err(format!("Expected no indexed open playbook, got {:?}", w)),
                }
            },
        ),
        check_def(
            "the indexed open-playbook lookup did not enumerate all artifacts",
            &[(ENUMERATED_KEY, "bool")],
            |ctx, _params| {
                let enumerated = ctx
                    .get::<bool>(ENUMERATED_KEY)
                    .ok_or("No enumeration flag")?;
                if *enumerated {
                    Err("Indexed lookup called list_artifacts (it enumerated all artifacts)".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the indexed and full-scan open-playbook lookups agree",
            &[
                (INDEXED_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
                (OPEN_LOOKUP_KEY, "Option<OpenPlaybookRun>"),
            ],
            |ctx, _params| {
                let indexed = ctx
                    .get::<Option<OpenPlaybookRun>>(INDEXED_LOOKUP_KEY)
                    .ok_or("No indexed lookup result")?;
                let scan = ctx
                    .get::<Option<OpenPlaybookRun>>(OPEN_LOOKUP_KEY)
                    .ok_or("No scan lookup result")?;
                if indexed == scan {
                    Ok(())
                } else {
                    Err(format!(
                        "Indexed lookup {:?} disagrees with scan {:?}",
                        indexed, scan
                    ))
                }
            },
        ),
    ]
}

/// Run the indexed lookup against a `ListCountingQueryAdapter` over the route
/// hearth, recording the result and whether `list_artifacts` was ever called.
fn run_indexed_lookup(
    ctx: Context,
    conversation_id: &str,
    candidates: &[String],
) -> Result<Context, String> {
    let hearth = ctx
        .get::<PathBuf>(HEARTH_PATH_KEY)
        .ok_or("No route resolver hearth path")?
        .clone();
    let handle = ctx
        .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(HEARTH_HANDLE_KEY)
        .ok_or("No route resolver temp dir handle")?
        .clone();
    let registry = route_registry(hearth.clone());
    let (query, list_calls) =
        ListCountingQueryAdapter::new(FileSystemQueryAdapter::new(hearth.clone()));
    let found = find_open_playbook_run_indexed(&query, &registry, conversation_id, candidates)
        .map_err(|e| format!("indexed lookup failed: {}", e))?;
    let enumerated = list_calls.load(Ordering::SeqCst) > 0;
    let mut out = Context::new();
    out.set(HEARTH_PATH_KEY, hearth);
    out.set(HEARTH_HANDLE_KEY, handle);
    out.set(INDEXED_LOOKUP_KEY, found);
    out.set(ENUMERATED_KEY, enumerated);
    Ok(out)
}
