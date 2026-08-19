//! The Playbooks column, projected for the host to PULL.
//!
//! # Why this exists, and why it is a PORT rather than a new idea
//!
//! `kit/app/frontend/src/lib/panelPublish.ts` has published `foundry:playbooks`
//! since 2026-08-15, with a real state derivation, and production carries 32 rows
//! of it. It fires from the frontend through a Tauri invoke, so THE ONLY THING
//! THAT CAN TRIGGER IT IS A PERSON OPENING ANVIL'S SURFACE — the same defect
//! kiln's column had, and the same one that left temper's Measures column an
//! absent key while its store held 161 evals.
//!
//! Foundry pulls a kit's panel at kit start from a route the kit declares. An
//! engine route is reachable without a person; a frontend publisher is not.
//!
//! I ALREADY GOT THIS WRONG ONCE, and the shape of the mistake is why this file
//! is a careful port. On 2026-08-16 I shipped a `/panel/playbooks` route whose
//! `state` was the literal `"now"` for every row and whose `title` was the
//! machine's entire description — one of them about a thousand characters — and
//! then reverted it (9fd803e2), because a host pull would have written mine OVER
//! the better rows already in production. The revert's own words: "the fix is the
//! one kiln got — move the derivation into the engine and serve it — not a second
//! projection beside it."
//!
//! So every rule below is transcribed from `panelPublish.ts` and `registry.ts`,
//! and where they disagree with this file, THEY are right and this is the defect.
//!
//! # The contract is the host's
//!
//!     key    foundry:playbooks
//!     value  {"version":1,"playbooks":[{"id","title","group","state","meta"?}]}

use serde::Serialize;

pub const CONTRACT_VERSION: u32 = 1;

/// The kit that owns a playbook it did not receive from someone else.
/// `registry.ts::OWNING_KIT`.
pub const OWNING_KIT: &str = "anvil-kit";

/// The customer's own two groups, `panelPublish.ts`.
pub const GROUP_YOURS: &str = "Yours";
pub const GROUP_SHARED: &str = "Shared with you";

/// What a row says when this engine cannot place it in a ruled state. It RAN and
/// the engine records that it ran, never whether the run was clean — printing
/// "clean" from a call count would be an invented outcome.
///
/// TRANSCRIBED VERBATIM from `panelPublish.ts::UNRECORDED_STATE`, and I had it
/// WRONG until production said so. My first draft wrote kiln's and temper's
/// phrase — "not recorded on this machine" — and my own scenario asserted that
/// same wrong string, so the feature passed while the column would have changed
/// under a reader's eyes the moment the host pulled this instead of the
/// frontend. The ground truth was one command away the whole time: the shipped
/// document in `~/.foundry/kit-state/anvil-kit/state.json` carries this exact
/// sentence on 25 of its 32 rows. An oracle I wrote both sides of proved
/// nothing; the deployed artifact proved it immediately.
pub const UNRECORDED_STATE: &str = "this engine records that it ran, not how it went";

/// How many rows reach the panel. A column is a place to look, not the registry.
pub const MAX_ROWS: usize = 200;

/// The facts one playbook is derived from. Deliberately a plain struct rather
/// than the proto types: the caller folds three engine reads into this, and the
/// derivation below can then be read — and graded — without standing up a hearth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookFacts {
    /// `artifact_id` when there is one, else `kind`. `registry.ts` line 155.
    pub id: String,
    pub kind: String,
    pub owner_kit: String,
    /// `AtlasEntry.integrity.loads` — whether the definition could be read.
    pub loads: bool,
    /// How many runs of this kind are live right now.
    pub live_runs: usize,
    /// Recorded calls for this kind, folded across every owner group.
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PanelPlaybook {
    pub id: String,
    pub title: String,
    pub group: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PanelDocument {
    pub version: u32,
    pub playbooks: Vec<PanelPlaybook>,
}

/// A leading on-disk artifact timestamp: `20260609T1322_`.
/// `playbookNames.ts::ARTIFACT_TIMESTAMP`, as a hand-rolled matcher so this
/// crate takes no regex dependency for one pattern.
fn strip_artifact_timestamp(s: &str) -> &str {
    let b = s.as_bytes();
    // 8 digits, 'T', 4..=6 digits, '_'
    if b.len() < 14 || !b[..8].iter().all(|c| c.is_ascii_digit()) || b[8] != b'T' {
        return s;
    }
    let mut i = 9;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let digits = i - 9;
    if (4..=6).contains(&digits) && i < b.len() && b[i] == b'_' {
        return &s[i + 1..];
    }
    s
}

/// `track_lifecycle` → `Track lifecycle`. PRESENTATION, NOT DATA: this re-spells
/// a value the engine supplied and invents none. A name that humanises to
/// nothing is returned trimmed and unchanged — an empty name is a defect to see,
/// not to paper over.
pub fn display_name(identifier: &str) -> String {
    let stripped = strip_artifact_timestamp(identifier);
    let parts: Vec<&str> = stripped
        .split(|c| c == '_' || c == '-' || c == ' ')
        .filter(|w| !w.is_empty())
        .collect();
    if parts.is_empty() {
        return identifier.trim().to_string();
    }
    let joined = parts.join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => joined,
    }
}

/// Which of the host's dots one row is in, and the fact it was read from.
///
/// ORDER IS MEANING, and it is `panelPublish.ts::deriveState`'s order: a thing
/// that CANNOT be read outranks a thing that is running, which outranks a thing
/// that has never run.
pub fn derive_state(f: &PlaybookFacts) -> (&'static str, &'static str) {
    if !f.loads {
        return ("fault", "definition_would_not_load");
    }
    if f.live_runs > 0 {
        return ("now", "run_live_right_now");
    }
    if f.calls == 0 {
        return ("hollow", "no_call_recorded");
    }
    (UNRECORDED_STATE, "ran_but_no_outcome_recorded")
}

/// The trailing slot. A PHRASE, NEVER A BARE COUNT — the host's parser rejects a
/// lone integer, because a row is a name and a state and a bare figure reads as a
/// count of something nobody named. `None` when this engine has nothing worth
/// putting there: an empty string would render as a slot that failed rather than
/// as no slot.
pub fn derive_meta(f: &PlaybookFacts) -> Option<String> {
    if !f.loads {
        return Some("cannot be read".to_string());
    }
    match f.live_runs {
        0 => {
            if f.calls == 0 {
                Some("nothing has run".to_string())
            } else {
                None
            }
        }
        1 => Some("1 live".to_string()),
        n => Some(format!("{n} live")),
    }
}

/// Build the document. PURE — no hearth, no network — so what gets served is
/// decidable without standing anything up.
///
/// THE ORDER IS THE CALLER'S. `registry.ts` sorts by what a reader cares about
/// (running, then run, then never, then unreadable) before the rows arrive here,
/// and re-sorting would give the column a different answer from the pane beside it.
pub fn build_document(facts: &[PlaybookFacts]) -> PanelDocument {
    let playbooks = facts
        .iter()
        .take(MAX_ROWS)
        .map(|f| {
            let (state, _because) = derive_state(f);
            let title = display_name(if f.kind.is_empty() { &f.id } else { &f.kind });
            PanelPlaybook {
                id: f.id.clone(),
                title,
                group: if f.owner_kit == OWNING_KIT {
                    GROUP_YOURS.to_string()
                } else {
                    GROUP_SHARED.to_string()
                },
                state: state.to_string(),
                meta: derive_meta(f),
            }
        })
        .collect();
    PanelDocument {
        version: CONTRACT_VERSION,
        playbooks,
    }
}
