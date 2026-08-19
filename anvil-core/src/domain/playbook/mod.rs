//! Playbook artifact schema types, loader, seeds, interpreter, and registry.
//!
//! Phase 1: `types` + `load_error` + `loader` — schema types and YAML loader
//! with named rejections (R2, R5.1–5.3).
//!
//! Phase 3: `seeds/` — compiled-in `PlaybookMachine` literals for `track` and
//! `playbook` (R3, R4). Accessed via `seeds::track_seed()` and
//! `seeds::playbook_seed()`.
//!
//! Phase 4:
//! - `interpreter` — pure query functions over `PlaybookMachine` (R12.1).
//!   `outgoing_transitions(machine, from_state)` is the minimum surface Phase 5
//!   needs. No I/O. No global state.
//! - `registry` — `PlaybookRegistry` trait at the consumer boundary. Resolves
//!   a kind-name to a `&PlaybookMachine`. `SeedPlaybookRegistry` is today's
//!   adapter; Track #14 swaps in a hearth-loader adapter.

pub mod anchor_coverage;
pub mod atlas;
pub mod bindability;
pub mod candidate;
pub mod composite_registry;
pub mod event_driven;
pub mod evidence_obligation;
pub mod exemplar;
pub mod exemplar_resolver;
pub mod exemplar_step;
pub mod fs_probe;
pub mod generate;
pub mod hearth_registry;
pub mod hook_serve;
pub mod integrity;
pub mod interpreter;
pub mod load_error;
pub mod loader;
pub mod next_step;
pub mod registry;
pub mod registry_projection;
pub mod scorecard_agg;
pub mod seeds;
pub mod status_read;
pub mod types;
