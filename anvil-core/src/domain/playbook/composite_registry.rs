//! `CompositePlaybookRegistry` — hearth-first with seed-fallback composition.
//!
//! Wraps two `PlaybookRegistry` impls: a hearth registry (H) and a seed
//! registry (S). `machine_for` attempts hearth resolution first; if the
//! hearth returns `None`, the seed is consulted.
//!
//! ## Generic shape
//!
//! `CompositePlaybookRegistry<H, S>` is generic over the two concrete
//! registry types. This avoids `dyn` dispatch at the composition layer while
//! keeping the trait-based seam intact. Callers that pass `&composite` as
//! `&dyn PlaybookRegistry` trigger a single virtual dispatch at the
//! consumer boundary, not two.
//!
//! ## Lifetime story
//!
//! Both `H::machine_for` and `S::machine_for` return `Option<&'a PlaybookMachine>`
//! where `'a` is tied to `&'a self`. Since the composite holds both as fields,
//! `&'a self` covers `&'a self.hearth` and `&'a self.seed` — no additional
//! lifetime annotations are required beyond the trait's existing signature.
//!
//! `SeedPlaybookRegistry` returns `Option<&'static PlaybookMachine>`; `'static`
//! satisfies any `'a`, so the seed arm compiles without annotation.

use crate::domain::playbook::registry::{PlaybookRegistry, PlaybookSource};
use crate::domain::playbook::types::PlaybookMachine;

/// Generic composite that attempts hearth resolution first, then falls back to seed.
pub struct CompositePlaybookRegistry<H: PlaybookRegistry, S: PlaybookRegistry> {
    hearth: H,
    seed: S,
}

impl<H: PlaybookRegistry, S: PlaybookRegistry> CompositePlaybookRegistry<H, S> {
    /// Construct a new composite from a hearth registry and a seed registry.
    pub fn new(hearth: H, seed: S) -> Self {
        Self { hearth, seed }
    }
}

impl<H: PlaybookRegistry, S: PlaybookRegistry> PlaybookRegistry
    for CompositePlaybookRegistry<H, S>
{
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.hearth
            .machine_for(kind)
            .or_else(|| self.seed.machine_for(kind))
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.hearth
            .playbook_id_for(kind)
            .or_else(|| self.seed.playbook_id_for(kind))
    }

    fn source_for(&self, kind: &str) -> Option<PlaybookSource> {
        self.hearth
            .source_for(kind)
            .or_else(|| self.seed.source_for(kind))
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        // Hearth-first de-dup by kind: every hearth machine, then each seed
        // machine whose kind the hearth did not already provide. Hearth order
        // is preserved first, then seed order — mirroring the `machine_for`
        // hearth-precedence resolution.
        let mut out: Vec<&'a PlaybookMachine> = self.hearth.all_machines();
        let mut seen: std::collections::HashSet<String> =
            out.iter().map(|m| m.kind.clone()).collect();
        for machine in self.seed.all_machines() {
            if seen.insert(machine.kind.clone()) {
                out.push(machine);
            }
        }
        out
    }
}
