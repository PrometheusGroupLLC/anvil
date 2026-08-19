//! Step module for `playbook_registry_seed_adapter.feature`.
//!
//! Provides steps for:
//! - Calling `SeedPlaybookRegistry::machine_for(kind)`
//! - Asserting the resolved machine's `kind` field
//! - Asserting the result is `None` (absent) for unknown kinds

use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core::domain::playbook::types::PlaybookMachine;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

/// Context key for the resolved machine (Option<PlaybookMachine>).
const RESOLVED_KEY: &str = "wr_resolved";
/// Context key for the resolved playbook id (Option<String>).
const RESOLVED_ID_KEY: &str = "wr_resolved_id";

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== When: resolve kind via seed registry =====
        step_def(
            "the seed registry resolves kind {string}",
            &[],
            &[(RESOLVED_KEY, "Option<PlaybookMachine>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let registry = SeedPlaybookRegistry;
                // Clone the machine so it can be stored in context.
                let resolved: Option<PlaybookMachine> = registry.machine_for(&kind).cloned();
                let mut out = Context::new();
                out.set(RESOLVED_KEY, resolved);
                Ok(out)
            },
        ),
        // ===== Then: resolved machine has kind =====
        check_def(
            "the resolved machine has kind {string}",
            &[(RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, params| {
                let expected_kind = params.get_string(0).ok_or("Expected kind")?;
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(RESOLVED_KEY)
                    .ok_or("No resolved machine")?;
                match resolved {
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
                        "Expected machine with kind '{}' but got None",
                        expected_kind
                    )),
                }
            },
        ),
        // ===== When: resolve playbook id via seed registry =====
        step_def(
            "the seed registry resolves playbook id for kind {string}",
            &[],
            &[(RESOLVED_ID_KEY, "Option<String>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let registry = SeedPlaybookRegistry;
                let resolved: Option<String> = registry.playbook_id_for(&kind);
                let mut out = Context::new();
                out.set(RESOLVED_ID_KEY, resolved);
                Ok(out)
            },
        ),
        // ===== Then: resolved playbook id equals =====
        check_def(
            "the resolved playbook id is {string}",
            &[(RESOLVED_ID_KEY, "Option<String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected id")?;
                let resolved = ctx
                    .get::<Option<String>>(RESOLVED_ID_KEY)
                    .ok_or("No resolved playbook id")?;
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
        // ===== Then: resolved playbook id is absent =====
        check_def(
            "the resolved playbook id is absent",
            &[(RESOLVED_ID_KEY, "Option<String>")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<Option<String>>(RESOLVED_ID_KEY)
                    .ok_or("No resolved playbook id key")?;
                if let Some(id) = resolved {
                    Err(format!("Expected None but got Some('{}')", id))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Then: resolved machine is absent =====
        check_def(
            "the resolved machine is absent",
            &[(RESOLVED_KEY, "Option<PlaybookMachine>")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<Option<PlaybookMachine>>(RESOLVED_KEY)
                    .ok_or("No resolved machine key")?;
                if resolved.is_some() {
                    Err(format!(
                        "Expected None but got Some(machine with kind '{}')",
                        resolved.as_ref().unwrap().kind
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
