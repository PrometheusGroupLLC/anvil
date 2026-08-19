//! Step module for `registry_enumeration.feature` and the BP0 enumeration
//! scenarios in `composite_playbook_registry.feature` (playbook_routing_layer).
//!
//! Provides steps for:
//! - Building an ordered fake hearth/seed registry from a kinds list and
//!   enumerating the composite (proving hearth-first de-dup + ordering).
//! - Enumerating a real `HearthPlaybookRegistry` built from a temp hearth on
//!   disk (proving malformed machines are absent — AC9).
//! - Enumerating the `SeedPlaybookRegistry`.

use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::{
    PlaybookMachine, RouteConfig, StateDefinition, TransitionDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ─── Context keys ───────────────────────────────────────────────────────────

/// Ordered list of (kind, slot) for the hearth fake (slot is always "hearth").
const HEARTH_KINDS_KEY: &str = "renum_hearth_kinds";
/// Ordered list of kinds for the seed fake.
const SEED_KINDS_KEY: &str = "renum_seed_kinds";
/// Composite-enumerated kinds in order.
const ENUM_KINDS_KEY: &str = "renum_enum_kinds";
/// The slot ("hearth"|"seed") that provided the de-dup-winning machine for a kind.
const ENUM_TRACK_SLOT_KEY: &str = "renum_enum_track_slot";
/// Temp hearth path + handle for disk-backed enumeration.
const TEMP_HEARTH_PATH_KEY: &str = "renum_temp_hearth_path";
const TEMP_HEARTH_HANDLE_KEY: &str = "renum_temp_hearth_handle";
/// Hearth-registry-enumerated kinds.
const HEARTH_ENUM_KINDS_KEY: &str = "renum_hearth_enum_kinds";
/// Seed-registry-enumerated kinds.
const SEED_ENUM_KINDS_KEY: &str = "renum_seed_enum_kinds";

// ─── Test double: ordered fake ──────────────────────────────────────────────

/// A `PlaybookRegistry` whose `all_machines` enumeration is insertion-ordered
/// and whose machines carry a `slot` marker (encoded into `directory`) so the
/// composite de-dup winner is observable.
struct OrderedFakeRegistry {
    order: Vec<String>,
    map: HashMap<String, PlaybookMachine>,
}

impl OrderedFakeRegistry {
    fn from_kinds(slot: &str, kinds: &[String]) -> Self {
        let mut order = Vec::new();
        let mut map = HashMap::new();
        for kind in kinds {
            order.push(kind.clone());
            map.insert(kind.clone(), fake_machine(kind, slot));
        }
        Self { order, map }
    }
}

impl PlaybookRegistry for OrderedFakeRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.map.get(kind).map(|_| format!("{}_playbook", kind))
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.order.iter().filter_map(|k| self.map.get(k)).collect()
    }
}

/// A minimal `PlaybookMachine`; `directory` encodes the slot ("hearth"|"seed")
/// so de-dup precedence is observable.
fn fake_machine(kind: &str, slot: &str) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        directory: slot.to_string(),
        registry: format!("{}s.md", kind),
        parent_kind: None,
        description: format!("Fake {} machine ({} slot).", kind, slot),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string(), "reviewer".to_string()],
        states: vec![StateDefinition {
            name: "active".to_string(),
            role_filters: vec![],
            registry_section: "active".to_string(),
            projection_targets: vec![],
            is_review_gate: false,
            is_terminal: false,
            hook: None,
            hooks_by_role: std::collections::BTreeMap::new(),
            measurement_by_role: std::collections::BTreeMap::new(),
        }],
        transitions: vec![TransitionDefinition {
            from_state: "active".to_string(),
            to_state: "completed".to_string(),
            required_role: "doer".to_string(),
            required_satisfaction: None,
            requires_approver: false,
            hook: None,
        }],
        register: anvil_core::domain::playbook::types::Register::Driven,
        ..Default::default()
    }
}

fn parse_kinds_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn minimal_machine_yaml(kind: &str) -> String {
    format!(
        r#"kind: {kind}
directory: {kind}s
registry: {kind}s.md
description: "Minimal {kind} machine for testing."
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind
    )
}

fn ensure_temp_hearth(
    ctx: &Context,
) -> Result<(PathBuf, Arc<Mutex<Option<tempfile::TempDir>>>), String> {
    if let (Some(path), Some(handle)) = (
        ctx.get::<PathBuf>(TEMP_HEARTH_PATH_KEY),
        ctx.get::<Arc<Mutex<Option<tempfile::TempDir>>>>(TEMP_HEARTH_HANDLE_KEY),
    ) {
        return Ok((path.clone(), handle.clone()));
    }
    let temp_dir =
        tempfile::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let path = temp_dir.path().to_path_buf();
    let handle: Arc<Mutex<Option<tempfile::TempDir>>> = Arc::new(Mutex::new(Some(temp_dir)));
    Ok((path, handle))
}

fn write_machine_dir(hearth: &PathBuf, dir: &str, contents: &str) -> Result<(), String> {
    let machine_path = hearth.join("playbooks").join(dir).join("machine.yaml");
    if let Some(parent) = machine_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dirs: {}", e))?;
    }
    std::fs::write(&machine_path, contents).map_err(|e| format!("Failed to write: {}", e))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== composite enumeration (ordered fakes) =====
        step_def(
            "a fake hearth registry with kinds {string}",
            &[],
            &[(HEARTH_KINDS_KEY, "Vec<String>")],
            |_ctx, params| {
                let kinds =
                    parse_kinds_list(params.get_string(0).ok_or("Expected kinds")?.as_ref());
                let mut out = Context::new();
                out.set(HEARTH_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        step_def(
            "a fake seed registry with kinds {string}",
            &[(HEARTH_KINDS_KEY, "Vec<String>")],
            &[
                (HEARTH_KINDS_KEY, "Vec<String>"),
                (SEED_KINDS_KEY, "Vec<String>"),
            ],
            |ctx, params| {
                let kinds =
                    parse_kinds_list(params.get_string(0).ok_or("Expected kinds")?.as_ref());
                let hearth = ctx
                    .get::<Vec<String>>(HEARTH_KINDS_KEY)
                    .ok_or("No hearth kinds")?
                    .clone();
                let mut out = Context::new();
                out.set(HEARTH_KINDS_KEY, hearth);
                out.set(SEED_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        step_def(
            "the composite registry enumerates all kinds",
            &[
                (HEARTH_KINDS_KEY, "Vec<String>"),
                (SEED_KINDS_KEY, "Vec<String>"),
            ],
            &[
                (ENUM_KINDS_KEY, "Vec<String>"),
                (ENUM_TRACK_SLOT_KEY, "HashMap<String,String>"),
            ],
            |ctx, _params| {
                let hearth_kinds = ctx
                    .get::<Vec<String>>(HEARTH_KINDS_KEY)
                    .ok_or("No hearth kinds")?
                    .clone();
                let seed_kinds = ctx
                    .get::<Vec<String>>(SEED_KINDS_KEY)
                    .ok_or("No seed kinds")?
                    .clone();
                let hearth = OrderedFakeRegistry::from_kinds("hearth", &hearth_kinds);
                let seed = OrderedFakeRegistry::from_kinds("seed", &seed_kinds);
                let composite = CompositePlaybookRegistry::new(hearth, seed);

                let machines = composite.all_machines();
                let kinds: Vec<String> = machines.iter().map(|m| m.kind.clone()).collect();
                // slot is encoded in `directory`.
                let slot_by_kind: HashMap<String, String> = machines
                    .iter()
                    .map(|m| (m.kind.clone(), m.directory.clone()))
                    .collect();

                let mut out = Context::new();
                out.set(ENUM_KINDS_KEY, kinds);
                out.set(ENUM_TRACK_SLOT_KEY, slot_by_kind);
                Ok(out)
            },
        ),
        check_def(
            "the enumerated kinds are {string}",
            &[(ENUM_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let expected =
                    parse_kinds_list(params.get_string(0).ok_or("Expected kinds")?.as_ref());
                let actual = ctx
                    .get::<Vec<String>>(ENUM_KINDS_KEY)
                    .ok_or("No enumerated kinds")?;
                if *actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected kinds {:?} but got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the enumerated machine for kind {string} came from the hearth",
            &[(ENUM_TRACK_SLOT_KEY, "HashMap<String,String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let slots = ctx
                    .get::<HashMap<String, String>>(ENUM_TRACK_SLOT_KEY)
                    .ok_or("No slot map")?;
                match slots.get(&kind).map(String::as_str) {
                    Some("hearth") => Ok(()),
                    Some(other) => Err(format!(
                        "Expected kind '{}' from hearth but it came from '{}'",
                        kind, other
                    )),
                    None => Err(format!("Kind '{}' not in enumeration", kind)),
                }
            },
        ),
        // ===== hearth-registry enumeration (real disk) =====
        step_def(
            "a hearth with a valid {string} machine on disk",
            &[],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (hearth, handle) = ensure_temp_hearth(&ctx)?;
                write_machine_dir(
                    &hearth,
                    &format!("{}_dir", kind),
                    &minimal_machine_yaml(&kind),
                )?;
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the hearth also has a valid {string} machine on disk",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let (hearth, handle) = ensure_temp_hearth(&ctx)?;
                let contents = if kind == "knowledge_lifecycle" {
                    anvil_test_support::query_port::knowledge_lifecycle_machine_yaml().to_string()
                } else {
                    minimal_machine_yaml(&kind)
                };
                write_machine_dir(&hearth, &format!("{}_dir", kind), &contents)?;
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the hearth also has a malformed machine.yaml on disk",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let (hearth, handle) = ensure_temp_hearth(&ctx)?;
                write_machine_dir(&hearth, "broken_dir", "this: is: not: valid: [[[")?;
                let mut out = Context::new();
                out.set(TEMP_HEARTH_PATH_KEY, hearth);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "I enumerate the hearth registry kinds",
            &[
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (HEARTH_ENUM_KINDS_KEY, "Vec<String>"),
                (TEMP_HEARTH_PATH_KEY, "PathBuf"),
                (TEMP_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>(TEMP_HEARTH_PATH_KEY)
                    .ok_or("No temp hearth")?
                    .clone();
                let handle = ctx
                    .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(TEMP_HEARTH_HANDLE_KEY)
                    .ok_or("No temp hearth handle")?
                    .clone();
                let registry = HearthPlaybookRegistry::new(hearth.clone());
                let kinds = registry.kinds();
                let mut out = Context::new();
                out.set(HEARTH_ENUM_KINDS_KEY, kinds);
                out.set(TEMP_HEARTH_PATH_KEY, hearth);
                out.set(TEMP_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the enumerated kinds include {string}",
            &[(HEARTH_ENUM_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let kinds = ctx
                    .get::<Vec<String>>(HEARTH_ENUM_KINDS_KEY)
                    .ok_or("No enumerated kinds")?;
                if kinds.contains(&kind) {
                    Ok(())
                } else {
                    Err(format!("Expected kind '{}' in {:?}", kind, kinds))
                }
            },
        ),
        check_def(
            "the enumerated kinds do not include {string}",
            &[(HEARTH_ENUM_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let kinds = ctx
                    .get::<Vec<String>>(HEARTH_ENUM_KINDS_KEY)
                    .ok_or("No enumerated kinds")?;
                if kinds.contains(&kind) {
                    Err(format!(
                        "Expected kind '{}' absent but found in {:?}",
                        kind, kinds
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== seed-registry enumeration =====
        step_def(
            "I enumerate the seed registry kinds",
            &[],
            &[(SEED_ENUM_KINDS_KEY, "Vec<String>")],
            |_ctx, _params| {
                let registry = SeedPlaybookRegistry;
                let mut kinds = registry.kinds();
                kinds.sort();
                let mut out = Context::new();
                out.set(SEED_ENUM_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        check_def(
            "the seed enumerated kinds are exactly {string}",
            &[(SEED_ENUM_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let mut expected =
                    parse_kinds_list(params.get_string(0).ok_or("Expected kinds")?.as_ref());
                expected.sort();
                let actual = ctx
                    .get::<Vec<String>>(SEED_ENUM_KINDS_KEY)
                    .ok_or("No seed enumerated kinds")?;
                if *actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly {:?} but got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
    ]
}
