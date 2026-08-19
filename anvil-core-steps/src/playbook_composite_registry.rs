//! Step module for `composite_playbook_registry.feature`.
//!
//! Provides steps for:
//! - Constructing a `FakePlaybookRegistry` (a test double implementing
//!   `PlaybookRegistry` with a fixed `HashMap<String, PlaybookMachine>`)
//!   as the hearth slot and seed slot of a `CompositePlaybookRegistry`
//! - Calling `machine_for(kind)` on the composite
//! - Asserting that the composite returns the hearth value (hearth-first)
//! - Asserting fall-through to seed on hearth miss
//! - Asserting None when both hearth and seed miss
//!
//! These steps use `FakePlaybookRegistry` — a test-only struct with a fixed
//! map — rather than `HearthPlaybookRegistry` or `SeedPlaybookRegistry`. This
//! isolates composite logic from adapter implementation details.

use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::playbook::types::{
    PlaybookMachine, RouteConfig, StateDefinition, TransitionDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

// ─── Context keys ──────────────────────────────────────────────────────────────

/// Key for the fake hearth registry's kind map (HashMap<String, PlaybookMachine>).
const FAKE_HEARTH_MAP_KEY: &str = "cwr_fake_hearth_map";
/// Key for the fake seed registry's kind map (HashMap<String, PlaybookMachine>).
const FAKE_SEED_MAP_KEY: &str = "cwr_fake_seed_map";
/// Key for the composite-resolved machine (Option<PlaybookMachine>).
const COMPOSITE_RESOLVED_KEY: &str = "cwr_resolved";
/// Key for the hearth slot's id-registry presence (Option<String> kind).
const ID_HEARTH_KIND_KEY: &str = "cwr_id_hearth_kind";
/// Key for the seed slot's id-registry presence (Option<String> kind).
const ID_SEED_KIND_KEY: &str = "cwr_id_seed_kind";
/// Key for the composite-resolved playbook id (Option<String>).
const COMPOSITE_RESOLVED_ID_KEY: &str = "cwr_resolved_id";

// ─── Test double ───────────────────────────────────────────────────────────────

/// A minimal `PlaybookMachine` value for a given kind.
///
/// All fields beyond `kind` carry zero-values; the only field the composite
/// scenarios assert on is `kind`.
fn fake_machine(kind: &str) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        directory: format!("{}s", kind),
        registry: format!("{}s.md", kind),
        parent_kind: None,
        description: format!("Fake {} machine for composite registry tests.", kind),
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

/// Test double: a `PlaybookRegistry` backed by a fixed map.
///
/// `machine_for` performs a map lookup; the map is owned by `self`.
struct FakePlaybookRegistry {
    map: HashMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for FakePlaybookRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map.get(kind)
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.map.get(kind).map(|_| format!("{}_playbook", kind))
    }
}

/// Test double for the `playbook_id_for` delegation scenarios (M3).
///
/// Returns a slot-labeled id (`"{slot}:{kind}"`) when `kind` matches the one
/// it was built with, so the composite scenarios can distinguish a hearth-slot
/// answer from a seed-slot answer and prove delegation order. `machine_for` is
/// unused by the id scenarios and returns `None`.
struct IdFakeRegistry {
    slot: &'static str,
    kind: Option<String>,
}

impl PlaybookRegistry for IdFakeRegistry {
    fn machine_for<'a>(&'a self, _kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        None
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        match &self.kind {
            Some(k) if k == kind => Some(format!("{}:{}", self.slot, kind)),
            _ => None,
        }
    }
}

// ─── Step definitions ──────────────────────────────────────────────────────────

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: fake hearth with a specific kind =====
        step_def(
            "a fake hearth registry with kind {string}",
            &[],
            &[(FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut map: HashMap<String, PlaybookMachine> = HashMap::new();
                map.insert(kind.clone(), fake_machine(&kind));
                let mut out = Context::new();
                out.set(FAKE_HEARTH_MAP_KEY, map);
                Ok(out)
            },
        ),
        // ===== Given: fake hearth with no kinds =====
        step_def(
            "a fake hearth registry with no kinds",
            &[],
            &[(FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>")],
            |_ctx, _params| {
                let map: HashMap<String, PlaybookMachine> = HashMap::new();
                let mut out = Context::new();
                out.set(FAKE_HEARTH_MAP_KEY, map);
                Ok(out)
            },
        ),
        // ===== And: fake seed with a specific kind =====
        step_def(
            "a fake seed registry with kind {string}",
            &[(FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>")],
            &[
                (FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>"),
                (FAKE_SEED_MAP_KEY, "HashMap<String,PlaybookMachine>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth_map = ctx
                    .get::<HashMap<String, PlaybookMachine>>(FAKE_HEARTH_MAP_KEY)
                    .ok_or("No fake hearth map")?
                    .clone();
                let mut seed_map: HashMap<String, PlaybookMachine> = HashMap::new();
                seed_map.insert(kind.clone(), fake_machine(&kind));
                let mut out = Context::new();
                out.set(FAKE_HEARTH_MAP_KEY, hearth_map);
                out.set(FAKE_SEED_MAP_KEY, seed_map);
                Ok(out)
            },
        ),
        // ===== And: fake seed with no kinds =====
        step_def(
            "a fake seed registry with no kinds",
            &[(FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>")],
            &[
                (FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>"),
                (FAKE_SEED_MAP_KEY, "HashMap<String,PlaybookMachine>"),
            ],
            |ctx, _params| {
                let hearth_map = ctx
                    .get::<HashMap<String, PlaybookMachine>>(FAKE_HEARTH_MAP_KEY)
                    .ok_or("No fake hearth map")?
                    .clone();
                let seed_map: HashMap<String, PlaybookMachine> = HashMap::new();
                let mut out = Context::new();
                out.set(FAKE_HEARTH_MAP_KEY, hearth_map);
                out.set(FAKE_SEED_MAP_KEY, seed_map);
                Ok(out)
            },
        ),
        // ===== When: composite registry resolves kind =====
        step_def(
            "the composite registry resolves kind {string}",
            &[
                (FAKE_HEARTH_MAP_KEY, "HashMap<String,PlaybookMachine>"),
                (FAKE_SEED_MAP_KEY, "HashMap<String,PlaybookMachine>"),
            ],
            &[(COMPOSITE_RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth_map = ctx
                    .get::<HashMap<String, PlaybookMachine>>(FAKE_HEARTH_MAP_KEY)
                    .ok_or("No fake hearth map")?
                    .clone();
                let seed_map = ctx
                    .get::<HashMap<String, PlaybookMachine>>(FAKE_SEED_MAP_KEY)
                    .ok_or("No fake seed map")?
                    .clone();

                let hearth = FakePlaybookRegistry { map: hearth_map };
                let seed = FakePlaybookRegistry { map: seed_map };
                let composite = CompositePlaybookRegistry::new(hearth, seed);

                // Clone so the result outlives the composite (which is dropped at end of block).
                let resolved: Option<PlaybookMachine> = composite.machine_for(&kind).cloned();

                let mut out = Context::new();
                out.set(COMPOSITE_RESOLVED_KEY, resolved);
                Ok(out)
            },
        ),
        // ===== Then: composite-resolved machine has kind =====
        check_def(
            "the composite-resolved machine has kind {string}",
            &[(COMPOSITE_RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_kind = params.get_string(0).ok_or("Expected kind")?;
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(COMPOSITE_RESOLVED_KEY)
                    .ok_or("No composite resolved machine")?;
                match resolved.as_ref() {
                    Some(m) => {
                        if m.kind != expected_kind {
                            Err(format!(
                                "Expected machine with kind '{}' but got '{}'",
                                expected_kind, m.kind
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(format!(
                        "Expected machine with kind '{}' but composite returned None",
                        expected_kind
                    )),
                }
            },
        ),
        // ===== M3: playbook_id_for delegation =====
        // Given: hearth slot for id resolution knows / does not know a kind
        step_def(
            "an id hearth registry with kind {string}",
            &[],
            &[(ID_HEARTH_KIND_KEY, "Option<String>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut out = Context::new();
                out.set(ID_HEARTH_KIND_KEY, Some(kind));
                Ok(out)
            },
        ),
        step_def(
            "an id hearth registry with no kinds",
            &[],
            &[(ID_HEARTH_KIND_KEY, "Option<String>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(ID_HEARTH_KIND_KEY, None::<String>);
                Ok(out)
            },
        ),
        step_def(
            "an id seed registry with kind {string}",
            &[(ID_HEARTH_KIND_KEY, "Option<String>")],
            &[
                (ID_HEARTH_KIND_KEY, "Option<String>"),
                (ID_SEED_KIND_KEY, "Option<String>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth = ctx
                    .get::<Option<String>>(ID_HEARTH_KIND_KEY)
                    .ok_or("No id hearth kind")?
                    .clone();
                let mut out = Context::new();
                out.set(ID_HEARTH_KIND_KEY, hearth);
                out.set(ID_SEED_KIND_KEY, Some(kind));
                Ok(out)
            },
        ),
        step_def(
            "an id seed registry with no kinds",
            &[(ID_HEARTH_KIND_KEY, "Option<String>")],
            &[
                (ID_HEARTH_KIND_KEY, "Option<String>"),
                (ID_SEED_KIND_KEY, "Option<String>"),
            ],
            |ctx, _params| {
                let hearth = ctx
                    .get::<Option<String>>(ID_HEARTH_KIND_KEY)
                    .ok_or("No id hearth kind")?
                    .clone();
                let mut out = Context::new();
                out.set(ID_HEARTH_KIND_KEY, hearth);
                out.set(ID_SEED_KIND_KEY, None::<String>);
                Ok(out)
            },
        ),
        // When: composite resolves a playbook id for a kind
        step_def(
            "the composite registry resolves playbook id for kind {string}",
            &[
                (ID_HEARTH_KIND_KEY, "Option<String>"),
                (ID_SEED_KIND_KEY, "Option<String>"),
            ],
            &[(COMPOSITE_RESOLVED_ID_KEY, "Option<String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let hearth_kind = ctx
                    .get::<Option<String>>(ID_HEARTH_KIND_KEY)
                    .ok_or("No id hearth kind")?
                    .clone();
                let seed_kind = ctx
                    .get::<Option<String>>(ID_SEED_KIND_KEY)
                    .ok_or("No id seed kind")?
                    .clone();
                let hearth = IdFakeRegistry {
                    slot: "hearth",
                    kind: hearth_kind,
                };
                let seed = IdFakeRegistry {
                    slot: "seed",
                    kind: seed_kind,
                };
                let composite = CompositePlaybookRegistry::new(hearth, seed);
                let resolved = composite.playbook_id_for(&kind);
                let mut out = Context::new();
                out.set(COMPOSITE_RESOLVED_ID_KEY, resolved);
                Ok(out)
            },
        ),
        // Then: composite-resolved playbook id equals
        check_def(
            "the composite-resolved playbook id is {string}",
            &[(COMPOSITE_RESOLVED_ID_KEY, "Option<String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected id")?;
                let resolved = ctx
                    .get::<Option<String>>(COMPOSITE_RESOLVED_ID_KEY)
                    .ok_or("No composite resolved id")?;
                match resolved {
                    Some(id) if id == expected.as_ref() as &str => Ok(()),
                    Some(id) => Err(format!(
                        "Expected playbook id '{}' but got '{}'",
                        expected, id
                    )),
                    None => Err(format!("Expected playbook id '{}' but got None", expected)),
                }
            },
        ),
        // Then: composite-resolved playbook id is absent
        check_def(
            "the composite-resolved playbook id is absent",
            &[(COMPOSITE_RESOLVED_ID_KEY, "Option<String>")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<Option<String>>(COMPOSITE_RESOLVED_ID_KEY)
                    .ok_or("No composite resolved id key")?;
                if let Some(id) = resolved {
                    Err(format!("Expected None but got Some('{}')", id))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Then: composite-resolved machine is absent =====
        check_def(
            "the composite-resolved machine is absent",
            &[(COMPOSITE_RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(COMPOSITE_RESOLVED_KEY)
                    .ok_or("No composite resolved machine key")?;
                if resolved.is_some() {
                    Err(format!(
                        "Expected None but composite returned Some(machine with kind '{}')",
                        resolved.as_ref().unwrap().kind
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
