//! Fs-backed describe step definitions (F1).
//!
//! Every existing describe step drives the synthetic `TestDescribeAdapter`;
//! a repo-wide search finds zero references to `FileSystemDescribeAdapter` in
//! tests/features. These steps seed a real on-disk hearth and drive the REAL
//! `FileSystemDescribeAdapter::read_instance` so the describe read site
//! (`fs_describe_adapter.rs` state computation) is genuinely exercised.
//!
//! `FileSystemDescribeAdapter` scans `ALL_ARTIFACT_TYPES` joining
//! `<directory_name>/<id>/status.yaml`, so the seeding step writes verbatim
//! content under the path given (which must include the `<type-dir>/<id>/`
//! prefix and `status.yaml`), and the invocation step is called with the bare
//! artifact id.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::describe::DescribeError;
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::playbook::types::PlaybookMachine;
use anvil_core_hearth::fs_describe_adapter::FileSystemDescribeAdapter;
use anvil_core::ports::describe_port::{DescribePort, InstanceState};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

type DescribeReadOutcome = Result<InstanceState, DescribeError>;

/// A single-machine `PlaybookRegistry` whose lone machine declares an arbitrary
/// `directory:` and `kind:`. Used to drive `FileSystemDescribeAdapter::with_registry`
/// with a HOSTILE directory declaration (`..`, an absolute path) and prove the
/// adapter's trust boundary refuses to scan it — the path-traversal defense.
struct HostileDirRegistry {
    machine: PlaybookMachine,
}

impl HostileDirRegistry {
    fn new(kind: &str, directory: &str) -> Self {
        let machine = PlaybookMachine {
            kind: kind.to_string(),
            directory: directory.to_string(),
            registry: format!("{}.md", kind),
            description: "Hostile-directory fixture for describe traversal defense".to_string(),
            ..Default::default()
        };
        HostileDirRegistry { machine }
    }
}

impl PlaybookRegistry for HostileDirRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        (kind == self.machine.kind).then_some(&self.machine)
    }

    fn playbook_id_for(&self, _kind: &str) -> Option<String> {
        None
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        vec![&self.machine]
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a describe fs hearth with:",
            &[],
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = retained_temp_dir("anvil-describe-fs-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].clone(), table.headers[1].clone()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].clone(), row[1].clone()));
                    }
                }
                for (path, content) in pairs {
                    let full = tmp.join(path.trim());
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create dir: {}", e))?;
                    }
                    let content = content.replace("\\n", "\n");
                    std::fs::write(&full, content)
                        .map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
                }
                let mut out = Context::new();
                out.set("describe_fs_hearth", tmp);
                out.set("describe_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "describe fs read_instance is called for {string}",
            &[("describe_fs_hearth", "PathBuf")],
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("describe_read_result", "DescribeReadOutcome"),
            ],
            |ctx, params| {
                let artifact_id = params
                    .get_string(0)
                    .ok_or("Expected artifact_id")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("describe_fs_hearth")
                    .ok_or("No describe_fs_hearth")?
                    .clone();
                let adapter = FileSystemDescribeAdapter::new(hearth.clone());
                let result = adapter.read_instance(&artifact_id);
                let mut out = Context::new();
                out.set("describe_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "describe_fs_hearth_handle");
                out.set("describe_read_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the describe fs instance state is {string}",
            &[("describe_read_result", "DescribeReadOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let result = ctx
                    .get::<DescribeReadOutcome>("describe_read_result")
                    .ok_or("No describe_read_result")?;
                match result {
                    Ok(inst) if inst.state == expected.as_ref() as &str => Ok(()),
                    Ok(inst) => Err(format!(
                        "Expected describe instance state '{}', got '{}'",
                        expected, inst.state
                    )),
                    Err(e) => Err(format!("Expected success, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "the describe fs instance kind is {string}",
            &[("describe_read_result", "DescribeReadOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected kind")?;
                let result = ctx
                    .get::<DescribeReadOutcome>("describe_read_result")
                    .ok_or("No describe_read_result")?;
                match result {
                    Ok(inst) if inst.kind == expected.as_ref() as &str => Ok(()),
                    Ok(inst) => Err(format!(
                        "Expected describe instance kind '{}', got '{}'",
                        expected, inst.kind
                    )),
                    Err(e) => Err(format!("Expected success, got error: {:?}", e)),
                }
            },
        ),
        check_def(
            "the describe fs read is an UnknownIdentifier error",
            &[("describe_read_result", "DescribeReadOutcome")],
            |ctx, _params| {
                let result = ctx
                    .get::<DescribeReadOutcome>("describe_read_result")
                    .ok_or("No describe_read_result")?;
                match result {
                    Err(DescribeError::UnknownIdentifier { .. }) => Ok(()),
                    Err(other) => Err(format!(
                        "Expected UnknownIdentifier error, got {:?}",
                        other
                    )),
                    Ok(inst) => Err(format!(
                        "Expected UnknownIdentifier error, got instance kind '{}' state '{}'",
                        inst.kind, inst.state
                    )),
                }
            },
        ),
        // ===== Path-traversal defense fixture (with_registry trust boundary) =====
        // Lays out one retained temp `root` holding two SIBLING dirs:
        //   root/hearth/proposals/<benign>/status.yaml  (a normal in-hearth instance)
        //   root/secret/<secret>/status.yaml            (a file OUTSIDE the hearth)
        // The describe hearth is `root/hearth`; the secret dir `root/secret` is
        // reachable from it only via `../secret` or its absolute path — exactly the
        // shapes a hostile machine `directory:` would use to escape.
        step_def(
            "a describe fs sandbox with a benign hearth entry {string} and an out-of-hearth secret instance {string} in state {string}",
            &[],
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_secret_abs", "PathBuf"),
                ("describe_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let benign_id = params.get_string(0).ok_or("Expected benign id")?.to_string();
                let secret_id = params.get_string(1).ok_or("Expected secret id")?.to_string();
                let secret_state = params.get_string(2).ok_or("Expected secret state")?.to_string();

                let (handle, root) = retained_temp_dir("anvil-describe-fs-traversal-")?;

                let hearth = root.join("hearth");
                let benign_dir = hearth.join("proposals").join(&benign_id);
                std::fs::create_dir_all(&benign_dir)
                    .map_err(|e| format!("Failed to create benign dir: {}", e))?;
                std::fs::write(
                    benign_dir.join("status.yaml"),
                    "version: 1\nkind: proposal\ntransitions:\n  - to: draft\n",
                )
                .map_err(|e| format!("Failed to write benign status.yaml: {}", e))?;

                let secret_dir = root.join("secret");
                let secret_instance = secret_dir.join(&secret_id);
                std::fs::create_dir_all(&secret_instance)
                    .map_err(|e| format!("Failed to create secret dir: {}", e))?;
                std::fs::write(
                    secret_instance.join("status.yaml"),
                    format!(
                        "version: 1\nkind: exfiltrated\ntransitions:\n  - to: {}\n",
                        secret_state
                    ),
                )
                .map_err(|e| format!("Failed to write secret status.yaml: {}", e))?;

                let mut out = Context::new();
                out.set("describe_fs_hearth", hearth);
                out.set("describe_fs_secret_abs", secret_dir);
                out.set("describe_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        // When: build the adapter with a registry whose lone machine declares the
        // secret dir by ABSOLUTE path, then read the secret id through it.
        step_def(
            "describe fs read_instance via a registry declaring the secret dir by absolute path for kind {string} is called for {string}",
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_secret_abs", "PathBuf"),
            ],
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("describe_read_result", "DescribeReadOutcome"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let artifact_id = params.get_string(1).ok_or("Expected artifact_id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("describe_fs_hearth")
                    .ok_or("No describe_fs_hearth")?
                    .clone();
                let secret_abs = ctx
                    .get::<PathBuf>("describe_fs_secret_abs")
                    .ok_or("No describe_fs_secret_abs")?
                    .clone();
                let registry =
                    HostileDirRegistry::new(&kind, &secret_abs.to_string_lossy());
                let adapter = FileSystemDescribeAdapter::with_registry(hearth.clone(), &registry);
                let result = adapter.read_instance(&artifact_id);
                let mut out = Context::new();
                out.set("describe_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "describe_fs_hearth_handle");
                out.set("describe_read_result", result);
                Ok(out)
            },
        ),
        // When: build the adapter with a registry whose lone machine declares the
        // secret dir by a RELATIVE `..` escape, then read the secret id through it.
        step_def(
            "describe fs read_instance via a registry declaring directory {string} for kind {string} is called for {string}",
            &[("describe_fs_hearth", "PathBuf")],
            &[
                ("describe_fs_hearth", "PathBuf"),
                ("describe_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("describe_read_result", "DescribeReadOutcome"),
            ],
            |ctx, params| {
                let directory = params.get_string(0).ok_or("Expected directory")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let artifact_id = params.get_string(2).ok_or("Expected artifact_id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("describe_fs_hearth")
                    .ok_or("No describe_fs_hearth")?
                    .clone();
                let registry = HostileDirRegistry::new(&kind, &directory);
                let adapter = FileSystemDescribeAdapter::with_registry(hearth.clone(), &registry);
                let result = adapter.read_instance(&artifact_id);
                let mut out = Context::new();
                out.set("describe_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "describe_fs_hearth_handle");
                out.set("describe_read_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the describe fs instance last_transition to is {string}",
            &[("describe_read_result", "DescribeReadOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected to")?;
                let result = ctx
                    .get::<DescribeReadOutcome>("describe_read_result")
                    .ok_or("No describe_read_result")?;
                match result {
                    Ok(inst) => match &inst.last_transition {
                        Some(t) if t.to == expected.as_ref() as &str => Ok(()),
                        Some(t) => Err(format!(
                            "Expected last_transition.to '{}', got '{}'",
                            expected, t.to
                        )),
                        None => Err(format!(
                            "Expected last_transition.to '{}', got None",
                            expected
                        )),
                    },
                    Err(e) => Err(format!("Expected success, got error: {:?}", e)),
                }
            },
        ),
    ]
}
