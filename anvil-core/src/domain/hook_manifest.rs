//! Hook-manifest fold — the harness-agnostic kit-hook DELIVERY contract.
//!
//! A native-hook installer (and Foundry) needs ONE Anvil-side surface that says,
//! per kit/hearth: which hooks exist, what each binds to, its body, and whether
//! it is a HARD (refuse) or SOFT (warn) pre-mutation gate. That contract is
//! DERIVED, never duplicated: the hook bodies and their (state, role) bindings
//! already live in each playbook machine's `hooks_by_role` declarations under
//! `playbooks/<wf>/hooks/`. This fold reads those declarations and the manifest's
//! `hard_enforce` policy and produces the flat, sorted, installable hook set.
//!
//! The body of each hook is read through the SAME [`PlaybookHookBodyPort`] that
//! `begin` uses (via [`crate::domain::begin::resolve_and_read_hook`]'s reader),
//! and passed through the SAME byte budget ([`cap_hook_body`]), so the contract's
//! `body` can never drift from what `begin` actually serves.
//!
//! This module is harness-AGNOSTIC: no harness names appear here. `hard_enforce`
//! is just a list of playbook kinds; the lane semantics live in the
//! harness/foundry-platform, not Anvil.

use crate::domain::begin::{cap_hook_body, PlaybookHookBodyPort};
use crate::domain::playbook::registry::PlaybookRegistry;

/// One installable hook in the delivery contract.
///
/// Carries the binding `(artifact_kind, state, role)`, the actual served `body`
/// (the same content `begin` serves for that seam, budget-capped), and the
/// `gate` classification ("hard" | "soft").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHook {
    pub artifact_kind: String,
    pub state: String,
    pub role: String,
    pub body: String,
    /// "hard" when `artifact_kind ∈ hard_enforce`, else "soft".
    pub gate: String,
}

/// The hard-gate classification string.
pub const GATE_HARD: &str = "hard";
/// The soft-gate classification string.
pub const GATE_SOFT: &str = "soft";

/// Fold every registered playbook machine's `hooks_by_role` declarations with
/// the `hard_enforce` policy into the flat, sorted installable hook set.
///
/// For each machine in the registry, for each state, for each `(role, filename)`
/// in `state.hooks_by_role`, the body is resolved via `hook_reader` (the same
/// path `begin` uses) and the `(artifact_kind, state, role)` triple is recorded.
/// A state with an empty `hooks_by_role` contributes nothing. The result is
/// sorted by `(artifact_kind, state, role)`.
///
/// `hard_enforce` is a list of playbook kinds; a hook's gate is `GATE_HARD` when
/// its `artifact_kind` is in that list, else `GATE_SOFT`.
///
/// Pure over its inputs apart from the body reads delegated to `hook_reader`.
pub fn fold_hook_manifest(
    registry: &dyn PlaybookRegistry,
    hook_reader: &dyn PlaybookHookBodyPort,
    hard_enforce: &[String],
) -> Vec<ResolvedHook> {
    let mut hooks: Vec<ResolvedHook> = Vec::new();

    for machine in registry.all_machines() {
        let kind = &machine.kind;
        let gate = if hard_enforce.iter().any(|k| k == kind) {
            GATE_HARD
        } else {
            GATE_SOFT
        };
        // Resolve the on-disk source once per kind; if the registry cannot
        // produce one, this kind contributes no installable hooks.
        let Some(source) = registry.source_for(kind) else {
            continue;
        };

        for state in &machine.states {
            // `hooks_by_role` is a BTreeMap, so iteration is role-sorted; the
            // final sort below makes the total order explicit regardless.
            for (role, filename) in &state.hooks_by_role {
                // SAME body-read path `begin` uses, SAME budget cap, so the
                // contract body can never drift from what `begin` serves. A
                // missing/unreadable body is skipped (the loader already
                // validated hook references at registry-build time).
                let Ok(body) = hook_reader.read_playbook_hook_body(&source, filename) else {
                    continue;
                };
                hooks.push(ResolvedHook {
                    artifact_kind: kind.clone(),
                    state: state.name.clone(),
                    role: role.clone(),
                    body: cap_hook_body(body),
                    gate: gate.to_string(),
                });
            }
        }
    }

    hooks.sort_by(|a, b| {
        a.artifact_kind
            .cmp(&b.artifact_kind)
            .then_with(|| a.state.cmp(&b.state))
            .then_with(|| a.role.cmp(&b.role))
    });

    hooks
}
