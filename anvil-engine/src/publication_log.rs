//! Crucible CQRS publication log — anvil's canonical JSONL event sink.
//!
//! This is the shared **Crucible CQRS** standard (foundry
//! `docs/kit-event-publication-contract.md`; forge `CLAUDE.md` §3; temper Rule
//! 8): every kit persists its domain events, append-only, to
//! `~/.<base>/publication-log/events-YYYY-MM-DD.jsonl` (UTC daily rotation) as
//! one canonical envelope per line, so the whole fleet's events are
//! traceable / replayable / grep-able in ONE consistent shape.
//!
//! anvil already HAS a CQRS pipeline — a domain `Event` enum (plus
//! `CompleteEvent` / `AmendEvent` / `PersistPlaybookEvent`) emitted by the pure
//! handlers and dispatched in `main.rs`. The gap this module closes is the
//! SINK: those events were never persisted to the standard publication log.
//! This module adds a best-effort MIRROR teed off the existing dispatch loops
//! (the authoritative writes — status.yaml, registry, projections — are
//! untouched and remain fatal-on-error).
//!
//! ## Why an inline writer (not a `foundry-kit-engine` dep)
//!
//! anvil does NOT depend on `foundry-kit-engine` (it pulls in only
//! `foundry-engine-addressing` for the rendezvous record). Adding the whole
//! kit-engine crate (Bus, EventStore, SQLite, …) just for the JSONL writer is
//! disproportionate, and `JsonlPublicationLog::append` is keyed on the engine's
//! own `DomainEvent` struct which anvil's enums are not. lore made the same
//! call (vendor a small conformant inline writer rather than add the dep). This
//! writer matches the contract EXACTLY: canonical envelope, `events-YYYY-MM-DD`
//! daily UTC rotation via a `Mutex<WriteState>` that caches the dated path, dir
//! auto-create on first append.
//!
//! ## Canonical envelope (one JSON object per line)
//! ```json
//! {"eventId":"<uuid-v4>","aggregateId":"<agg>","kind":"<EventVariant>","timestamp":"<rfc3339-utc>","data":<payload>}
//! ```
//! `aggregateId` is omitted when the variant carries no natural correlation id.
//!
//! ## Activation gate (keep anvil's own tests inert)
//!
//! The mirror is BEST-EFFORT and GATED. [`PublicationLog::from_env`] returns
//! `Some` only when the engine is running on its real data dir:
//!
//! - `$FOUNDRY_PUBLICATION_LOG_DIR` set (non-empty) ⇒ write there verbatim
//!   (Brine/unit tests that DO want to assert the log point this at a temp dir);
//! - else, when `$ANVIL_TEMPER_HOME` is ABSENT ⇒ default to
//!   `~/.anvil/publication-log/`. `ANVIL_TEMPER_HOME` is anvil's established
//!   "we are running inside a test temp-tree" signal — the Brine engine harness
//!   always sets it (to redirect the §0 temper stream), production never does.
//!   So Brine stays publication-free without polluting the developer's real
//!   `~/.anvil/`, and a real engine activates the standard sink automatically.
//! - else (test tree, no explicit override) ⇒ `None`, fully inert.
//!
//! Any write failure is swallowed and logged once — a mirror I/O error must
//! NEVER break the engine or an event's real handling.

use anvil_core::domain::amend_events::AmendEvent;
use anvil_core::domain::complete_events::CompleteEvent;
use anvil_core::domain::events::Event;
use anvil_core::domain::persist_playbook_events::PersistPlaybookEvent;
use anvil_core::domain::shared_types::ActorIdentity;
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Env override for the publication-log directory (the Crucible standard name).
/// When set (non-empty) it takes precedence over the per-kit default — Brine
/// fixtures and unit tests point this at a temp dir so they can assert the log
/// without touching a real `~/.anvil/`.
pub const PUBLICATION_LOG_DIR_ENV: &str = "FOUNDRY_PUBLICATION_LOG_DIR";

/// anvil's "running inside a test temp-tree" signal (the Brine engine harness
/// always sets it to redirect the §0 temper stream). Its ABSENCE is what lets
/// the default `~/.anvil/publication-log/` sink activate for a real engine.
const TEST_TREE_SIGNAL_ENV: &str = "ANVIL_TEMPER_HOME";

/// The "ephemeral / test engine — do NOT touch real user state" signal. Every
/// test-engine spawn helper already sets it to suppress the `~/.anvil/engine.json`
/// rendezvous record (main.rs). The publication log is the SAME class of real-
/// deployment side effect, so the same signal must keep it inert — otherwise test
/// engines that spawn WITHOUT redirecting the log (the RPC/orchestrate/mcp harness
/// helpers) write their fixture events into the production `~/.anvil/publication-log/`
/// and pollute fleet adoption measurement. An explicit `FOUNDRY_PUBLICATION_LOG_DIR`
/// still wins (checked first), so a test that ASSERTS on the log is unaffected.
const RENDEZVOUS_DISABLE_ENV: &str = "ANVIL_RENDEZVOUS_DISABLE";

/// Append-only JSONL event log at the canonical Crucible path for anvil, with
/// UTC daily rotation. Best-effort: every public write swallows + logs-once on
/// failure and never propagates an error to the caller.
pub struct PublicationLog {
    dir: PathBuf,
    state: Mutex<WriteState>,
    /// Latches true after the first logged failure so a broken sink doesn't
    /// spam the engine log on every event.
    warned: AtomicBool,
}

#[derive(Default)]
struct WriteState {
    /// Cached `YYYY-MM-DD` of the currently-resolved file, so we only rebuild
    /// the path string when the UTC day actually rolls over.
    date: String,
    path: PathBuf,
}

impl PublicationLog {
    /// Construct a log writing into `dir` (created on first append).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            state: Mutex::new(WriteState::default()),
            warned: AtomicBool::new(false),
        }
    }

    /// The activation gate (see module docs). `Some(log)` only on a real data
    /// dir; `None` keeps anvil's own tests inert.
    pub fn from_env() -> Option<Self> {
        if let Some(dir) = std::env::var_os(PUBLICATION_LOG_DIR_ENV).filter(|s| !s.is_empty()) {
            return Some(Self::new(PathBuf::from(dir)));
        }
        // No explicit override: only activate the default sink for a REAL engine —
        // never for an ephemeral/test engine. Both the temp-tree signal and the
        // rendezvous-disable signal mark "do not touch real user state".
        if std::env::var_os(TEST_TREE_SIGNAL_ENV).is_some()
            || std::env::var_os(RENDEZVOUS_DISABLE_ENV).is_some()
        {
            return None;
        }
        Self::default_dir().map(Self::new)
    }

    /// `~/.anvil/publication-log/` — the same `~/.anvil/` the engine.json
    /// rendezvous record lives under. `None` when `$HOME` can't be resolved.
    pub fn default_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".anvil").join("publication-log"))
    }

    /// The directory this log writes into.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn path_for_date(&self, date: &str) -> PathBuf {
        self.dir.join(format!("events-{date}.jsonl"))
    }

    /// Resolve (cached) the current UTC day's file, rolling at midnight UTC.
    fn current_path(&self) -> PathBuf {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.date != today {
            st.date = today.clone();
            st.path = self.path_for_date(&today);
        }
        st.path.clone()
    }

    /// Append one canonical-envelope line. Returns the io error to the caller
    /// (used by [`Self::mirror`], which swallows it).
    fn append_line(&self, line: &str) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.current_path();
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(f, "{line}")
    }

    /// Best-effort mirror of one domain event. Builds the canonical envelope
    /// and appends it; a write failure is logged ONCE (latched) and swallowed
    /// — it never breaks the event's real handling.
    pub fn mirror(&self, kind: &str, aggregate_id: Option<&str>, data: serde_json::Value) {
        let mut envelope = serde_json::Map::new();
        envelope.insert("eventId".into(), serde_json::json!(new_uuid_v4()));
        if let Some(agg) = aggregate_id {
            envelope.insert("aggregateId".into(), serde_json::json!(agg));
        }
        envelope.insert("kind".into(), serde_json::json!(kind));
        envelope.insert(
            "timestamp".into(),
            serde_json::json!(Utc::now().to_rfc3339()),
        );
        envelope.insert("data".into(), data);
        let line = serde_json::Value::Object(envelope).to_string();
        if let Err(e) = self.append_line(&line) {
            if !self.warned.swap(true, Ordering::SeqCst) {
                tracing::warn!(
                    outcome = "publication_log_append_failed",
                    error = %e,
                    dir = %self.dir.display(),
                    "Crucible publication-log mirror append failed (best-effort; further failures suppressed)"
                );
            }
        }
    }

    /// Mirror a begin-handler [`Event`].
    pub fn mirror_begin(&self, event: &Event) {
        let (kind, agg, data) = begin_envelope(event);
        self.mirror(kind, agg.as_deref(), data);
    }

    /// Mirror a [`CompleteEvent`].
    pub fn mirror_complete(&self, event: &CompleteEvent) {
        let (kind, agg, data) = complete_envelope(event);
        self.mirror(kind, agg.as_deref(), data);
    }

    /// Mirror an [`AmendEvent`].
    pub fn mirror_amend(&self, event: &AmendEvent) {
        let (kind, agg, data) = amend_envelope(event);
        self.mirror(kind, agg.as_deref(), data);
    }

    /// Mirror a [`PersistPlaybookEvent`].
    pub fn mirror_persist(&self, event: &PersistPlaybookEvent) {
        let (kind, agg, data) = persist_envelope(event);
        self.mirror(kind, agg.as_deref(), data);
    }
}

/// Serialize an [`ActorIdentity`] for the envelope `data` payload (the domain
/// enums don't derive `Serialize`, so we project the fields explicitly).
fn actor_json(actor: &ActorIdentity) -> serde_json::Value {
    serde_json::json!({
        "name": actor.name,
        "actor_type": actor.actor_type,
        "model": actor.model,
        "provider": actor.provider,
        "context_window": actor.context_window,
        "sdk_version": actor.sdk_version,
        "entrypoint": actor.entrypoint,
        "registered_at": actor.registered_at,
    })
}

/// Map a begin [`Event`] variant to `(kind, aggregate_id, data)`. `kind` is the
/// variant name; `aggregate_id` is the natural correlation key the variant
/// carries (the artifact / track path); `data` is the variant's payload.
fn begin_envelope(event: &Event) -> (&'static str, Option<String>, serde_json::Value) {
    match event {
        // K8 genesis mirrors ids and placement ONLY — never the typed item
        // body, which is authoritative on disk and not a log payload.
        Event::BacklogItemCreation {
            item,
            actor,
            conversation_id,
            ..
        } => (
            "BacklogItemCreation",
            Some(format!("backlog_items/{}", item.backlog_item_id)),
            serde_json::json!({
                "backlog_item_id": item.backlog_item_id,
                "business_node_id": item.business_node_id,
                "state": item.state.as_str(),
                "actor": actor_json(actor),
                "conversation_id": conversation_id,
            }),
        ),
        Event::ReviewTransition {
            track_path,
            to_state,
            actor,
            role,
            approver,
            note,
        } => (
            "ReviewTransition",
            Some(track_path.clone()),
            serde_json::json!({
                "track_path": track_path,
                "to_state": to_state,
                "actor": actor_json(actor),
                "role": role,
                "approver": approver,
                "note": note,
            }),
        ),
        Event::ArtifactAdopted {
            track_path,
            to_state,
            actor,
            role,
            note,
        } => (
            "ArtifactAdopted",
            Some(track_path.clone()),
            serde_json::json!({
                "track_path": track_path,
                "to_state": to_state,
                "actor": actor_json(actor),
                "role": role,
                "note": note,
                "event_type": "adoption",
            }),
        ),
        Event::ReviewDocCreated {
            track_path,
            doc_name,
            header,
        } => (
            "ReviewDocCreated",
            Some(track_path.clone()),
            serde_json::json!({
                "track_path": track_path,
                "doc_name": doc_name,
                "header": header,
            }),
        ),
        Event::ArtifactCreation {
            track_name,
            parent_id,
            display_name,
            actor,
            approver,
            status,
            directory,
            registry_file,
            scaffold_files,
            creation_role,
            conversation_id,
        } => (
            "ArtifactCreation",
            Some(track_name.clone()),
            serde_json::json!({
                "track_name": track_name,
                "parent_id": parent_id,
                "display_name": display_name,
                "actor": actor_json(actor),
                "approver": approver,
                "status_kind": status.kind,
                "status_state": status.state,
                "target_owner": status.target_owner,
                "directory": directory,
                "registry_file": registry_file,
                "scaffold_files": scaffold_files
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                "creation_role": creation_role,
                "conversation_id": conversation_id,
            }),
        ),
        Event::ProjectionOnlySnapshot {
            artifact_path,
            event_type,
            body,
            actor,
        } => (
            "ProjectionOnlySnapshot",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "event_type": event_type,
                "body": body,
                "actor": actor_json(actor),
            }),
        ),
        Event::BeginMarkerWritten {
            artifact_path,
            kind,
            actor,
            state,
            at,
            conversation_id,
        } => (
            "BeginMarkerWritten",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "kind": kind,
                "actor": actor,
                "state": state,
                "at": at,
                "conversation_id": conversation_id,
            }),
        ),
        Event::PlaybookCreation {
            playbook_name,
            parent_id,
            actor,
            approver,
            status,
        } => (
            "PlaybookCreation",
            Some(playbook_name.clone()),
            serde_json::json!({
                "playbook_name": playbook_name,
                "parent_id": parent_id,
                "actor": actor_json(actor),
                "approver": approver,
                "status_kind": status.kind,
                "status_state": status.state,
            }),
        ),
    }
}

/// Map a [`CompleteEvent`] variant to `(kind, aggregate_id, data)`.
fn complete_envelope(event: &CompleteEvent) -> (&'static str, Option<String>, serde_json::Value) {
    match event {
        CompleteEvent::TransitionRecorded {
            artifact_path,
            to_state,
            at,
            role,
            approver,
            note,
            actor_name,
            satisfaction,
        } => (
            "TransitionRecorded",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "to_state": to_state,
                "at": at,
                "role": role,
                "approver": approver,
                "note": note,
                "actor_name": actor_name,
                "satisfaction": satisfaction,
            }),
        ),
        CompleteEvent::CarryForwardWritten {
            artifact_path,
            body,
        } => (
            "CarryForwardWritten",
            Some(artifact_path.clone()),
            serde_json::json!({ "artifact_path": artifact_path, "body": body }),
        ),
        CompleteEvent::ActorUpserted {
            artifact_path,
            identity,
        } => (
            "ActorUpserted",
            Some(artifact_path.clone()),
            serde_json::json!({ "artifact_path": artifact_path, "identity": actor_json(identity) }),
        ),
        CompleteEvent::ReflectionWritten {
            artifact_path,
            source_state,
            filename,
            body,
        } => (
            "ReflectionWritten",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "source_state": source_state,
                "filename": filename,
                "body": body,
            }),
        ),
    }
}

/// Map an [`AmendEvent`] variant to `(kind, aggregate_id, data)`.
fn amend_envelope(event: &AmendEvent) -> (&'static str, Option<String>, serde_json::Value) {
    match event {
        AmendEvent::OpRecorded {
            artifact_path,
            target_document,
            entry,
        } => (
            "OpRecorded",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "target_document": target_document,
                "op_id": entry.op_id,
            }),
        ),
        AmendEvent::ActorUpserted {
            artifact_path,
            identity,
        } => (
            "ActorUpserted",
            Some(artifact_path.clone()),
            serde_json::json!({ "artifact_path": artifact_path, "identity": actor_json(identity) }),
        ),
        AmendEvent::TransitionRecorded {
            artifact_path,
            to_state,
            at,
            role,
            actor_name,
        } => (
            "TransitionRecorded",
            Some(artifact_path.clone()),
            serde_json::json!({
                "artifact_path": artifact_path,
                "to_state": to_state,
                "at": at,
                "role": role,
                "actor_name": actor_name,
            }),
        ),
    }
}

/// Map a [`PersistPlaybookEvent`] variant to `(kind, aggregate_id, data)`. The
/// correlation key is the playbook kind (the natural id this event is about).
fn persist_envelope(
    event: &PersistPlaybookEvent,
) -> (&'static str, Option<String>, serde_json::Value) {
    match event {
        PersistPlaybookEvent::PlaybookPersisted {
            owner_home,
            kind,
            machine_yaml: _,
            hooks: _,
            exemplars: _,
        } => (
            "PlaybookPersisted",
            Some(kind.clone()),
            serde_json::json!({ "owner_home": owner_home, "kind": kind }),
        ),
    }
}

/// Generate an RFC 4122 version-4 (random) UUID string without a `uuid` dep —
/// anvil already depends on `rand`, and the contract only needs a fresh v4 id
/// string. Matches lore's "stay dependency-light, inline a conformant
/// generator" idiom.
fn new_uuid_v4() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    // version 4 (0100) in the high nibble of byte 6; RFC variant (10) in the
    // two high bits of byte 8.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::domain::shared_types::ActorIdentity;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    /// Serializes the env-mutating `from_env` tests — they set/remove process-wide
    /// vars (PUBLICATION_LOG_DIR_ENV / TEST_TREE_SIGNAL_ENV / RENDEZVOUS_DISABLE_ENV)
    /// that would race under cargo's default parallel test runner.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d =
            std::env::temp_dir().join(format!("anvil-publog-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn sample_actor() -> ActorIdentity {
        ActorIdentity {
            name: "Tester-000001".to_string(),
            actor_type: "agent".to_string(),
            model: "opus".to_string(),
            provider: "anthropic".to_string(),
            context_window: 200000,
            sdk_version: "1.0".to_string(),
            entrypoint: "cli".to_string(),
            registered_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn uuid_v4_has_the_canonical_shape_and_version_variant_bits() {
        let id = new_uuid_v4();
        assert_eq!(id.len(), 36);
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        // version nibble == 4
        assert_eq!(parts[2].chars().next().unwrap(), '4');
        // variant nibble in {8,9,a,b}
        assert!(matches!(
            parts[3].chars().next().unwrap(),
            '8' | '9' | 'a' | 'b'
        ));
    }

    #[test]
    fn mirror_writes_the_standard_dated_path_with_the_canonical_envelope() {
        let dir = temp_dir();
        let log = PublicationLog::new(&dir);
        let event = Event::ReviewTransition {
            track_path: "tracks/20260101T0000_demo".to_string(),
            to_state: "plan".to_string(),
            actor: sample_actor(),
            role: "doer".to_string(),
            approver: None,
            note: Some("looks good".to_string()),
        };
        log.mirror_begin(&event);

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let expected = dir.join(format!("events-{today}.jsonl"));
        assert!(
            expected.exists(),
            "standard dated file should exist: {expected:?}"
        );

        let content = fs::read_to_string(&expected).unwrap();
        let line = content.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        // canonical envelope shape
        assert_eq!(v["kind"], "ReviewTransition");
        assert_eq!(v["aggregateId"], "tracks/20260101T0000_demo");
        assert!(v["eventId"].as_str().unwrap().len() == 36);
        assert!(v["timestamp"].as_str().unwrap().contains('T'));
        // payload round-trips the variant fields
        assert_eq!(v["data"]["to_state"], "plan");
        assert_eq!(v["data"]["role"], "doer");
        assert_eq!(v["data"]["note"], "looks good");
        assert_eq!(v["data"]["actor"]["name"], "Tester-000001");
    }

    #[test]
    fn aggregate_id_is_omitted_when_a_variant_carries_no_correlation_key() {
        // Every current anvil variant DOES carry a correlation key, so assert
        // the omission behavior directly on the writer.
        let dir = temp_dir();
        let log = PublicationLog::new(&dir);
        log.mirror("BareEvent", None, serde_json::json!({"x": 1}));
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let content = fs::read_to_string(dir.join(format!("events-{today}.jsonl"))).unwrap();
        let v: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert!(
            v.get("aggregateId").is_none(),
            "no aggregateId key when None"
        );
        assert_eq!(v["kind"], "BareEvent");
    }

    #[test]
    fn rotation_uses_a_separate_file_per_utc_day() {
        let dir = temp_dir();
        let log = PublicationLog::new(&dir);
        let d1 = log.path_for_date("2026-06-22");
        let d2 = log.path_for_date("2026-06-23");
        assert_ne!(d1, d2);
        assert_eq!(d1.file_name().unwrap(), "events-2026-06-22.jsonl");
        assert_eq!(d2.file_name().unwrap(), "events-2026-06-23.jsonl");
    }

    #[test]
    fn mirror_is_best_effort_on_an_unwritable_dir() {
        // Point the dir at a path whose parent is a FILE, so create_dir_all
        // fails. mirror must swallow the error and not panic.
        let base = temp_dir();
        fs::create_dir_all(&base).unwrap();
        let file_as_parent = base.join("not-a-dir");
        fs::write(&file_as_parent, b"x").unwrap();
        let log = PublicationLog::new(file_as_parent.join("publication-log"));
        // Should not panic / not propagate.
        log.mirror("K", Some("agg"), serde_json::json!({}));
        // A second call exercises the warn-latch path too.
        log.mirror("K", Some("agg"), serde_json::json!({}));
    }

    #[test]
    fn from_env_is_inert_inside_a_test_tree_and_honors_an_explicit_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // explicit override wins regardless of test-tree signal
        std::env::set_var(PUBLICATION_LOG_DIR_ENV, "/tmp/anvil-publog-override");
        std::env::set_var(TEST_TREE_SIGNAL_ENV, "/tmp/some-temper-home");
        let log = PublicationLog::from_env().expect("override should activate");
        assert_eq!(log.dir(), PathBuf::from("/tmp/anvil-publog-override"));
        std::env::remove_var(PUBLICATION_LOG_DIR_ENV);

        // no override + test-tree signal present ⇒ inert
        assert!(
            PublicationLog::from_env().is_none(),
            "must be inert in a test tree"
        );
        std::env::remove_var(TEST_TREE_SIGNAL_ENV);
    }

    #[test]
    fn from_env_is_inert_when_rendezvous_is_disabled() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // An ephemeral/test engine (rendezvous disabled) must NOT write to the real
        // ~/.anvil/publication-log — that is the fixture-pollution leak.
        std::env::remove_var(PUBLICATION_LOG_DIR_ENV);
        std::env::remove_var(TEST_TREE_SIGNAL_ENV);
        std::env::set_var(RENDEZVOUS_DISABLE_ENV, "1");
        assert!(
            PublicationLog::from_env().is_none(),
            "must be inert when rendezvous is disabled (ephemeral/test engine)"
        );

        // ...but an explicit log-dir override still wins, so a test that ASSERTS on
        // the publication log keeps working even with rendezvous disabled.
        std::env::set_var(PUBLICATION_LOG_DIR_ENV, "/tmp/anvil-publog-rdv-override");
        let log = PublicationLog::from_env().expect("explicit override must win");
        assert_eq!(log.dir(), PathBuf::from("/tmp/anvil-publog-rdv-override"));
        std::env::remove_var(PUBLICATION_LOG_DIR_ENV);
        std::env::remove_var(RENDEZVOUS_DISABLE_ENV);
    }

    #[test]
    fn complete_amend_persist_variants_map_to_their_names() {
        let dir = temp_dir();
        let log = PublicationLog::new(&dir);
        log.mirror_complete(&CompleteEvent::ActorUpserted {
            artifact_path: "tracks/t".to_string(),
            identity: sample_actor(),
        });
        log.mirror_persist(&PersistPlaybookEvent::PlaybookPersisted {
            owner_home: "/home/x".to_string(),
            kind: "review".to_string(),
            machine_yaml: "kind: review".to_string(),
            hooks: Vec::new(),
            exemplars: Vec::new(),
        });
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let content = fs::read_to_string(dir.join(format!("events-{today}.jsonl"))).unwrap();
        let kinds: Vec<String> = content
            .lines()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(l).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(kinds, vec!["ActorUpserted", "PlaybookPersisted"]);
        // PlaybookPersisted correlates on kind, never leaks machine_yaml.
        let last: serde_json::Value =
            serde_json::from_str(content.lines().last().unwrap()).unwrap();
        assert_eq!(last["aggregateId"], "review");
        assert!(last["data"].get("machine_yaml").is_none());
    }
}
