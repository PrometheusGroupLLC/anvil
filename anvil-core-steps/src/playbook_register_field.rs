//! Step module for `playbook_register_field.feature` (playbook_routing_layer BP1).
//!
//! Reuses the `playbook_loader` Given ("a playbook machine.yaml with content:")
//! which stores the raw YAML under `wl_yaml_text`. Adds:
//! - a When that parses the YAML and stores the loaded machine
//! - a Then asserting the machine's `register`
//! - a driven-candidates selection over a small in-memory registry, proving
//!   free machines are excluded (the AC4 primitive at the core seam).

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook::registry::{driven_candidates, PlaybookRegistry};
use anvil_core::domain::playbook::types::{
    PlaybookMachine, Register, RouteConfig, StateDefinition, TransitionDefinition,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::HashMap;

/// Key the playbook_loader Given writes the raw YAML to.
const YAML_KEY: &str = "wl_yaml_text";
/// The loaded machine for register assertions.
const RF_MACHINE_KEY: &str = "rf_machine";
/// The selected driven candidate kinds.
const RF_DRIVEN_KINDS_KEY: &str = "rf_driven_kinds";

/// A small ordered registry double for the driven-candidates scenario.
struct MixedRegistry {
    order: Vec<String>,
    map: HashMap<String, PlaybookMachine>,
}

impl PlaybookRegistry for MixedRegistry {
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

fn machine(kind: &str, register: Register) -> PlaybookMachine {
    PlaybookMachine {
        kind: kind.to_string(),
        directory: format!("{}s", kind),
        registry: format!("{}s.md", kind),
        parent_kind: None,
        description: format!("{} machine", kind),
        route: RouteConfig::default(),
        required_fields: vec![],
        roles: vec!["doer".to_string()],
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
            to_state: "active".to_string(),
            required_role: "doer".to_string(),
            required_satisfaction: None,
            requires_approver: false,
            hook: None,
        }],
        register,
        ..Default::default()
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the register-field loader parses the machine",
            &[(YAML_KEY, "String")],
            &[(RF_MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let yaml = ctx.get::<String>(YAML_KEY).ok_or("No yaml text")?;
                let result = load_from_yaml("register-field-test", yaml, &[]);
                let mut out = Context::new();
                match result {
                    Ok(m) => out.set(RF_MACHINE_KEY, Some(m)),
                    Err(_) => out.set(RF_MACHINE_KEY, None::<PlaybookMachine>),
                }
                Ok(out)
            },
        ),
        check_def(
            "the parse succeeds and the machine register is {string}",
            &[(RF_MACHINE_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected register value")?;
                let machine = ctx
                    .get::<Option<PlaybookMachine>>(RF_MACHINE_KEY)
                    .ok_or("No machine key")?
                    .as_ref()
                    .ok_or("Parse did not succeed")?;
                let actual = if machine.is_driven() {
                    "driven"
                } else {
                    "free"
                };
                if actual != expected.as_ref() as &str {
                    Err(format!(
                        "Expected register '{}' but got '{}'",
                        expected, actual
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        step_def(
            "a registry with a driven {string} machine and a free {string} machine",
            &[],
            &[(RF_DRIVEN_KINDS_KEY, "Vec<String>")],
            |_ctx, params| {
                let driven_kind = params
                    .get_string(0)
                    .ok_or("Expected driven kind")?
                    .to_string();
                let free_kind = params
                    .get_string(1)
                    .ok_or("Expected free kind")?
                    .to_string();
                let mut map = HashMap::new();
                map.insert(driven_kind.clone(), machine(&driven_kind, Register::Driven));
                map.insert(free_kind.clone(), machine(&free_kind, Register::Free));
                let registry = MixedRegistry {
                    order: vec![driven_kind, free_kind],
                    map,
                };
                let kinds: Vec<String> = driven_candidates(&registry)
                    .iter()
                    .map(|m| m.kind.clone())
                    .collect();
                let mut out = Context::new();
                out.set(RF_DRIVEN_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        // The selection happens in the Given above; this When is a no-op marker
        // so the scenario reads naturally (Given seeds + selects, When asserts).
        step_def(
            "I select the driven candidates",
            &[(RF_DRIVEN_KINDS_KEY, "Vec<String>")],
            &[(RF_DRIVEN_KINDS_KEY, "Vec<String>")],
            |ctx, _params| {
                let kinds = ctx
                    .get::<Vec<String>>(RF_DRIVEN_KINDS_KEY)
                    .ok_or("No driven kinds")?
                    .clone();
                let mut out = Context::new();
                out.set(RF_DRIVEN_KINDS_KEY, kinds);
                Ok(out)
            },
        ),
        check_def(
            "the driven candidate kinds include {string}",
            &[(RF_DRIVEN_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let kinds = ctx
                    .get::<Vec<String>>(RF_DRIVEN_KINDS_KEY)
                    .ok_or("No driven kinds")?;
                if kinds.contains(&kind) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' in driven candidates {:?}",
                        kind, kinds
                    ))
                }
            },
        ),
        check_def(
            "the driven candidate kinds do not include {string}",
            &[(RF_DRIVEN_KINDS_KEY, "Vec<String>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let kinds = ctx
                    .get::<Vec<String>>(RF_DRIVEN_KINDS_KEY)
                    .ok_or("No driven kinds")?;
                if kinds.contains(&kind) {
                    Err(format!(
                        "Expected '{}' absent but found in {:?}",
                        kind, kinds
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
