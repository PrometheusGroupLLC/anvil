//! # foundry-engine-addressing
//!
//! Shared **engine-addressing contract** for Foundry kits.
//!
//! Every Foundry kit engine binds a **dynamic** TCP port (Foundry's
//! supervisor assigns `--port`, e.g. `143xx`), but historically every MCP/CLI
//! client defaulted to a hard-coded URL with no shared discovery — so the
//! engine could be alive while every client got connection-refused. This
//! crate is the single library both sides link to close that gap:
//!
//! - **Engine side ([`publish`] / [`cleanup`]):** at the moment the real bound
//!   port is known, the engine atomically writes a [`RendezvousRecord`] to
//!   `<dir>/engine.json` (`<dir>` is typically `~/.<kit>/`). On clean shutdown
//!   it removes the file.
//! - **Client side ([`resolve`]):** an MCP server or CLI resolves the engine
//!   URL in one fixed order — env override, then the rendezvous file (its URL
//!   is **honestly health-probed** over HTTP before being trusted), then a
//!   configured default. The returned [`Resolution`] records *why* a URL was
//!   chosen so clients can log it.
//!
//! ## Honest health (the load-bearing rule)
//!
//! A rendezvous file is a **hint, not truth**. An HTTP `GET <url><health_path>`
//! with a short timeout is **authoritative**: only a 2xx response trusts the
//! file. The recorded `pid` is consulted via `kill(pid, 0)` purely as an
//! *advisory* stale-file signal (it is logged, never allowed to override a
//! failed HTTP probe — that avoids the pid-reuse race). A dead probe falls
//! through cleanly; the resolver never hangs and never returns a silent wrong
//! default.
//!
//! ## Scope
//!
//! T0 shipped the writer, the resolver, and the HTTP health probe. T2 adds the
//! data-dir-keyed singleton lock ([`acquire_singleton`]) — orphan *prevention*:
//! a second engine on the SAME data-dir refuses to bind, while engines on
//! distinct data-dirs (Foundry's dynamic ports, Brine's temp dirs) never
//! contend.

mod probe;
mod record;
mod resolver;
mod singleton;
mod writer;

pub use probe::{probe_health, IdentityCheck, ProbeOutcome};
pub use record::{RendezvousRecord, SCHEMA_VERSION};
pub use resolver::{resolve, Resolution, ResolveOpts, ResolveSource};
pub use singleton::{acquire_singleton, SingletonError, SingletonGuard, SINGLETON_LOCK_FILENAME};
pub use writer::{cleanup, publish, RENDEZVOUS_FILENAME};
