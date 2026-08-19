//! Step module for the K8 engine API feature
//! (`anvil-engine/features/backlog_item_api.feature`).
//!
//! REAL SEAM (plan §3, Tasks 5/6/9). Every step here drives a REAL
//! [`anvil_test_support::engine::EngineProcess`] over REAL gRPC against a real isolated
//! temporary hearth that the engine owns, and asserts against the bytes that
//! engine actually wrote. There is no in-memory registry, no
//! `TestSnapshotAdapter`, no mocked engine, and no direct state mutation
//! standing in for an operation under test.
//!
//! Two deliberate non-gRPC seams appear, both REAL production code paths that
//! have no public RPC of their own:
//!
//! - `internal_execute` calls the real `SnapshotCommandHandler::execute`
//!   against the engine-owned hearth, because plan Task 6 requires proving the
//!   INTERNAL execute route is closed to K8.
//! - `raw_append` calls the real `SnapshotPort::append_transition` on the real
//!   `FileSystemSnapshotAdapter` for the same reason.
//!
//! The one out-of-scope producer — Temper's `registered -> reading` write — is
//! the explicitly labeled `seed_external_temper_reading` precondition sanctioned
//! by plan §3. It never performs the transition under test.

use anvil_test_support::engine::{spawn_engine_for_hearth, EngineProcess};
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use anvil_engine::proto as pb;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const WORLD: &str = "bi_rpc_world";
const WORLD_TY: &str = "BacklogEngineFixture";

/// The organ every engine-side precondition lives in — the queue and evaluate
/// scenarios name it verbatim.
const ORGAN: &str = "bn_0rgan00001";
/// The frozen count of RPCs `AnvilService` declares after the K8 append.
///
/// 27 -> 28 with T-RD-PBK-DEPTH's `RunDetail`, which is declared with the other
/// read RPCs (beside `ReadInstanceArtifact`) rather than after the K8 three —
/// so the "K8 appended LAST, in order" tail check below is untouched and still
/// means what it meant. A later track adding its own read RPC in that same place
/// moves this number again; a later track that INSERTS one into the middle of
/// the K8 block still reds the tail check, which is the property being pinned.
///
/// 28 -> 29 with T1's read-only `JoinCoverage`, declared beside the other read
/// RPCs (immediately after `PlaybookFidelity`) for the same reason `RunDetail`
/// was: the K8 three stay the last three, so the tail check below is untouched
/// and still means what it meant.
const EXPECTED_RPC_COUNT: usize = 29;
/// The engine's wire version must be unchanged by a pure RPC append.
const EXPECTED_WIRE_PROTO_VERSION: u32 = 3;

type Client = pb::anvil_service_client::AnvilServiceClient<tonic::transport::Channel>;

#[derive(Clone)]
pub struct BacklogEngineFixture {
    pub hearth: PathBuf,
    _temp: RetainedTempDir,
    engine: Arc<Mutex<Option<EngineProcess>>>,
    state: Arc<Mutex<FixtureState>>,
}

#[derive(Default)]
struct FixtureState {
    /// The item the scenario's `Given` created, if any.
    item_id: Option<String>,
    /// The DECIDE proposal a `commit`/`reject` precondition opened.
    proposal_id: Option<String>,
    last: Option<Attempt>,
    /// Every K8 byte captured immediately before the operation under test.
    before: Option<BTreeMap<String, Vec<u8>>>,
    second: Option<SecondHearth>,
    /// Ids the first hearth's evaluation aged out.
    aged_out: Vec<String>,
}

struct SecondHearth {
    path: PathBuf,
    item_id: String,
}

#[derive(Clone, Debug)]
enum Attempt {
    Ok {
        state: Option<String>,
        path: Option<String>,
    },
    Error {
        message: String,
    },
}

impl BacklogEngineFixture {
    fn new() -> Result<Self, String> {
        let (temp, hearth) = retained_temp_dir("anvil-backlog-engine")?;
        seed_hearth(&hearth)?;
        let engine = spawn_engine_for_hearth(&hearth)?;
        Ok(Self {
            hearth,
            _temp: temp,
            engine: Arc::new(Mutex::new(Some(engine))),
            state: Arc::new(Mutex::new(FixtureState::default())),
        })
    }

    fn port(&self) -> Result<u16, String> {
        self.engine
            .lock()
            .map_err(|_| "engine mutex".to_string())?
            .as_ref()
            .map(|e| e.port)
            .ok_or_else(|| "no running backlog engine".to_string())
    }

    /// Kill the engine and start a fresh one on the SAME hearth. This is the
    /// real restart the genesis-persistence scenario names: nothing is carried
    /// in memory across it.
    fn restart(&self) -> Result<(), String> {
        let mut guard = self.engine.lock().map_err(|_| "engine mutex".to_string())?;
        // Dropping the old EngineProcess kills the child and joins its drain.
        *guard = None;
        *guard = Some(spawn_engine_for_hearth(&self.hearth)?);
        Ok(())
    }

    async fn client(&self) -> Result<Client, String> {
        let port = self.port()?;
        Client::connect(format!("http://127.0.0.1:{port}"))
            .await
            .map_err(|e| format!("connect to engine on {port}: {e}"))
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut FixtureState) -> T) -> Result<T, String> {
        let mut guard = self.state.lock().map_err(|_| "state mutex".to_string())?;
        Ok(f(&mut guard))
    }

    fn record(&self, attempt: Attempt) -> Result<(), String> {
        self.with_state(|s| s.last = Some(attempt))
    }

    fn record_result(&self, outcome: Result<Attempt, String>) -> Result<(), String> {
        let attempt = match outcome {
            Ok(a) => a,
            Err(message) => Attempt::Error { message },
        };
        self.record(attempt)
    }

    fn last(&self) -> Result<Attempt, String> {
        self.with_state(|s| s.last.clone())?
            .ok_or_else(|| "no backlog engine RPC was attempted".to_string())
    }

    fn item_id(&self) -> Result<String, String> {
        self.with_state(|s| s.item_id.clone())?
            .ok_or_else(|| "this step needs a backlog item created through Begin".to_string())
    }

    /// Capture every K8 byte so a refused operation can be proven to have
    /// written nothing at all.
    fn capture(&self) -> Result<(), String> {
        let snapshot = k8_bytes(&self.hearth)?;
        self.with_state(|s| s.before = Some(snapshot))
    }
}

/// A directory the engine accepts as a hearth (Req 4 predicate).
fn seed_hearth(hearth: &Path) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("tracks")).map_err(|e| format!("create tracks: {e}"))?;
    std::fs::write(hearth.join("tracks.md"), "# Tracks\n").map_err(|e| format!("tracks.md: {e}"))
}

/// Every byte under the hearth's K8 surface, keyed by relative path.
fn k8_bytes(hearth: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let key = path
                    .strip_prefix(root)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                let bytes = std::fs::read(&path).map_err(|e| format!("read {key}: {e}"))?;
                out.insert(key, bytes);
            }
        }
        Ok(())
    }
    walk(hearth, &hearth.join("backlog_items"), &mut out)?;
    if let Ok(bytes) = std::fs::read(hearth.join("backlog_items.md")) {
        out.insert("backlog_items.md".to_string(), bytes);
    }
    Ok(out)
}

fn world(ctx: &Context) -> Result<BacklogEngineFixture, String> {
    ctx.get::<BacklogEngineFixture>(WORLD)
        .cloned()
        .ok_or_else(|| "backlog engine fixture missing; start with 'a running backlog engine'".to_string())
}

fn carry(w: BacklogEngineFixture) -> Context {
    Context::new().with(WORLD, w)
}

fn p(params: &brine_core::step_types::Params, i: usize) -> Result<String, String> {
    params
        .get_string(i)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing string parameter {i}"))
}

fn expect_ok(w: &BacklogEngineFixture) -> Result<(Option<String>, Option<String>), String> {
    match w.last()? {
        Attempt::Ok { state, path } => Ok((state, path)),
        Attempt::Error { message } => Err(format!("expected OK but engine returned error: {message}")),
    }
}

fn expect_error(w: &BacklogEngineFixture) -> Result<String, String> {
    match w.last()? {
        Attempt::Error { message } => Ok(message),
        Attempt::Ok { .. } => Err("expected a non-OK status but the RPC succeeded".into()),
    }
}

// ---------------------------------------------------------------------------
// Request builders. Identity is caller-supplied; EVERY audit stamp, history
// sequence, id and timestamp is the engine's.
// ---------------------------------------------------------------------------

fn actor_fields() -> (String, String, String, String) {
    (
        "Rpcdriver-000001".to_string(),
        "agent".to_string(),
        "claude-opus-5".to_string(),
        "anthropic".to_string(),
    )
}

fn begin_request(hearth: &Path, item_json: &str) -> pb::BeginRequest {
    let (name, ty, model, provider) = actor_fields();
    let mut create_fields = std::collections::HashMap::new();
    create_fields.insert("item".to_string(), item_json.to_string());
    pb::BeginRequest {
        hearth_path: hearth.display().to_string(),
        artifact_type: "backlog_item".to_string(),
        actor_name: name,
        actor_type: ty,
        actor_model: model,
        actor_provider: provider,
        actor_context_window: 200_000,
        actor_sdk_version: "1.0.0".to_string(),
        actor_entrypoint: "brine".to_string(),
        session_role: "creator".to_string(),
        ctx_org: "Foundation".to_string(),
        ctx_role: "read".to_string(),
        ctx_clearance: "internal".to_string(),
        create_fields,
        ..Default::default()
    }
}

fn snapshot_request(hearth: &Path, bi_id: &str, to: &str, role: &str) -> pb::SnapshotRequest {
    let (name, ty, model, provider) = actor_fields();
    pb::SnapshotRequest {
        hearth_path: hearth.display().to_string(),
        artifact_path: format!("backlog_items/{bi_id}"),
        to_state: to.to_string(),
        actor_name: name,
        actor_role: role.to_string(),
        // The #5 ATTEND gate demands an engine-verified nonempty approver; on
        // every other row the field is simply recorded.
        approver: "Nick".to_string(),
        actor_type: ty,
        actor_model: model,
        actor_provider: provider,
        actor_context_window: 200_000,
        actor_sdk_version: "1.0.0".to_string(),
        actor_entrypoint: "brine".to_string(),
        ..Default::default()
    }
}

fn mutate_request(hearth: &Path, role: &str, op: pb::backlog_mutate_request::Operation) -> pb::BacklogMutateRequest {
    let (name, ty, model, provider) = actor_fields();
    pb::BacklogMutateRequest {
        hearth_path: hearth.display().to_string(),
        actor_name: name,
        actor_role: role.to_string(),
        actor_type: ty,
        actor_model: model,
        actor_provider: provider,
        actor_context_window: 200_000,
        actor_sdk_version: "1.0.0".to_string(),
        actor_entrypoint: "brine".to_string(),
        operation: Some(op),
    }
}

fn evaluate_request(hearth: &Path, organ: &str) -> pb::BacklogEvaluateRequest {
    let (name, ty, model, provider) = actor_fields();
    pb::BacklogEvaluateRequest {
        hearth_path: hearth.display().to_string(),
        business_node_id: organ.to_string(),
        actor_name: name,
        actor_type: ty,
        actor_model: model,
        actor_provider: provider,
        actor_context_window: 200_000,
        actor_sdk_version: "1.0.0".to_string(),
        actor_entrypoint: "brine".to_string(),
    }
}

fn evidence(kind: &str, id: &str) -> Option<pb::BacklogEvidenceRef> {
    Some(pb::BacklogEvidenceRef {
        kind: kind.to_string(),
        id: id.to_string(),
    })
}

/// The one genesis payload every engine-side precondition starts from, in the
/// organ the queue/evaluate scenarios name.
fn genesis_json(title: &str) -> String {
    format!(
        r#"{{
          "business_node_id": "{ORGAN}",
          "title": "{title}",
          "action_class": "dev",
          "intake": {{
            "edge": "spark_triage",
            "evidence_refs": [{{"kind": "spark", "id": "sp_seed01"}}]
          }},
          "origin_binding": {{
            "value_gap_served": {{"kind": "temper_measure", "id": "tm_valuegap01"}},
            "minting_council_id": null,
            "experiment_id": null,
            "predicted_value": null
          }}
        }}"#
    )
}

/// The full SHAPE edit that gives a candidate effort, a playbook binding and
/// all three caller-authored rank inputs in one real mutation.
fn full_shape(bi_id: &str) -> pb::backlog_mutate_request::Operation {
    use pb::backlog_shape_edit as se;
    pb::backlog_mutate_request::Operation::ShapeEdit(pb::BacklogShapeEdit {
        backlog_item_id: bi_id.to_string(),
        action_class_edit: None,
        description_edit: None,
        effort_class_edit: Some(se::EffortClassEdit::SetEffortClass("s".to_string())),
        playbook_binding_edit: Some(se::PlaybookBindingEdit::SetPlaybookBinding(
            pb::BacklogPlaybookBinding {
                definition: Some(pb::backlog_playbook_binding::Definition::PlaybookDefinitionId(
                    "pd_seed01".to_string(),
                )),
                route_to_intake: false,
            },
        )),
        rank_inputs: Some(pb::BacklogRankInputsEdit {
            value_gap_magnitude_edit: Some(
                pb::backlog_rank_inputs_edit::ValueGapMagnitudeEdit::SetValueGapMagnitude(
                    pb::BacklogValueGapMagnitude {
                        reference: evidence("temper_measure", "tm_valuegap01"),
                        magnitude: 4.0,
                    },
                ),
            ),
            nick_weight_edit: Some(pb::backlog_rank_inputs_edit::NickWeightEdit::SetNickWeight(1.0)),
            dependency_readiness_edit: Some(
                pb::backlog_rank_inputs_edit::DependencyReadinessEdit::SetDependencyReadiness(
                    pb::BacklogDependencyReadiness {
                        status: "ready".to_string(),
                        blocker_refs: vec![],
                    },
                ),
            ),
        }),
        wake_condition_edit: None,
        superseded_by_edit: None,
    })
}

/// A SHAPE edit that stages only `superseded_by`. The value model admits this
/// key for `{nick_shape, orchestrator}` only, so it can never ride along on the
/// organ loop's shaping mutation.
fn supersede_shape(bi_id: &str, superseded_by: &str) -> pb::backlog_mutate_request::Operation {
    use pb::backlog_shape_edit as se;
    pb::backlog_mutate_request::Operation::ShapeEdit(pb::BacklogShapeEdit {
        backlog_item_id: bi_id.to_string(),
        action_class_edit: None,
        description_edit: None,
        effort_class_edit: None,
        playbook_binding_edit: None,
        rank_inputs: None,
        wake_condition_edit: None,
        superseded_by_edit: Some(se::SupersededByEdit::SetSupersededBy(
            superseded_by.to_string(),
        )),
    })
}

/// A SHAPE edit that stages only a wake condition, so a park row's guard is
/// satisfied by a real mutation rather than a hand-written file.
fn wake_shape(bi_id: &str) -> pb::backlog_mutate_request::Operation {
    use pb::backlog_shape_edit as se;
    pb::backlog_mutate_request::Operation::ShapeEdit(pb::BacklogShapeEdit {
        backlog_item_id: bi_id.to_string(),
        action_class_edit: None,
        description_edit: None,
        effort_class_edit: None,
        playbook_binding_edit: None,
        rank_inputs: None,
        wake_condition_edit: Some(se::WakeConditionEdit::SetWakeCondition(
            pb::BacklogWakeCondition {
                kind: "item_state".to_string(),
                reference: evidence("backlog_item", "bi_wake000001"),
                predicate: "state==done".to_string(),
            },
        )),
        superseded_by_edit: None,
    })
}

fn stamp_op(bi_id: &str) -> pb::backlog_mutate_request::Operation {
    pb::backlog_mutate_request::Operation::StampExecutionBinding(pb::BacklogStampExecutionBinding {
        backlog_item_id: bi_id.to_string(),
        execution_binding: Some(pb::BacklogExecutionBinding {
            track_id: "tr_seed01".to_string(),
            playbook_definition_id: "pd_seed01".to_string(),
            playbook_run_id: "wf::seed/any-shape 42".to_string(),
            run_id: "lr_abcdefghjkmnpqrstvwxyz0123".to_string(),
        }),
        outcome_binding: Some(pb::BacklogOutcomeBindingDecl {
            success_measure: Some(pb::backlog_outcome_binding_decl::SuccessMeasure::SuccessMeasureId(
                "sm_seed01".to_string(),
            )),
            tree_node: "tree/seed".to_string(),
            reading_status: "registered".to_string(),
        }),
    })
}

// ---------------------------------------------------------------------------
// Real gRPC drivers. Each returns the engine's own answer; nothing is faked on
// failure.
// ---------------------------------------------------------------------------

async fn call_begin(w: &BacklogEngineFixture, item_json: &str) -> Result<pb::BeginResponse, String> {
    let mut client = w.client().await?;
    client
        .begin(anvil_test_support::surfaced(begin_request(&w.hearth, item_json)))
        .await
        .map(|r| r.into_inner())
        .map_err(|s| format!("{}: {}", s.code(), s.message()))
}

async fn call_snapshot(
    w: &BacklogEngineFixture,
    bi_id: &str,
    to: &str,
    role: &str,
) -> Result<pb::SnapshotResponse, String> {
    let mut client = w.client().await?;
    client
        .snapshot(anvil_test_support::surfaced(snapshot_request(&w.hearth, bi_id, to, role)))
        .await
        .map(|r| r.into_inner())
        .map_err(|s| format!("{}: {}", s.code(), s.message()))
}

async fn call_mutate(
    w: &BacklogEngineFixture,
    hearth: &Path,
    role: &str,
    op: pb::backlog_mutate_request::Operation,
) -> Result<pb::BacklogMutateResponse, String> {
    let mut client = w.client().await?;
    client
        .backlog_mutate(anvil_test_support::surfaced(mutate_request(hearth, role, op)))
        .await
        .map(|r| r.into_inner())
        .map_err(|s| format!("{}: {}", s.code(), s.message()))
}

async fn call_evaluate(
    w: &BacklogEngineFixture,
    hearth: &Path,
    organ: &str,
) -> Result<pb::BacklogEvaluateResponse, String> {
    let mut client = w.client().await?;
    client
        .backlog_evaluate(anvil_test_support::surfaced(evaluate_request(hearth, organ)))
        .await
        .map(|r| r.into_inner())
        .map_err(|s| format!("{}: {}", s.code(), s.message()))
}

async fn call_queue(
    w: &BacklogEngineFixture,
    organ: &str,
) -> Result<pb::BacklogQueueResponse, String> {
    let mut client = w.client().await?;
    client
        .backlog_queue(anvil_test_support::surfaced(pb::BacklogQueueRequest {
            hearth_path: w.hearth.display().to_string(),
            scope: Some(pb::backlog_queue_request::Scope::BusinessNodeId(
                organ.to_string(),
            )),
        }))
        .await
        .map(|r| r.into_inner())
        .map_err(|s| format!("{}: {}", s.code(), s.message()))
}

/// Read the item's CURRENT state back out of the engine-written bytes. A
/// precondition builder asserts against this, so a recipe that silently fails
/// to reach its declared state fails the scenario instead of masking it.
fn stored_state(hearth: &Path, bi_id: &str) -> Result<String, String> {
    use anvil_core::ports::backlog_item_port::BacklogItemPort;
    anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
        hearth.to_path_buf(),
    )
    .load_item(bi_id)
    .map(|l| l.item.state.as_str().to_string())
    .map_err(|e| format!("load {bi_id}: {e}"))
}

// ---------------------------------------------------------------------------
// Preconditions — every one built from real Begin / BacklogMutate / Snapshot
// calls against the running engine.
// ---------------------------------------------------------------------------

async fn create_item(w: &BacklogEngineFixture, title: &str) -> Result<String, String> {
    let response = call_begin(w, &genesis_json(title)).await?;
    let bi_id = response
        .track_path
        .rsplit('/')
        .next()
        .filter(|s| s.starts_with("bi_"))
        .ok_or_else(|| format!("Begin returned an unexpected path: {}", response.track_path))?
        .to_string();
    w.with_state(|s| s.item_id = Some(bi_id.clone()))?;
    Ok(bi_id)
}

async fn shape_and_rank(w: &BacklogEngineFixture, bi_id: &str) -> Result<(), String> {
    call_mutate(w, &w.hearth, "organ_loop", full_shape(bi_id)).await?;
    recompute(w, &w.hearth).await
}

async fn recompute(w: &BacklogEngineFixture, hearth: &Path) -> Result<(), String> {
    call_mutate(
        w,
        hearth,
        "organ_loop",
        pb::backlog_mutate_request::Operation::RecomputeRank(pb::BacklogRecomputeRank {
            business_node_id: ORGAN.to_string(),
        }),
    )
    .await
    .map(|_| ())
}

async fn to_state(
    w: &BacklogEngineFixture,
    bi_id: &str,
    to: &str,
    role: &str,
) -> Result<(), String> {
    call_snapshot(w, bi_id, to, role).await?;
    let actual = stored_state(&w.hearth, bi_id)?;
    if actual != to {
        return Err(format!(
            "precondition drove {bi_id} to '{to}' but the store reads '{actual}'"
        ));
    }
    Ok(())
}

/// Reach `ready`: shaped, ranked, then the governed #1.
async fn ready(w: &BacklogEngineFixture, bi_id: &str) -> Result<(), String> {
    shape_and_rank(w, bi_id).await?;
    to_state(w, bi_id, "ready", "organ_loop").await
}

/// Reach `in_flight`: ready, stamped, then the approved #5 pickup.
async fn in_flight(w: &BacklogEngineFixture, bi_id: &str) -> Result<(), String> {
    ready(w, bi_id).await?;
    call_mutate(w, &w.hearth, "track_driver", stamp_op(bi_id)).await?;
    to_state(w, bi_id, "in_flight", "track_driver").await
}

/// Reach `parked` through the #7 row, with a real staged wake condition.
async fn parked(w: &BacklogEngineFixture, bi_id: &str) -> Result<(), String> {
    ready(w, bi_id).await?;
    call_mutate(w, &w.hearth, "organ_loop", wake_shape(bi_id)).await?;
    to_state(w, bi_id, "parked", "organ_loop").await
}

/// Advance the organ's committed rank age past the default budget of 3 using
/// only real `recompute_rank` mutations.
async fn age_past_budget(w: &BacklogEngineFixture, hearth: &Path) -> Result<(), String> {
    for _ in 0..4 {
        recompute(w, hearth).await?;
    }
    Ok(())
}

async fn build_precondition(w: &BacklogEngineFixture, name: &str) -> Result<(), String> {
    let bi_id = create_item(w, name).await?;
    match name {
        "valid_candidate" | "row_4" | "mutable_shape_edit" | "mutable_veto_age_out" => {}
        "mutable_recompute_rank" => {
            call_mutate(w, &w.hearth, "organ_loop", full_shape(&bi_id)).await?;
        }
        "row_1" | "ranked_organ" | "mutable_propose_reshuffle" => {
            shape_and_rank(w, &bi_id).await?;
        }
        "row_3" => {
            shape_and_rank(w, &bi_id).await?;
            // `superseded_by` admits only {nick_shape, orchestrator}: the
            // supersede staging is its own real mutation under Nick's role.
            call_mutate(
                w,
                &w.hearth,
                "nick_shape",
                supersede_shape(&bi_id, "bi_super0001"),
            )
            .await?;
        }
        "row_5" => {
            ready(w, &bi_id).await?;
            call_mutate(w, &w.hearth, "track_driver", stamp_op(&bi_id)).await?;
        }
        "mutable_stamp_execution_binding" | "row_9" => {
            ready(w, &bi_id).await?;
        }
        "mutable_record_outcome_signoff" => {
            in_flight(w, &bi_id).await?;
        }
        "row_10" => {
            in_flight(w, &bi_id).await?;
            // The in-scope Nick path: sign-off flips the binding to
            // `unmeasurable_signed` AND records the sign-off.
            call_mutate(
                w,
                &w.hearth,
                "nick_shape",
                pb::backlog_mutate_request::Operation::RecordOutcomeSignoff(
                    pb::BacklogRecordOutcomeSignoff {
                        backlog_item_id: bi_id.clone(),
                        approver: "Nick".to_string(),
                    },
                ),
            )
            .await?;
        }
        "row_10_reading" => {
            in_flight(w, &bi_id).await?;
            // OUT-OF-SCOPE PRODUCER (plan §3): Temper's registered -> reading
            // write has no K8 operation. This labeled precondition stands in
            // for it and never performs the transition under test.
            anvil_test_support::backlog_item::seed_external_temper_reading(&w.hearth, &bi_id)?;
        }
        "row_13" | "row_16" => {
            parked(w, &bi_id).await?;
        }
        "mutable_commit_reshuffle" | "mutable_reject_reshuffle" => {
            shape_and_rank(w, &bi_id).await?;
            let response = call_mutate(
                w,
                &w.hearth,
                "orchestrator",
                pb::backlog_mutate_request::Operation::ProposeReshuffle(pb::BacklogProposeReshuffle {
                    proposed: vec![pb::BacklogProposedPosition {
                        backlog_item_id: bi_id.clone(),
                        position: 1,
                    }],
                }),
            )
            .await?;
            if response.proposal_id.is_empty() {
                return Err("propose_reshuffle returned no proposal id".to_string());
            }
            w.with_state(|s| s.proposal_id = Some(response.proposal_id.clone()))?;
        }
        "mutable_lift_age_out_veto" => {
            call_mutate(
                w,
                &w.hearth,
                "nick_shape",
                pb::backlog_mutate_request::Operation::VetoAgeOut(pb::BacklogVetoAgeOut {
                    backlog_item_id: bi_id.clone(),
                    approver: "Nick".to_string(),
                }),
            )
            .await?;
        }
        "shaped_over_budget" => {
            shape_and_rank(w, &bi_id).await?;
            age_past_budget(w, &w.hearth).await?;
        }
        other => return Err(format!("unknown backlog engine precondition '{other}'")),
    }
    Ok(())
}

/// Build the named mutation the `@transport` operation table drives.
fn transport_operation(
    operation: &str,
    bi_id: &str,
    proposal_id: Option<&str>,
) -> Result<pb::backlog_mutate_request::Operation, String> {
    use pb::backlog_mutate_request::Operation as Op;
    Ok(match operation {
        "shape_edit" => full_shape(bi_id),
        "recompute_rank" => Op::RecomputeRank(pb::BacklogRecomputeRank {
            business_node_id: ORGAN.to_string(),
        }),
        "stamp_execution_binding" => stamp_op(bi_id),
        "record_outcome_signoff" => Op::RecordOutcomeSignoff(pb::BacklogRecordOutcomeSignoff {
            backlog_item_id: bi_id.to_string(),
            approver: "Nick".to_string(),
        }),
        "propose_reshuffle" => Op::ProposeReshuffle(pb::BacklogProposeReshuffle {
            proposed: vec![pb::BacklogProposedPosition {
                backlog_item_id: bi_id.to_string(),
                position: 1,
            }],
        }),
        "commit_reshuffle" => Op::CommitReshuffle(pb::BacklogCommitReshuffle {
            proposal_id: proposal_id
                .ok_or("commit_reshuffle needs an open proposal")?
                .to_string(),
            approver: "Nick".to_string(),
        }),
        "reject_reshuffle" => Op::RejectReshuffle(pb::BacklogRejectReshuffle {
            proposal_id: proposal_id
                .ok_or("reject_reshuffle needs an open proposal")?
                .to_string(),
            approver: "Nick".to_string(),
        }),
        "veto_age_out" => Op::VetoAgeOut(pb::BacklogVetoAgeOut {
            backlog_item_id: bi_id.to_string(),
            approver: "Nick".to_string(),
        }),
        "lift_age_out_veto" => Op::LiftAgeOutVeto(pb::BacklogLiftAgeOutVeto {
            backlog_item_id: bi_id.to_string(),
            approver: "Nick".to_string(),
        }),
        other => return Err(format!("unknown backlog mutation operation '{other}'")),
    })
}

// ---------------------------------------------------------------------------
// Alternate-writer routes. Each is a REAL production seam that must refuse a
// backlog_item, leaving every K8 byte untouched.
// ---------------------------------------------------------------------------

async fn bypass_attempt(w: &BacklogEngineFixture, route: &str) -> Result<Attempt, String> {
    let bi_id = w.item_id()?;
    let artifact_path = format!("backlog_items/{bi_id}");
    let (name, ty, model, provider) = actor_fields();
    let outcome: Result<String, String> = match route {
        "complete" => {
            let mut client = w.client().await?;
            client
                .complete(anvil_test_support::surfaced(pb::CompleteRequest {
                    hearth_path: w.hearth.display().to_string(),
                    artifact_path: artifact_path.clone(),
                    actor_name: name,
                    actor_type: ty,
                    actor_model: model,
                    actor_provider: provider,
                    actor_context_window: 200_000,
                    actor_sdk_version: "1.0.0".to_string(),
                    actor_entrypoint: "brine".to_string(),
                    ..Default::default()
                }))
                .await
                .map(|_| "Complete succeeded".to_string())
                .map_err(|s| format!("{}: {}", s.code(), s.message()))
        }
        "amend" => {
            let mut client = w.client().await?;
            client
                .amend(anvil_test_support::surfaced(pb::AmendRequest {
                    hearth_path: w.hearth.display().to_string(),
                    artifact_path: artifact_path.clone(),
                    actor_name: name,
                    actor_type: ty,
                    actor_model: model,
                    actor_provider: provider,
                    actor_context_window: 200_000,
                    actor_sdk_version: "1.0.0".to_string(),
                    actor_entrypoint: "brine".to_string(),
                    ..Default::default()
                }))
                .await
                .map(|_| "Amend succeeded".to_string())
                .map_err(|s| format!("{}: {}", s.code(), s.message()))
        }
        "adoption" => {
            let mut client = w.client().await?;
            let mut request = begin_request(&w.hearth, "");
            request.artifact_type = String::new();
            request.create_fields.clear();
            request.identifier = bi_id.clone();
            request.adopt = true;
            request.session_role = "resumer".to_string();
            client
                .begin(anvil_test_support::surfaced(request))
                .await
                .map(|_| "adoption Begin succeeded".to_string())
                .map_err(|s| format!("{}: {}", s.code(), s.message()))
        }
        "internal_execute" => {
            // The INTERNAL route: the real command handler over the real
            // filesystem adapter for the engine-owned hearth.
            use anvil_core::domain::snapshot::{SnapshotCommandHandler, SnapshotRequest};
            let adapter = anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter::new(
                w.hearth.clone(),
            );
            let actor_writer =
                anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter::new(
                    w.hearth.clone(),
                );
            let request = SnapshotRequest {
                artifact_path: artifact_path.clone(),
                to_state: "ready".to_string(),
                actor_name: name,
                actor_role: "organ_loop".to_string(),
                actor_type: ty,
                actor_model: model,
                actor_provider: provider,
                actor_context_window: 200_000,
                actor_sdk_version: "1.0.0".to_string(),
                actor_entrypoint: "brine".to_string(),
                at: "2026-07-25T00:00:00Z".to_string(),
                ..Default::default()
            };
            SnapshotCommandHandler::execute(&adapter, &actor_writer, request)
                .map(|_| "internal execute succeeded".to_string())
                .map_err(|e| e.to_string())
        }
        "raw_append" => {
            use anvil_core::domain::shared_types::TransitionContent;
            use anvil_core::ports::snapshot_port::SnapshotPort;
            let adapter = anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter::new(
                w.hearth.clone(),
            );
            adapter
                .append_transition(
                    &artifact_path,
                    &TransitionContent {
                        to: "ready".to_string(),
                        at: "2026-07-25T00:00:00Z".to_string(),
                        actor: name,
                        role: "organ_loop".to_string(),
                        approver: None,
                        note: None,
                        satisfaction: None,
                        event_type: None,
                    },
                )
                .map(|_| "raw append succeeded".to_string())
                .map_err(|e| e.to_string())
        }
        other => return Err(format!("unknown bypass route '{other}'")),
    };
    Ok(match outcome {
        Ok(message) => Attempt::Ok {
            state: Some(message),
            path: None,
        },
        Err(message) => Attempt::Error { message },
    })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a running backlog engine",
            &[],
            &[(WORLD, WORLD_TY)],
            |_ctx, _params| async move {
                // Spawning blocks on a real gRPC HealthCheck, so this runs on a
                // dedicated thread rather than the brine runtime.
                let fixture = tokio::task::spawn_blocking(BacklogEngineFixture::new)
                    .await
                    .map_err(|e| format!("engine spawn task failed: {e}"))??;
                Ok(carry(fixture))
            },
        ),
        async_step_def(
            "a backlog item created through Begin with input {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let name = p(&params, 0)?;
                build_precondition(&w, &name).await?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a backlog Begin genesis RPC with input {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let name = p(&params, 0)?;
                if name == "reread_after_restart" {
                    // Prove persistence across a REAL engine restart: the
                    // previously created item must still read back as exactly
                    // created(seq:0) with nothing else on disk.
                    let existing = w.item_id()?;
                    let w2 = w.clone();
                    tokio::task::spawn_blocking(move || w2.restart())
                        .await
                        .map_err(|e| format!("engine restart task failed: {e}"))??;
                    assert_sole_created_entry(&w.hearth, &existing)?;
                }
                w.capture()?;
                let json = anvil_test_support::backlog_item::genesis_input_json(&name)?;
                let outcome = call_begin(&w, &json).await.map(|r| Attempt::Ok {
                    state: Some(r.state),
                    path: Some(r.track_path),
                });
                w.record_result(outcome)?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a backlog Snapshot RPC from {string} to {string} with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let from = p(&params, 0)?;
                let to = p(&params, 1)?;
                let role = p(&params, 2)?;
                let bi_id = w.item_id()?;
                let actual = stored_state(&w.hearth, &bi_id)?;
                if actual != from {
                    return Err(format!(
                        "the scenario declares source state '{from}' but {bi_id} reads '{actual}'"
                    ));
                }
                w.capture()?;
                let outcome = call_snapshot(&w, &bi_id, &to, &role)
                    .await
                    .and_then(|_| {
                        stored_state(&w.hearth, &bi_id).map(|state| Attempt::Ok {
                            state: Some(state),
                            path: None,
                        })
                    });
                w.record_result(outcome)?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a BacklogMutate RPC for operation {string} with role {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let operation = p(&params, 0)?;
                let role = p(&params, 1)?;
                // A domain-rejection scenario names no precondition item; the
                // request still carries a syntactically valid id so the
                // rejection is the ROLE, not a missing argument.
                let bi_id = w
                    .with_state(|s| s.item_id.clone())?
                    .unwrap_or_else(|| "bi_absent0001".to_string());
                let proposal_id = w.with_state(|s| s.proposal_id.clone())?;
                let op = transport_operation(&operation, &bi_id, proposal_id.as_deref())?;
                w.capture()?;
                let outcome = call_mutate(&w, &w.hearth, &role, op).await.map(|r| Attempt::Ok {
                    state: Some(r.operation),
                    path: None,
                });
                w.record_result(outcome)?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a BacklogEvaluate RPC for organ {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let organ = p(&params, 0)?;
                w.capture()?;
                let hearth = w.hearth.clone();
                let outcome = call_evaluate(&w, &hearth, &organ).await;
                match &outcome {
                    Ok(response) => {
                        let aged = response.aged_out_item_ids.clone();
                        w.with_state(|s| s.aged_out = aged)?;
                    }
                    Err(_) => {}
                }
                w.record_result(outcome.map(|_| Attempt::Ok {
                    state: None,
                    path: None,
                }))?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a BacklogQueue RPC for organ {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let organ = p(&params, 0)?;
                w.capture()?;
                let outcome = call_queue(&w, &organ).await.and_then(|r| {
                    // A queue read is a projection, never a write: it must
                    // report the ranked item the precondition materialized.
                    if r.items.is_empty() && r.unranked_items.is_empty() {
                        Err("BacklogQueue returned neither a ranked nor an unranked partition".to_string())
                    } else {
                        Ok(Attempt::Ok {
                            state: None,
                            path: None,
                        })
                    }
                });
                w.record_result(outcome)?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a raw Snapshot bypass attempt via {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let route = p(&params, 0)?;
                w.capture()?;
                let attempt = bypass_attempt(&w, &route).await?;
                w.record(attempt)?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "the backlog service RPC count is inspected",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, _params| async move {
                let w = world(&ctx)?;
                w.capture()?;
                // Counted from the COMPILED descriptor set, not a hand list.
                let names = anvil_engine::service_rpc_names()?;
                if names.len() != EXPECTED_RPC_COUNT {
                    return Err(format!(
                        "AnvilService must declare exactly {EXPECTED_RPC_COUNT} RPCs after the K8 \
                         append, but the compiled descriptor declares {}",
                        names.len()
                    ));
                }
                let tail: Vec<&str> = names.iter().rev().take(3).rev().map(|s| s.as_str()).collect();
                if tail != ["BacklogMutate", "BacklogEvaluate", "BacklogQueue"] {
                    return Err(format!(
                        "the three K8 RPCs must be appended LAST, in order; tail was {tail:?}"
                    ));
                }
                // And the running engine must still answer at the unchanged
                // wire version — an append never bumps it.
                let mut client = w.client().await?;
                let health = client
                    .health_check(anvil_test_support::surfaced(pb::HealthCheckRequest {}))
                    .await
                    .map(|r| r.into_inner())
                    .map_err(|s| format!("{}: {}", s.code(), s.message()))?;
                if health.wire_proto_version != EXPECTED_WIRE_PROTO_VERSION {
                    return Err(format!(
                        "an RPC append must leave WIRE_PROTO_VERSION at {EXPECTED_WIRE_PROTO_VERSION}, \
                         but the engine reports {}",
                        health.wire_proto_version
                    ));
                }
                w.record(Attempt::Ok {
                    state: Some(names.len().to_string()),
                    path: None,
                })?;
                Ok(carry(w))
            },
        ),
        async_step_def(
            "a second backlog hearth sets age budget {string}",
            &[(WORLD, WORLD_TY)],
            &[(WORLD, WORLD_TY)],
            |ctx, params| async move {
                let w = world(&ctx)?;
                let budget = p(&params, 0)?;
                // §9 cross-hearth policy leak: a SECOND resolved hearth, served
                // by the SAME engine process, writes its own engine-flags.env
                // budget and holds an item aged past the FIRST hearth's default
                // budget but well within its own. K8 policy must resolve
                // strictly per request, never installed process-wide.
                // Nested INSIDE the engine's permitted root so the SAME
                // engine process serves both hearths — the in-process leak this
                // scenario exists to rule out.
                let second = w.hearth.join("second_hearth");
                seed_hearth(&second)?;
                std::fs::write(
                    second.join("engine-flags.env"),
                    format!("ANVIL_BACKLOG_AGE_BUDGET={budget}\n"),
                )
                .map_err(|e| format!("write engine-flags.env: {e}"))?;

                let response = {
                    let mut client = w.client().await?;
                    let mut request = begin_request(&second, &genesis_json("second hearth item"));
                    request.hearth_path = second.display().to_string();
                    client
                        .begin(anvil_test_support::surfaced(request))
                        .await
                        .map(|r| r.into_inner())
                        .map_err(|s| format!("{}: {}", s.code(), s.message()))?
                };
                let second_id = response
                    .track_path
                    .rsplit('/')
                    .next()
                    .ok_or("second-hearth Begin returned no path")?
                    .to_string();
                call_mutate(&w, &second, "organ_loop", full_shape(&second_id)).await?;
                age_past_budget(&w, &second).await?;

                w.with_state(|s| {
                    s.second = Some(SecondHearth {
                        path: second.clone(),
                        item_id: second_id.clone(),
                    })
                })?;
                Ok(carry(w))
            },
        ),
        // ── Then ─────────────────────────────────────────────────────────────
        check_def(
            "the backlog RPC succeeds",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_ok(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the backlog RPC state is {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let (state, _) = expect_ok(&world(&ctx)?)?;
                let want = p(&params, 0)?;
                match state {
                    Some(s) if s == want => Ok(()),
                    other => Err(format!("expected state {want:?} but was {other:?}")),
                }
            },
        ),
        check_def(
            "the stored backlog history ends with {string} by role {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                expect_ok(&w)?;
                let want_kind = p(&params, 0)?;
                let want_role = p(&params, 1)?;
                let bi_id = w.item_id()?;
                let history = stored_history(&w.hearth, &bi_id)?;
                let last = history
                    .last()
                    .ok_or_else(|| format!("{bi_id} has no history at all"))?;
                if history.len() < 2 {
                    return Err(format!(
                        "{bi_id} still holds only {} entry: the RPC returned OK but appended \
                         nothing",
                        history.len()
                    ));
                }
                if last.kind.as_str() != want_kind {
                    let kinds: Vec<&str> = history.iter().map(|h| h.kind.as_str()).collect();
                    return Err(format!(
                        "expected the operation to append {want_kind:?} last, but the stored \
                         ledger reads {kinds:?}"
                    ));
                }
                if last.role.as_str() != want_role {
                    return Err(format!(
                        "expected the {want_kind:?} entry to be written by role {want_role:?} \
                         but it was written by {:?}",
                        last.role.as_str()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the stored backlog item shows {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                expect_ok(&w)?;
                let token = p(&params, 0)?;
                let bi_id = w.item_id()?;
                stored_effect_holds(&w.hearth, &bi_id, &token)
            },
        ),
        check_def(
            "the evaluation batch names the item",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                expect_ok(&w)?;
                let bi_id = w.item_id()?;
                let aged = w.with_state(|s| s.aged_out.clone())?;
                if !aged.contains(&bi_id) {
                    return Err(format!(
                        "BacklogEvaluate returned batch {aged:?}, which does not name the \
                         over-budget item {bi_id}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the created backlog path matches {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let (_, path) = expect_ok(&w)?;
                let prefix = p(&params, 0)?;
                let path = path.ok_or("the Begin response carried no path")?;
                if !path.starts_with(&prefix) {
                    return Err(format!("expected path under {prefix:?} but was {path:?}"));
                }
                // The path is not a claim: the exact directory must exist.
                let dir = w.hearth.join(&path);
                if !dir.join("item.yaml").is_file() {
                    return Err(format!("{} has no item.yaml", dir.display()));
                }
                Ok(())
            },
        ),
        check_def(
            "the backlog RPC returns a non-OK status",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                expect_error(&world(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the backlog RPC error contains {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let msg = expect_error(&world(&ctx)?)?;
                let want = p(&params, 0)?;
                if msg.contains(&want) {
                    Ok(())
                } else {
                    Err(format!("expected error containing {want:?} but was {msg:?}"))
                }
            },
        ),
        check_def(
            "the backlog engine files are byte-identical",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                // A rejected bypass must leave every K8 file byte-identical to
                // the bytes captured immediately before the attempt.
                let before = w
                    .with_state(|s| s.before.clone())?
                    .ok_or("no pre-operation byte capture")?;
                let after = k8_bytes(&w.hearth)?;
                if before == after {
                    return Ok(());
                }
                let changed: Vec<String> = before
                    .keys()
                    .chain(after.keys())
                    .filter(|k| before.get(*k) != after.get(*k))
                    .cloned()
                    .collect();
                Err(format!("K8 bytes changed on a refused operation: {changed:?}"))
            },
        ),
        check_def(
            "the second-hearth backlog policy is unaffected",
            &[(WORLD, WORLD_TY)],
            |ctx, _params| {
                let w = world(&ctx)?;
                // Direction 1: the first hearth kept its DEFAULT budget, so its
                // over-budget item aged out. If the second hearth's 99 had been
                // installed process-wide, nothing would have aged out.
                let aged = w.with_state(|s| s.aged_out.clone())?;
                let first_id = w.item_id()?;
                if !aged.contains(&first_id) {
                    return Err(format!(
                        "the first hearth's default budget should have aged out {first_id}, \
                         but the evaluation aged out {aged:?} — the second hearth's budget leaked"
                    ));
                }
                // Direction 2: the second hearth's own budget still governs its
                // own item, which is aged past 3 but far inside 99.
                let (second_path, second_id) = w
                    .with_state(|s| s.second.as_ref().map(|h| (h.path.clone(), h.item_id.clone())))?
                    .ok_or("no second hearth was prepared")?;
                let state = stored_state(&second_path, &second_id)?;
                if state == "aged_out" {
                    return Err(format!(
                        "{second_id} aged out under a budget of 99 — the first hearth's default leaked"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no backlog directory exists under {string}",
            &[(WORLD, WORLD_TY)],
            |ctx, params| {
                let w = world(&ctx)?;
                let sub = p(&params, 0)?;
                let dir = w.hearth.join(&sub);
                let residue = std::fs::read_dir(&dir)
                    .map(|mut it| it.next().is_some())
                    .unwrap_or(false);
                if residue {
                    Err(format!("unexpected residue under {}", dir.display()))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}

/// Read the item's CURRENT history back out of the engine-written bytes.
fn stored_history(
    hearth: &Path,
    bi_id: &str,
) -> Result<Vec<anvil_core::domain::backlog_item::HistoryEntry>, String> {
    use anvil_core::ports::backlog_item_port::BacklogItemPort;
    anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
        hearth.to_path_buf(),
    )
    .load_item(bi_id)
    .map(|l| l.history)
    .map_err(|e| format!("load {bi_id}: {e}"))
}

/// Assert one named on-disk effect of a transport operation. Every token reads
/// the bytes the ENGINE wrote — never the request the fixture just sent.
fn stored_effect_holds(hearth: &Path, bi_id: &str, token: &str) -> Result<(), String> {
    use anvil_core::ports::backlog_item_port::BacklogItemPort;
    let loaded = anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
        hearth.to_path_buf(),
    )
    .load_item(bi_id)
    .map_err(|e| format!("load {bi_id}: {e}"))?;
    let item = &loaded.item;
    let last_payload =
        || -> Option<serde_yaml::Value> { loaded.history.last().and_then(|h| h.payload.clone()) };
    match token {
        "effort_class_and_playbook" => {
            if item.effort_class.is_none() {
                return Err(format!("{bi_id} carries no effort_class after shape_edit"));
            }
            if item.playbook_binding.is_none() {
                return Err(format!(
                    "{bi_id} carries no playbook_binding after shape_edit"
                ));
            }
            Ok(())
        }
        "materialized_rank" => {
            let rank = item
                .rank
                .as_ref()
                .ok_or_else(|| format!("{bi_id} carries no rank"))?;
            if rank.position == 0 {
                return Err(format!("{bi_id} holds an invalid rank position 0"));
            }
            Ok(())
        }
        "rank_position_1" => {
            let rank = item
                .rank
                .as_ref()
                .ok_or_else(|| format!("{bi_id} carries no rank"))?;
            if rank.position != 1 {
                return Err(format!(
                    "{bi_id} holds rank position {} but the approved proposal named 1",
                    rank.position
                ));
            }
            Ok(())
        }
        "execution_and_outcome_binding" => {
            if item.execution_binding.is_none() {
                return Err(format!("{bi_id} carries no execution_binding after stamp"));
            }
            if item.outcome_binding.is_none() {
                return Err(format!("{bi_id} carries no outcome_binding after stamp"));
            }
            Ok(())
        }
        "nick_signoff" => {
            let ob = item
                .outcome_binding
                .as_ref()
                .ok_or_else(|| format!("{bi_id} carries no outcome_binding"))?;
            if !ob.nick_signoff {
                return Err(format!("{bi_id} still reads nick_signoff: false"));
            }
            Ok(())
        }
        "veto_set" | "veto_lift" => {
            let want = if token == "veto_set" { "set" } else { "lift" };
            let payload = last_payload()
                .ok_or_else(|| format!("the last {bi_id} history entry carries no payload"))?;
            let action = payload
                .get("veto_action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if action != want {
                return Err(format!(
                    "expected the veto entry to record veto_action {want:?} but it recorded \
                     {action:?}"
                ));
            }
            if want == "lift" && payload.get("lifts_seq").and_then(|v| v.as_u64()).is_none() {
                return Err("a lift entry must name the sequence it lifts".to_string());
            }
            Ok(())
        }
        "state_aged_out" => {
            if item.state.as_str() != "aged_out" {
                return Err(format!(
                    "expected the over-budget item to read aged_out but it reads {:?}",
                    item.state.as_str()
                ));
            }
            Ok(())
        }
        other => Err(format!("unknown stored-effect token '{other}'")),
    }
}

/// A published item's history must be exactly `created(seq: 0)` — the genesis
/// invariant a restart has to preserve.
fn assert_sole_created_entry(hearth: &Path, bi_id: &str) -> Result<(), String> {
    use anvil_core::ports::backlog_item_port::BacklogItemPort;
    let loaded = anvil_core_hearth::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
        hearth.to_path_buf(),
    )
    .load_item(bi_id)
    .map_err(|e| format!("reload {bi_id} after restart: {e}"))?;
    let kinds: Vec<String> = loaded
        .history
        .iter()
        .map(|h| format!("{}({})", h.kind.as_str(), h.seq))
        .collect();
    if kinds != vec!["created(0)".to_string()] {
        return Err(format!(
            "a restarted genesis must persist exactly created(seq:0); found {kinds:?}"
        ));
    }
    Ok(())
}
