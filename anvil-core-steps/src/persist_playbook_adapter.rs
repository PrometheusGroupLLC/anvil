//! Step module for `persist_generated_playbook_adapter.feature` (track 1a, BP1).
//!
//! Exercises `ArtifactPort::persist_generated_playbook` on both implementors:
//! `FileSystemArtifactAdapter` (writes under a given owner-home, ignoring its
//! own hearth_path, with NO status.yaml) and `InMemoryArtifactAdapter` (records
//! the call). Uses `tempfile::TempDir` for hermetic owner-homes and the shared
//! `minimal_machine_yaml` fixture — never reads a real machine.yaml.

use anvil_core::domain::playbook::candidate::GeneratedExemplarFile;
use anvil_core_hearth::fs_artifact_adapter::FileSystemArtifactAdapter;
use anvil_core_hearth::in_memory_artifact_adapter::{
    InMemoryArtifactAdapter, PersistedPlaybookRecord,
};
use anvil_core::ports::artifact_port::ArtifactPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const OWNER_HOME_KEY: &str = "pwa_owner_home";
const OWNER_HOME_HANDLE_KEY: &str = "pwa_owner_home_handle";
const OWNER_HOME_B_KEY: &str = "pwa_owner_home_b";
const OWNER_HOME_B_HANDLE_KEY: &str = "pwa_owner_home_b_handle";
const HEARTH_A_KEY: &str = "pwa_hearth_a";
const HEARTH_A_HANDLE_KEY: &str = "pwa_hearth_a_handle";
const WRITTEN_PATH_KEY: &str = "pwa_written_path";
const IN_MEMORY_RECORDS_KEY: &str = "pwa_in_memory_records";
const SEEDED_BYTES_KEY: &str = "pwa_seeded_bytes";
const FS_ATTEMPT_OK_KEY: &str = "pwa_fs_attempt_ok";
const FS_ATTEMPT_ERROR_KEY: &str = "pwa_fs_attempt_error";

type Handle = Arc<Mutex<Option<tempfile::TempDir>>>;

fn new_temp() -> Result<(PathBuf, Handle), String> {
    let dir = tempfile::TempDir::new().map_err(|e| format!("temp dir: {}", e))?;
    let path = dir.path().to_path_buf();
    Ok((path, Arc::new(Mutex::new(Some(dir)))))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== fs adapter: owner-home only =====
        step_def(
            "a temp directory used as an owner-home",
            &[],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (path, handle) = new_temp()?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, path);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the fs artifact adapter persists a playbook for kind {string} under that owner-home",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (WRITTEN_PATH_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                // The adapter's hearth_path is deliberately a DIFFERENT path —
                // the op must ignore it and write under the owner_home arg.
                let adapter = FileSystemArtifactAdapter::new(PathBuf::from("/nonexistent-hearth"));
                let written = adapter
                    .persist_generated_playbook(
                        &owner_home.to_string_lossy(),
                        &kind,
                        &anvil_test_support::minimal_machine_yaml(&kind),
                        None,
                        None,
                    )
                    .map_err(|e| format!("persist failed: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(WRITTEN_PATH_KEY, written);
                Ok(out)
            },
        ),
        step_def(
            "the fs artifact adapter persists a playbook for kind {string} with exemplar {string} under that owner-home",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (WRITTEN_PATH_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let exemplar_id = params
                    .get_string(1)
                    .ok_or("Expected exemplar id")?
                    .to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let adapter = FileSystemArtifactAdapter::new(PathBuf::from("/nonexistent-hearth"));
                let exemplar = GeneratedExemplarFile {
                    id: exemplar_id,
                    markdown: "---\nid: triage-good\nband: good\ndimensions:\n- correctness\nevidence_class: self_description\nprovenance:\n  source: internal\n  corpus: fixture\nplaybook_version: fixture-version\nrefreshed_at: 2026-07-04T00:00:00Z\n---\nA distilled good triage pattern.\n".to_string(),
                };
                let written = adapter
                    .persist_generated_playbook(
                        &owner_home.to_string_lossy(),
                        &kind,
                        &anvil_test_support::minimal_machine_yaml(&kind),
                        None,
                        Some(std::slice::from_ref(&exemplar)),
                    )
                    .map_err(|e| format!("persist failed: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(WRITTEN_PATH_KEY, written);
                Ok(out)
            },
        ),
        step_def(
            "that owner-home already has different machine.yaml content for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let kind_dir = owner_home.join("playbooks").join(&kind);
                std::fs::create_dir_all(&kind_dir)
                    .map_err(|e| format!("create kind dir: {}", e))?;
                let bytes = anvil_test_support::different_minimal_machine_yaml(&kind).into_bytes();
                std::fs::write(kind_dir.join("machine.yaml"), &bytes)
                    .map_err(|e| format!("write seed machine: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(SEEDED_BYTES_KEY, bytes);
                Ok(out)
            },
        ),
        step_def(
            "that owner-home already has identical machine.yaml content for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let kind_dir = owner_home.join("playbooks").join(&kind);
                std::fs::create_dir_all(&kind_dir)
                    .map_err(|e| format!("create kind dir: {}", e))?;
                let bytes = anvil_test_support::minimal_machine_yaml(&kind).into_bytes();
                std::fs::write(kind_dir.join("machine.yaml"), &bytes)
                    .map_err(|e| format!("write seed machine: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(SEEDED_BYTES_KEY, bytes);
                Ok(out)
            },
        ),
        step_def(
            "that owner-home already has byte-different hook-bearing machine.yaml content for kind {string}",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                // Seed a hook-BEARING machine (references intent.md) and write the
                // sibling hooks/intent.md. The write-boundary race reload must list
                // that hook and load the seed cleanly, reporting a genuine
                // same-kind duplicate — NOT `playbook_existing_machine_invalid`
                // from reloading with an empty hook list.
                let kind_dir = owner_home.join("playbooks").join(&kind);
                std::fs::create_dir_all(kind_dir.join("hooks"))
                    .map_err(|e| format!("create kind dir: {}", e))?;
                let bytes = anvil_test_support::different_conformant_machine_yaml(&kind).into_bytes();
                std::fs::write(kind_dir.join("machine.yaml"), &bytes)
                    .map_err(|e| format!("write seed machine: {}", e))?;
                std::fs::write(
                    kind_dir.join("hooks").join("intent.md"),
                    "Hook body for intent.md",
                )
                .map_err(|e| format!("write seed hook: {}", e))?;
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(SEEDED_BYTES_KEY, bytes);
                Ok(out)
            },
        ),
        step_def(
            "the fs artifact adapter attempts to persist a playbook for kind {string} under that owner-home",
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
            ],
            &[
                (OWNER_HOME_KEY, "PathBuf"),
                (OWNER_HOME_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (SEEDED_BYTES_KEY, "Vec<u8>"),
                (WRITTEN_PATH_KEY, "String"),
                (FS_ATTEMPT_OK_KEY, "bool"),
                (FS_ATTEMPT_ERROR_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let owner_home = ctx
                    .get::<PathBuf>(OWNER_HOME_KEY)
                    .ok_or("No owner_home")?
                    .clone();
                let handle = ctx
                    .get::<Handle>(OWNER_HOME_HANDLE_KEY)
                    .ok_or("No owner_home handle")?
                    .clone();
                let seeded = ctx
                    .get::<Vec<u8>>(SEEDED_BYTES_KEY)
                    .ok_or("No seeded bytes")?
                    .clone();
                let adapter = FileSystemArtifactAdapter::new(PathBuf::from("/nonexistent-hearth"));
                let result = adapter.persist_generated_playbook(
                    &owner_home.to_string_lossy(),
                        &kind,
                        &anvil_test_support::minimal_machine_yaml(&kind),
                        None,
                        None,
                );
                let mut out = Context::new();
                out.set(OWNER_HOME_KEY, owner_home);
                out.set(OWNER_HOME_HANDLE_KEY, handle);
                out.set(SEEDED_BYTES_KEY, seeded);
                match result {
                    Ok(path) => {
                        out.set(WRITTEN_PATH_KEY, path);
                        out.set(FS_ATTEMPT_OK_KEY, true);
                        out.set(FS_ATTEMPT_ERROR_KEY, String::new());
                    }
                    Err(e) => {
                        out.set(WRITTEN_PATH_KEY, String::new());
                        out.set(FS_ATTEMPT_OK_KEY, false);
                        out.set(FS_ATTEMPT_ERROR_KEY, e.to_string());
                    }
                }
                Ok(out)
            },
        ),
        check_def(
            "a machine.yaml exists at {string} under the owner-home",
            &[(OWNER_HOME_KEY, "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let path = owner_home.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        check_def(
            "the fs artifact adapter succeeds",
            &[(FS_ATTEMPT_OK_KEY, "bool"), (FS_ATTEMPT_ERROR_KEY, "String")],
            |ctx, _params| {
                let ok = ctx.get::<bool>(FS_ATTEMPT_OK_KEY).ok_or("No fs attempt result")?;
                if *ok {
                    Ok(())
                } else {
                    let err = ctx
                        .get::<String>(FS_ATTEMPT_ERROR_KEY)
                        .map(String::as_str)
                        .unwrap_or("<unknown>");
                    Err(format!("Expected fs adapter success, got error: {}", err))
                }
            },
        ),
        check_def(
            "the fs artifact adapter returns an error containing {string}",
            &[(FS_ATTEMPT_OK_KEY, "bool"), (FS_ATTEMPT_ERROR_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected message fragment")?;
                let ok = ctx.get::<bool>(FS_ATTEMPT_OK_KEY).ok_or("No fs attempt result")?;
                let err = ctx
                    .get::<String>(FS_ATTEMPT_ERROR_KEY)
                    .ok_or("No fs attempt error")?;
                if !*ok && err.contains(expected.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected error containing '{}', got ok={} error='{}'",
                        expected, ok, err
                    ))
                }
            },
        ),
        check_def(
            "the existing machine.yaml bytes for kind {string} are unchanged",
            &[(OWNER_HOME_KEY, "PathBuf"), (SEEDED_BYTES_KEY, "Vec<u8>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let expected = ctx.get::<Vec<u8>>(SEEDED_BYTES_KEY).ok_or("No seeded bytes")?;
                let path = owner_home.join("playbooks").join(kind).join("machine.yaml");
                let actual =
                    std::fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
                if &actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} bytes to remain unchanged ({} bytes), got {} bytes",
                        path.display(),
                        expected.len(),
                        actual.len()
                    ))
                }
            },
        ),
        check_def(
            "that machine.yaml contains {string}",
            &[(WRITTEN_PATH_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let written = ctx.get::<String>(WRITTEN_PATH_KEY).ok_or("No written path")?;
                let body = std::fs::read_to_string(written)
                    .map_err(|e| format!("read written: {}", e))?;
                if body.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("machine.yaml does not contain '{}'", needle))
                }
            },
        ),
        check_def(
            "no status.yaml exists alongside the persisted machine.yaml",
            &[(WRITTEN_PATH_KEY, "String")],
            |ctx, _params| {
                let written = ctx.get::<String>(WRITTEN_PATH_KEY).ok_or("No written path")?;
                let status = PathBuf::from(written)
                    .parent()
                    .ok_or("no parent")?
                    .join("status.yaml");
                if status.exists() {
                    Err(format!("status.yaml unexpectedly present at {}", status.display()))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "no exemplars directory exists alongside the persisted machine.yaml",
            &[(WRITTEN_PATH_KEY, "String")],
            |ctx, _params| {
                let written = ctx.get::<String>(WRITTEN_PATH_KEY).ok_or("No written path")?;
                let exemplars = PathBuf::from(written)
                    .parent()
                    .ok_or("no parent")?
                    .join("exemplars");
                if exemplars.exists() {
                    Err(format!(
                        "exemplars directory unexpectedly present at {}",
                        exemplars.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "an exemplar markdown exists at {string} under the owner-home",
            &[(OWNER_HOME_KEY, "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let path = owner_home.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        check_def(
            "that exemplar markdown contains {string}",
            &[(OWNER_HOME_KEY, "PathBuf")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let owner_home = ctx.get::<PathBuf>(OWNER_HOME_KEY).ok_or("No owner_home")?;
                let path = owner_home
                    .join("playbooks")
                    .join("throwaway_kind")
                    .join("exemplars")
                    .join("triage-good.md");
                let body =
                    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
                if body.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("exemplar markdown does not contain '{}'", needle))
                }
            },
        ),
        // ===== fs adapter: ignores hearth_path (A vs B) =====
        step_def(
            "a fs artifact adapter constructed with a hearth_path of temp dir A",
            &[],
            &[
                (HEARTH_A_KEY, "PathBuf"),
                (HEARTH_A_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (path, handle) = new_temp()?;
                let mut out = Context::new();
                out.set(HEARTH_A_KEY, path);
                out.set(HEARTH_A_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a separate temp dir B used as an owner-home",
            &[
                (HEARTH_A_KEY, "PathBuf"),
                (HEARTH_A_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (HEARTH_A_KEY, "PathBuf"),
                (HEARTH_A_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (OWNER_HOME_B_KEY, "PathBuf"),
                (OWNER_HOME_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth_a = ctx.get::<PathBuf>(HEARTH_A_KEY).ok_or("No hearth A")?.clone();
                let handle_a = ctx
                    .get::<Handle>(HEARTH_A_HANDLE_KEY)
                    .ok_or("No hearth A handle")?
                    .clone();
                let (path_b, handle_b) = new_temp()?;
                let mut out = Context::new();
                out.set(HEARTH_A_KEY, hearth_a);
                out.set(HEARTH_A_HANDLE_KEY, handle_a);
                out.set(OWNER_HOME_B_KEY, path_b);
                out.set(OWNER_HOME_B_HANDLE_KEY, handle_b);
                Ok(out)
            },
        ),
        step_def(
            "the fs artifact adapter persists a playbook for kind {string} under owner-home B",
            &[
                (HEARTH_A_KEY, "PathBuf"),
                (HEARTH_A_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (OWNER_HOME_B_KEY, "PathBuf"),
                (OWNER_HOME_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (HEARTH_A_KEY, "PathBuf"),
                (HEARTH_A_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (OWNER_HOME_B_KEY, "PathBuf"),
                (OWNER_HOME_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (WRITTEN_PATH_KEY, "String"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth_a = ctx.get::<PathBuf>(HEARTH_A_KEY).ok_or("No hearth A")?.clone();
                let handle_a = ctx
                    .get::<Handle>(HEARTH_A_HANDLE_KEY)
                    .ok_or("No hearth A handle")?
                    .clone();
                let owner_b = ctx
                    .get::<PathBuf>(OWNER_HOME_B_KEY)
                    .ok_or("No owner B")?
                    .clone();
                let handle_b = ctx
                    .get::<Handle>(OWNER_HOME_B_HANDLE_KEY)
                    .ok_or("No owner B handle")?
                    .clone();
                // Adapter constructed with hearth_path = A; write target = B.
                let adapter = FileSystemArtifactAdapter::new(hearth_a.clone());
                let written = adapter
                    .persist_generated_playbook(
                        &owner_b.to_string_lossy(),
                        &kind,
                        &anvil_test_support::minimal_machine_yaml(&kind),
                        None,
                        None,
                    )
                    .map_err(|e| format!("persist failed: {}", e))?;
                let mut out = Context::new();
                out.set(HEARTH_A_KEY, hearth_a);
                out.set(HEARTH_A_HANDLE_KEY, handle_a);
                out.set(OWNER_HOME_B_KEY, owner_b);
                out.set(OWNER_HOME_B_HANDLE_KEY, handle_b);
                out.set(WRITTEN_PATH_KEY, written);
                Ok(out)
            },
        ),
        check_def(
            "a machine.yaml exists at {string} under owner-home B",
            &[(OWNER_HOME_B_KEY, "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected rel path")?;
                let owner_b = ctx.get::<PathBuf>(OWNER_HOME_B_KEY).ok_or("No owner B")?;
                let path = owner_b.join(rel);
                if path.is_file() {
                    Ok(())
                } else {
                    Err(format!("Expected file at {}", path.display()))
                }
            },
        ),
        check_def(
            "owner-home A has no playbooks directory",
            &[(HEARTH_A_KEY, "PathBuf")],
            |ctx, _params| {
                let hearth_a = ctx.get::<PathBuf>(HEARTH_A_KEY).ok_or("No hearth A")?;
                let playbooks = hearth_a.join("playbooks");
                if playbooks.exists() {
                    Err(format!(
                        "hearth A unexpectedly has a playbooks dir at {}",
                        playbooks.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== in-memory adapter records the call =====
        step_def(
            "an in-memory artifact adapter",
            &[],
            &[(IN_MEMORY_RECORDS_KEY, "Vec<PersistedPlaybookRecord>")],
            |_ctx, _params| {
                // Nothing to seed; the next step constructs the adapter, calls it,
                // and captures the records (adapter isn't Context-storable simply).
                let mut out = Context::new();
                out.set(IN_MEMORY_RECORDS_KEY, Vec::<PersistedPlaybookRecord>::new());
                Ok(out)
            },
        ),
        step_def(
            "the in-memory adapter persists a playbook for owner-home {string} and kind {string}",
            &[(IN_MEMORY_RECORDS_KEY, "Vec<PersistedPlaybookRecord>")],
            &[(IN_MEMORY_RECORDS_KEY, "Vec<PersistedPlaybookRecord>")],
            |_ctx, params| {
                let owner_home = params.get_string(0).ok_or("Expected owner_home")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let adapter = InMemoryArtifactAdapter::new();
                adapter
                    .persist_generated_playbook(
                        &owner_home,
                        &kind,
                        &anvil_test_support::minimal_machine_yaml(&kind),
                        None,
                        None,
                    )
                    .map_err(|e| format!("persist failed: {}", e))?;
                let records = adapter.persisted_playbooks.lock().unwrap().clone();
                let mut out = Context::new();
                out.set(IN_MEMORY_RECORDS_KEY, records);
                Ok(out)
            },
        ),
        check_def(
            "the in-memory adapter recorded {int} persisted playbook",
            &[(IN_MEMORY_RECORDS_KEY, "Vec<PersistedPlaybookRecord>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let records = ctx
                    .get::<Vec<PersistedPlaybookRecord>>(IN_MEMORY_RECORDS_KEY)
                    .ok_or("No records")?;
                if records.len() == expected {
                    Ok(())
                } else {
                    Err(format!("Expected {} records, got {}", expected, records.len()))
                }
            },
        ),
        check_def(
            "the recorded persisted playbook has owner-home {string} and kind {string}",
            &[(IN_MEMORY_RECORDS_KEY, "Vec<PersistedPlaybookRecord>")],
            |ctx, params| {
                let owner_home = params.get_string(0).ok_or("Expected owner_home")?;
                let kind = params.get_string(1).ok_or("Expected kind")?;
                let records = ctx
                    .get::<Vec<PersistedPlaybookRecord>>(IN_MEMORY_RECORDS_KEY)
                    .ok_or("No records")?;
                let rec = records.first().ok_or("No recorded playbook")?;
                if rec.owner_home == owner_home && rec.kind == kind {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected owner_home '{}' kind '{}', got owner_home '{}' kind '{}'",
                        owner_home, kind, rec.owner_home, rec.kind
                    ))
                }
            },
        ),
    ]
}
