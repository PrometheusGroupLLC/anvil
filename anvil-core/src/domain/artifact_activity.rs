//! PlaybookActivity read-side: which playbooks are registered and how often
//! each is called, grouped by the kit/author that owns it.
//!
//! This is the query that backs the Foundry playbook-activity UI. It is a pure
//! fold over three inputs:
//!   1. a [`PlaybookRegistry`] enumeration (the registered playbooks),
//!   2. an [`OwnerResolver`] (kind → owner, derived from `contributed_by`), and
//!   3. a redacted [`RoutingActivityRecord`] stream (the durable call log).
//!
//! It groups playbooks by owner and attaches a per-kind call count. The result
//! is deterministic: groups are ordered by owner name, kinds within a group are
//! ordered by kind name.

use crate::domain::playbook::registry::PlaybookRegistry;
use crate::ports::routing_activity_port::RoutingActivityRecord;
use std::collections::BTreeMap;

/// The default owner attributed to a playbook whose status.yaml lacks a
/// (non-empty) `contributed_by` field, or carries no status.yaml at all.
pub const DEFAULT_OWNER: &str = "anvil";

/// Resolves a playbook kind to the owner that contributed it.
///
/// Implementations read the `contributed_by` field from the playbook's
/// status.yaml (resolved via the registry's `playbook_id`). The domain query
/// depends on this trait so it stays pure and unaware of the filesystem; the
/// engine supplies a hearth-backed adapter, tests supply a map.
pub trait OwnerResolver {
    /// Return the owner for `kind`. Empty string / absent → caller applies
    /// [`DEFAULT_OWNER`]; this method returns whatever the source carries.
    fn owner_for(&self, kind: &str) -> Option<String>;
}

/// One registered playbook with its owner, description, and call count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactActivityEntry {
    pub kind: String,
    pub owner: String,
    pub description: String,
    pub call_count: u64,
}

/// Playbooks owned by a single kit/author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerGroup {
    pub owner: String,
    pub entries: Vec<ArtifactActivityEntry>,
}

/// The full PlaybookActivity result: owner groups in deterministic order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactActivityResult {
    pub groups: Vec<OwnerGroup>,
}

impl ArtifactActivityResult {
    /// Flat lookup of an entry by kind across all groups.
    pub fn entry_for(&self, kind: &str) -> Option<&ArtifactActivityEntry> {
        self.groups
            .iter()
            .flat_map(|g| g.entries.iter())
            .find(|e| e.kind == kind)
    }
}

/// Fold a routing-activity record stream into per-kind call counts.
///
/// Counts every record whose `kind` is non-empty. A record for a kind that is
/// no longer registered is simply absent from the final result (the join
/// happens in [`ArtifactActivityQuery::execute`]).
pub fn aggregate_counts(records: &[RoutingActivityRecord]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for record in records {
        if record.kind.is_empty() {
            continue;
        }
        *counts.entry(record.kind.clone()).or_insert(0) += 1;
    }
    counts
}

/// Pure query handler for PlaybookActivity.
pub struct ArtifactActivityQuery;

impl ArtifactActivityQuery {
    pub fn execute(
        registry: &dyn PlaybookRegistry,
        owners: &dyn OwnerResolver,
        records: &[RoutingActivityRecord],
    ) -> ArtifactActivityResult {
        Self::execute_with_counts(registry, owners, &aggregate_counts(records))
    }

    /// Build the owner-grouped result from PRE-FOLDED per-kind call counts. This
    /// is the single source of "call_count" truth — the engine now feeds it the
    /// activity-log fold (counts per resolved `workflow_kind`, the SAME fold
    /// `activity_summary.by_artifact_kind` uses) so the dashboard's owner roll-up
    /// reconciles with the universal usage view, rather than the narrower
    /// routing-activity sink. `execute` keeps the routing-sink path for callers
    /// that still pass records.
    pub fn execute_with_counts(
        registry: &dyn PlaybookRegistry,
        owners: &dyn OwnerResolver,
        counts: &BTreeMap<String, u64>,
    ) -> ArtifactActivityResult {
        // Build entries from the registry enumeration (the source of truth for
        // "registered"). Recorded-but-unregistered kinds are dropped here.
        let mut by_owner: BTreeMap<String, Vec<ArtifactActivityEntry>> = BTreeMap::new();
        for machine in registry.all_machines() {
            let owner = owners
                .owner_for(&machine.kind)
                .filter(|o| !o.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_OWNER.to_string());
            let call_count = counts.get(&machine.kind).copied().unwrap_or(0);
            by_owner
                .entry(owner.clone())
                .or_default()
                .push(ArtifactActivityEntry {
                    kind: machine.kind.clone(),
                    owner,
                    description: machine.description.clone(),
                    call_count,
                });
        }

        let groups = by_owner
            .into_iter()
            .map(|(owner, mut entries)| {
                entries.sort_by(|a, b| a.kind.cmp(&b.kind));
                OwnerGroup { owner, entries }
            })
            .collect();

        ArtifactActivityResult { groups }
    }
}
