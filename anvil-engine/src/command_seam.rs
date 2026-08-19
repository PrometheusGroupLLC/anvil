//! The CQRS **command** seam — which surface asked this engine to change something.
//!
//! ## What was already here, and what was missing
//!
//! This engine has recorded its command turns for a long time. Every begin,
//! snapshot, complete and amend already writes a structured record naming the
//! command, the actor, the resolved hearth, the emitted event variants and the
//! outcome — that is the record `cqrs_logging_begin.feature` and its seven
//! siblings assert. The read side gained its own record in `ws_bridge`: every
//! query a screen makes over the loopback bridge names the screen, and a query
//! that names none is refused.
//!
//! The half that was missing is WHICH PROGRAM asked for a change. `begin`,
//! `snapshot`, `complete` and `amend` arrive over one wire from two different
//! programs — the MCP shim and the `anvil-hooks` command line — and the record
//! could not tell them apart. The standing law is that the CLI, the UI and the
//! tool surface are peers, and state that cannot be explained means the seam
//! logging is defective. A change whose origin cannot be named is exactly that.
//!
//! So this module adds `surface` to the channel that already exists. It does
//! not invent a second one: the record is a `tracing` event with flattened
//! closed-label fields, which is the idiom both the per-command outcome record
//! and the `ws_bridge` read seam already use.
//!
//! ## Why two records per command and not one
//!
//! [`CommandSeam`] writes an `issued` record BEFORE the change is attempted and
//! a `settled` record AFTER the outcome is known, both carrying the same
//! `command_id`.
//!
//! One record written afterwards disappears entirely when the change is
//! interrupted — the engine is killed, the handler panics, a future is dropped
//! mid-await — and an action that left no trace is indistinguishable, later,
//! from an action nobody took. That is the failure this pair exists to prevent:
//! an interrupted change reads as **asked for, never resolved** rather than as
//! absent. `settled` is emitted from `Drop`, so it covers every exit path of a
//! handler including the seven separate error exits `begin` carries — a
//! hand-placed emit at each `return` is one refactor away from missing one.
//!
//! ## Why the surface is a closed set and has no default
//!
//! A default surface is exactly how an unattributable change comes to look
//! attributed, and a name the recorder supplies to itself records nothing about
//! who asked. So a request naming no surface is REFUSED and the change is not
//! made, and a name this engine does not recognise is refused too: an open field
//! accepts anything and therefore distinguishes nothing.
//!
//! `TestHarness` is a real member of that set rather than a lie told by the
//! Brine harness. The harness IS a caller of this engine, and having it name
//! itself is honest where having it impersonate `Cli` would corrupt the very
//! measurement this seam exists to produce. `ws_bridge` set the same precedent
//! when its sixteen harness call sites began naming themselves.

use std::sync::atomic::{AtomicU64, Ordering};

/// The gRPC metadata key carrying the surface, on every state-changing call.
///
/// Metadata rather than a request field on purpose: it is transport-level
/// provenance about the CALLER, not part of any command's domain payload, and
/// putting it in the message would mean adding it to four proto messages that
/// have nothing else in common. `authorization` — the other piece of caller
/// provenance this engine reads — travels the same way.
pub const SURFACE_METADATA_KEY: &str = "x-anvil-surface";

/// The `seam` label that selects these records out of the log stream.
///
/// The per-command outcome record and this one share the `command` field, so a
/// check keyed only on the command name reads whichever of the two it meets
/// first. `seam` is what makes the pair selectable, exactly as `ws_bridge` is
/// on the read side.
pub const COMMAND_SEAM: &str = "engine_command";

/// The surfaces that may ask this engine to change something.
///
/// Closed by construction. Adding a variant is a deliberate act with a wire
/// name chosen once; accepting free text would make the field unfalsifiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The `anvil-hooks` binary — the command line that reaches the engine
    /// without the MCP server. Not broker-independent: against a Foundry-mode
    /// engine it still presents a bearer, which the broker is what mints.
    Cli,
    /// The MCP shim, which is how an agent reaches this engine.
    Mcp,
    /// A screen. Commands do not arrive from the loopback bridge today (it is
    /// read-only), but the name is the same one the read seam records, so the
    /// two halves of the seam agree on what a surface is called.
    Ui,
    /// The Brine harness. A real caller that names itself rather than
    /// impersonating a shipped surface.
    TestHarness,
}

impl Surface {
    /// The wire name, which is also the recorded label.
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Cli => "cli",
            Surface::Mcp => "mcp",
            Surface::Ui => "ui",
            Surface::TestHarness => "test-harness",
        }
    }

    /// Parse a wire name. `None` for anything not in the closed set.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "cli" => Some(Surface::Cli),
            "mcp" => Some(Surface::Mcp),
            "ui" => Some(Surface::Ui),
            "test-harness" => Some(Surface::TestHarness),
            _ => None,
        }
    }
}

/// The label recorded when a request named no surface at all.
pub const UNNAMED_SURFACE: &str = "(unnamed)";
/// The label recorded when a request named a surface this engine does not know.
///
/// Deliberately NOT the rejected string itself: echoing caller-supplied text
/// into the log stream is how an unbounded label set gets in through the back
/// door, and the same allowlist discipline every other durable sink here keeps
/// says the record carries closed labels and identifiers, never caller input.
pub const UNRECOGNISED_SURFACE: &str = "(unrecognised)";

/// Monotonic source for `command_id`. Process-local and cheap: the id only has
/// to correlate an `issued` with its `settled` inside one engine's log stream,
/// which is the only place both records can appear.
static NEXT_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

fn next_command_id() -> u64 {
    NEXT_COMMAND_ID.fetch_add(1, Ordering::Relaxed)
}

/// Read the surface off an inbound request, refusing rather than defaulting.
///
/// Returns the parsed surface, or the label to record and the refusal reason.
/// The caller records and refuses — this function does not log, so that the
/// refusal record and the served record are written by the same code path and
/// cannot drift apart.
pub fn read_surface<T>(request: &tonic::Request<T>) -> Result<Surface, SurfaceRefusal> {
    let raw = request
        .metadata()
        .get(SURFACE_METADATA_KEY)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string();
    if raw.is_empty() {
        return Err(SurfaceRefusal::Unnamed);
    }
    Surface::parse(&raw).ok_or(SurfaceRefusal::Unrecognised)
}

/// Why a request's surface was not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRefusal {
    /// No surface on the request at all.
    Unnamed,
    /// A surface name outside the closed set.
    Unrecognised,
}

impl SurfaceRefusal {
    /// The label recorded in the `surface` field.
    pub fn surface_label(self) -> &'static str {
        match self {
            SurfaceRefusal::Unnamed => UNNAMED_SURFACE,
            SurfaceRefusal::Unrecognised => UNRECOGNISED_SURFACE,
        }
    }

    /// The recorded outcome.
    pub fn outcome(self) -> &'static str {
        match self {
            SurfaceRefusal::Unnamed => "refused_surface_required",
            SurfaceRefusal::Unrecognised => "refused_surface_unknown",
        }
    }

    /// The message the caller is refused with. This is the string a client keys
    /// on, so it is a stable code and never prose.
    pub fn status_message(self) -> &'static str {
        match self {
            SurfaceRefusal::Unnamed => "surface_required",
            SurfaceRefusal::Unrecognised => "surface_unknown",
        }
    }
}

/// Record a refused command and build the `Status` to refuse it with.
///
/// The change is not made. A refusal is written to the same seam as a served
/// command, with `phase` `settled`, because "asked for and refused" is a fact a
/// reader needs — an unattributable command that simply vanished would be the
/// unreadable absence this whole seam exists to prevent.
pub fn refuse(command: &'static str, refusal: SurfaceRefusal) -> tonic::Status {
    tracing::info!(
        seam = COMMAND_SEAM,
        command = command,
        surface = refusal.surface_label(),
        actor = "(unattributed)",
        phase = "settled",
        outcome = refusal.outcome(),
        command_id = 0_u64,
        "engine command seam"
    );
    tonic::Status::invalid_argument(refusal.status_message())
}

/// One command turn's seam records: `issued` on construction, `settled` on drop.
///
/// Held by value in the handler. Because `settled` is emitted from `Drop` it
/// covers every way a handler can leave — the happy return, each `?` on an
/// error exit, and an await that is cancelled — without a hand-placed emit at
/// each site that a later refactor could forget.
pub struct CommandSeam {
    command: &'static str,
    surface: &'static str,
    actor: String,
    id: u64,
    outcome: &'static str,
}

impl CommandSeam {
    /// Open a turn and write its `issued` record, before the change is tried.
    pub fn issue(command: &'static str, surface: Surface, actor: &str) -> Self {
        let id = next_command_id();
        tracing::info!(
            seam = COMMAND_SEAM,
            command = command,
            surface = surface.as_str(),
            actor = %actor,
            phase = "issued",
            outcome = "pending",
            command_id = id,
            "engine command seam"
        );
        Self {
            command,
            surface: surface.as_str(),
            actor: actor.to_string(),
            id,
            // Until the handler says otherwise the turn ended without reaching
            // its own success path. `error` is the honest default: a turn that
            // is dropped having neither succeeded nor been marked is one that
            // did not finish, and defaulting to `ok` would launder exactly the
            // interruptions this pair exists to make readable.
            outcome: "error",
        }
    }

    /// Mark the turn as having succeeded. Anything else stays `error`.
    pub fn ok(&mut self) {
        self.outcome = "ok";
    }

    /// The correlation id, for a caller that wants to name it elsewhere.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The surface label, for folding into the per-command outcome record.
    pub fn surface(&self) -> &'static str {
        self.surface
    }
}

impl Drop for CommandSeam {
    fn drop(&mut self) {
        tracing::info!(
            seam = COMMAND_SEAM,
            command = self.command,
            surface = self.surface,
            actor = %self.actor,
            phase = "settled",
            outcome = self.outcome,
            command_id = self.id,
            "engine command seam"
        );
    }
}
