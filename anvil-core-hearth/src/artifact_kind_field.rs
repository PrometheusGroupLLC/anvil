//! The durable-sink field naming the **governed artifact kind** a record
//! belongs to — `track`, `lore_query`, `decision`, … — and the one legacy
//! spelling it replaces.
//!
//! `NG-CORRELATION-KEY-DECISION-AMENDMENT`, answered by Nick on 2026-07-30
//! (decision 6, *"Two fields are simply named wrong"* → "yes fix them"),
//! narrowed to this rename alone:
//!
//! - the canonical field is **`artifact_kind`**. The old spelling
//!   `workflow_kind` was factually wrong about its own contents: it never
//!   carried a definition kind.
//! - **existing rows are never rewritten** and **no sink file is renamed**, so
//!   both spellings coexist on disk permanently. Every reader must therefore
//!   fold them into **one** internal model.
//! - a row carrying **both** spellings is the dual-emission shape the amendment
//!   forbids at the log seam. A reader must refuse it, not silently pick one —
//!   picking one is how "which name won" becomes unanswerable later.
//!
//! The **writers are unchanged by this module.** Flipping a writer to the
//! canonical spelling additionally requires the rotation boundary and the
//! migration-boundary record the amendment mandates. Readers move first.

/// The canonical durable-sink field for a governed artifact kind.
pub const CANONICAL_ARTIFACT_KIND_FIELD: &str = "artifact_kind";

/// The legacy spelling. Present on every row written before the rename and,
/// because those rows are immutable, present on disk forever.
pub const LEGACY_ARTIFACT_KIND_FIELD: &str = "workflow_kind";

/// What a durable row says about its governed artifact kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactKindRead {
    /// The canonical `artifact_kind` field, alone.
    Canonical(String),
    /// The legacy `workflow_kind` field, alone.
    Legacy(String),
    /// Neither field. Reads as the empty kind — additive-field parity with
    /// rows written before either spelling existed.
    Missing,
    /// Both spellings on one row. Forbidden; the caller must reject the row.
    BothSpellings { canonical: String, legacy: String },
}

impl ArtifactKindRead {
    /// The folded value. `None` only for [`Missing`](Self::Missing) and
    /// [`BothSpellings`](Self::BothSpellings) — a caller that reaches for a
    /// value on an ambiguous row gets nothing rather than a coin flip.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Canonical(v) | Self::Legacy(v) => Some(v),
            Self::Missing | Self::BothSpellings { .. } => None,
        }
    }

    /// The folded value, or the empty string. Use only after rejecting
    /// [`BothSpellings`](Self::BothSpellings).
    pub fn or_empty(&self) -> String {
        self.value().unwrap_or_default().to_string()
    }

    /// True when the row carries both spellings and must be rejected.
    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::BothSpellings { .. })
    }

    /// The message a rejecting reader should carry.
    pub fn ambiguity_message(&self, line: &str) -> String {
        match self {
            Self::BothSpellings { canonical, legacy } => format!(
                "row carries both `{CANONICAL_ARTIFACT_KIND_FIELD}` ({canonical}) and \
                 `{LEGACY_ARTIFACT_KIND_FIELD}` ({legacy}); dual emission of the artifact-kind \
                 field is forbidden at the log seam: {line}"
            ),
            _ => String::new(),
        }
    }
}

/// Fold a row's artifact-kind field, whichever spelling it uses.
///
/// `extract` is the caller's own field scanner, so each adapter keeps its own
/// unescaping behaviour and this module adds no second JSON parser.
pub fn read_artifact_kind<F>(line: &str, extract: F) -> ArtifactKindRead
where
    F: Fn(&str, &str) -> Option<String>,
{
    let canonical = extract(line, CANONICAL_ARTIFACT_KIND_FIELD);
    let legacy = extract(line, LEGACY_ARTIFACT_KIND_FIELD);
    match (canonical, legacy) {
        (Some(canonical), Some(legacy)) => ArtifactKindRead::BothSpellings { canonical, legacy },
        (Some(canonical), None) => ArtifactKindRead::Canonical(canonical),
        (None, Some(legacy)) => ArtifactKindRead::Legacy(legacy),
        (None, None) => ArtifactKindRead::Missing,
    }
}
