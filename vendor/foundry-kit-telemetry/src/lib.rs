//! `foundry-kit-telemetry` — the cross-kit **privacy-preserving telemetry**
//! contract.
//!
//! Decision: `foundry-hearth/decisions/kit-telemetry-contract`. Author guide:
//! `foundry/docs/kit-telemetry-contract.md`. Reference instrumentation:
//! Lore's `lore-engine/src/handlers/telemetry.rs`.
//!
//! Every kit reports deployed-install health + quality **the same way**. The
//! five invariants are enforced **here**, so a kit that emits through this
//! library cannot violate them:
//!
//!  1. **Aggregate on-device, ship rollups only.** A kit [`record`](Telemetry::record)s
//!     raw rows to a local jsonl; only the [`rollup`](Telemetry::rollup) (counts /
//!     rates / percentiles over a window) is eligible for egress.
//!  2. **Categorical + numeric only.** A metric value is a [`MetricValue`] — a
//!     declared categorical label, a count, or a number. There is no way to put
//!     free text / content / PHI through the recorder, and [`validate_envelope`]
//!     rejects any non-numeric metric leaf or unsafe key.
//!  3. **Hash every identifier.** Identifiers reach a rollup only via
//!     [`hash_id`](Telemetry::hash_id) — salted SHA-256 with a per-install salt
//!     held on-device, truncated. The raw id never leaves the device.
//!  4. **Opt-in + local-first.** Telemetry is off by default; the rollup carries
//!     `consent.opted_in` and is written to a human-inspectable
//!     `<kit>-telemetry.jsonl` before any egress.
//!  5. **DP noise on small buckets.** Counts below a threshold get calibrated
//!     Laplace noise ([`apply_dp`]) so a 1-user install can't be de-anonymized.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The shared telemetry taxonomy — pinned event kinds, field names, and closed
/// domains every Foundry producer uses so the fleet's rollups are one consistent,
/// comparable shape (reliability AND product usage). See [`standard`].
pub mod standard;

/// The envelope schema version this crate emits + validates.
pub const SCHEMA_VERSION: u32 = 1;

/// Truncated length (hex chars) of a salted identifier hash.
const HASH_HEX_LEN: usize = 16;

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors from telemetry recording / rollup / validation.
#[derive(Debug)]
pub enum TelemetryError {
    /// A categorical value is not in the field's declared domain (invariant 2).
    UndeclaredCategorical { field: String, value: String },
    /// A metric key or categorical label is not a safe token (invariant 2).
    UnsafeToken(String),
    /// A metric leaf value is not numeric (invariant 2).
    NonNumericMetric(String),
    /// Local jsonl / salt-file I/O failure.
    Io(String),
    /// Serialization failure.
    Serialize(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryError::UndeclaredCategorical { field, value } => write!(
                f,
                "categorical value {value:?} is not in the declared domain for field {field:?}"
            ),
            TelemetryError::UnsafeToken(t) => {
                write!(f, "unsafe metric key/label {t:?} (would risk leaking content)")
            }
            TelemetryError::NonNumericMetric(k) => {
                write!(f, "metric {k:?} has a non-numeric value (rollups are numeric only)")
            }
            TelemetryError::Io(m) => write!(f, "telemetry io error: {m}"),
            TelemetryError::Serialize(m) => write!(f, "telemetry serialize error: {m}"),
        }
    }
}

impl std::error::Error for TelemetryError {}

// ── Safe-token rule (invariant 2 enforcement) ────────────────────────────────

/// A "safe token" — a short identifier of `[a-z0-9_.:-]`, length ≤ 64. Metric
/// keys and categorical labels must be safe tokens so free text / content can
/// never ride in a key or label.
pub fn is_safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

// ── Metric values ────────────────────────────────────────────────────────────

/// A single metric value a kit records. The closed set is the structural
/// enforcement of invariant 2 — there is no `Text(String)` variant.
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// A categorical label from the field's declared domain (a safe token).
    Cat(String),
    /// A monotonic count.
    Count(u64),
    /// A numeric measurement (latency, tokens, ratio, …).
    Num(f64),
}

// ── Envelope ─────────────────────────────────────────────────────────────────

/// A `[start, end)` window over which a rollup aggregates (unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start: u64,
    pub end: u64,
}

/// Consent + privacy parameters carried with every rollup.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Consent {
    /// Off by default — a rollup with `opted_in: false` is NOT egress-eligible.
    pub opted_in: bool,
    /// Differential-privacy epsilon applied to small-count buckets.
    pub dp_epsilon: f64,
}

/// The versioned rollup envelope — the wire contract. The `metrics` object holds
/// only categorical-keyed numeric leaves (validated by [`validate_envelope`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub kit_id: String,
    pub kit_version: String,
    /// Opaque, stable-per-install hash — never the raw install id.
    pub install_id_hash: String,
    pub schema_version: u32,
    pub window: Window,
    pub metrics: serde_json::Value,
    pub consent: Consent,
}

/// Validate an envelope against the privacy invariants (invariant 2): every
/// metric leaf is numeric, and every object key is a safe token. Rejects any
/// string leaf (free text), array, or unsafe key — the structural guarantee that
/// no content/PHI rode along.
pub fn validate_envelope(env: &Envelope) -> Result<(), TelemetryError> {
    fn walk(v: &serde_json::Value, path: &str) -> Result<(), TelemetryError> {
        match v {
            serde_json::Value::Object(map) => {
                for (k, child) in map {
                    if !is_safe_token(k) {
                        return Err(TelemetryError::UnsafeToken(k.clone()));
                    }
                    walk(child, k)?;
                }
                Ok(())
            }
            serde_json::Value::Number(_) => Ok(()),
            // Strings, bools, arrays, nulls are not allowed as metric leaves —
            // numeric rollups only.
            _ => Err(TelemetryError::NonNumericMetric(path.to_string())),
        }
    }
    if env.schema_version != SCHEMA_VERSION {
        return Err(TelemetryError::Serialize(format!(
            "unsupported schema_version {}",
            env.schema_version
        )));
    }
    walk(&env.metrics, "metrics")
}

// ── Differential privacy (invariant 5) ───────────────────────────────────────

/// Add calibrated Laplace noise to a count, returning a non-negative rounded
/// count. Sensitivity 1 (one event moves a count by 1); scale `b = 1/epsilon`.
/// Sampled via inverse-CDF from a uniform so no extra dependency is needed.
pub fn apply_dp(count: u64, epsilon: f64, rng: &mut impl rand::Rng) -> u64 {
    if epsilon <= 0.0 {
        return count;
    }
    let b = 1.0 / epsilon;
    // u ∈ (-0.5, 0.5]; Laplace(b) = -b·sgn(u)·ln(1 - 2|u|).
    let u: f64 = rng.gen::<f64>() - 0.5;
    let noise = -b * u.signum() * (1.0 - 2.0 * u.abs()).max(f64::MIN_POSITIVE).ln();
    let noised = (count as f64 + noise).round();
    if noised < 0.0 {
        0
    } else {
        noised as u64
    }
}

// ── Per-install salt (invariant 3) ───────────────────────────────────────────

/// Resolve the per-install salt: from the `<KIT>_TELEMETRY_SALT` env override
/// (tests/CI), else a salt file held on-device (generated once). The salt is
/// **never** transmitted.
fn resolve_salt(env_var: &str, salt_file: &PathBuf) -> Result<String, TelemetryError> {
    if let Ok(s) = std::env::var(env_var) {
        if !s.is_empty() {
            return Ok(s);
        }
    }
    if let Ok(existing) = std::fs::read_to_string(salt_file) {
        let t = existing.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    // Generate once + persist (0600 on unix).
    let mut raw = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut raw);
    let salt = hex::encode(raw);
    if let Some(parent) = salt_file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TelemetryError::Io(e.to_string()))?;
    }
    std::fs::write(salt_file, &salt).map_err(|e| TelemetryError::Io(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(salt_file, std::fs::Permissions::from_mode(0o600));
    }
    Ok(salt)
}

/// Salted SHA-256 of `value`, truncated to [`HASH_HEX_LEN`] hex chars. Never the
/// raw value.
fn salted_hash(salt: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b"\x1f"); // domain separator between salt and value
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    hex[..HASH_HEX_LEN].to_string()
}

// ── Configuration ────────────────────────────────────────────────────────────

/// Per-kit telemetry configuration.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Kebab-case kit id, e.g. `lore-kit`.
    pub kit_id: String,
    /// Kit semver at emit time.
    pub kit_version: String,
    /// The human-inspectable local rollup-source jsonl (`<kit>-telemetry.jsonl`).
    pub local_file: PathBuf,
    /// The on-device per-install salt file.
    pub salt_file: PathBuf,
    /// A sidecar carrying the producer's version (`<kit>-telemetry.version`).
    /// The raw jsonl rows carry no version, so a producer persists it here for
    /// the out-of-process emitter to recover on rollup (see [`Telemetry::new`]).
    pub version_file: PathBuf,
    /// Env var that overrides the salt (tests/CI), e.g. `LORE_TELEMETRY_SALT`.
    pub salt_env: String,
    /// Declared categorical domains: field → its closed set of allowed labels.
    /// A `Cat` value outside its field's domain is rejected at `record` time.
    pub categorical_domains: BTreeMap<String, BTreeSet<String>>,
    /// Counts at or below this bucket size get DP noise on rollup.
    pub small_bucket_threshold: u64,
    /// DP epsilon recorded in the envelope + used for small-bucket noise.
    pub dp_epsilon: f64,
}

impl TelemetryConfig {
    /// Minimal config from a kit id + version + a directory; derives the local
    /// file, salt file, and salt env-var names by convention.
    pub fn new(kit_id: impl Into<String>, kit_version: impl Into<String>, dir: PathBuf) -> Self {
        let kit_id = kit_id.into();
        let upper = kit_id.replace('-', "_").to_uppercase();
        TelemetryConfig {
            local_file: dir.join(format!("{kit_id}-telemetry.jsonl")),
            salt_file: dir.join(format!("{kit_id}-telemetry.salt")),
            version_file: dir.join(format!("{kit_id}-telemetry.version")),
            salt_env: format!("{upper}_TELEMETRY_SALT"),
            categorical_domains: BTreeMap::new(),
            small_bucket_threshold: 5,
            dp_epsilon: 1.0,
            kit_id,
            kit_version: kit_version.into(),
        }
    }

    /// Declare a categorical field's closed domain.
    pub fn with_domain(
        mut self,
        field: impl Into<String>,
        labels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.categorical_domains.insert(
            field.into(),
            labels.into_iter().map(Into::into).collect(),
        );
        self
    }
}

// ── The recorder ─────────────────────────────────────────────────────────────

/// A raw on-device telemetry row (one event). Categorical + numeric only.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawRow {
    event_kind: String,
    ts: u64,
    /// Categorical fields: field → label (label is a safe token).
    #[serde(default)]
    cats: BTreeMap<String, String>,
    /// Numeric fields: field → value.
    #[serde(default)]
    nums: BTreeMap<String, f64>,
}

/// The on-device telemetry recorder + roller. Holds the resolved salt + the
/// opaque install hash. Records raw rows locally; rolls them up into a validated
/// envelope.
pub struct Telemetry {
    cfg: TelemetryConfig,
    salt: String,
    install_id_hash: String,
}

impl Telemetry {
    /// Resolve the salt + install hash and prepare the recorder.
    pub fn new(cfg: TelemetryConfig) -> Result<Self, TelemetryError> {
        let salt = resolve_salt(&cfg.salt_env, &cfg.salt_file)?;
        // The install hash is the salt's own hash — opaque, stable per install,
        // reveals nothing (the salt never leaves the device).
        let install_id_hash = salted_hash(&salt, "install");
        // Persist a real version to the sidecar so the out-of-process emitter can
        // recover it on rollup. Best-effort; a version is a safe token so this
        // never widens the content contract. Skip `"unknown"`/empty/unsafe so a
        // reconstructing reader (the emitter itself) can't clobber a real value.
        if is_safe_token(&cfg.kit_version) && cfg.kit_version != "unknown" {
            if let Some(parent) = cfg.version_file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&cfg.version_file, &cfg.kit_version);
        }
        Ok(Telemetry {
            cfg,
            salt,
            install_id_hash,
        })
    }

    /// Read the persisted producer version from a kit dir, if a prior producer
    /// wrote a valid one. Returns `None` when absent/empty/unsafe so callers can
    /// fall back to `"unknown"`. Used by the emitter, which rebuilds a rollup
    /// out-of-process and cannot know the producer's version otherwise.
    pub fn read_persisted_version(dir: &std::path::Path, kit_id: &str) -> Option<String> {
        let file = dir.join(format!("{kit_id}-telemetry.version"));
        let raw = std::fs::read_to_string(file).ok()?;
        let v = raw.trim();
        (is_safe_token(v)).then(|| v.to_string())
    }

    /// The opaque, stable-per-install identifier hash for envelopes.
    pub fn install_id_hash(&self) -> &str {
        &self.install_id_hash
    }

    /// The human-inspectable local rollup-source jsonl path (`<kit>-telemetry.jsonl`).
    pub fn local_file(&self) -> &std::path::Path {
        &self.cfg.local_file
    }

    /// Salted hash of an identifier (space / client / project / user). The ONLY
    /// sanctioned way an identifier may appear in telemetry (invariant 3).
    pub fn hash_id(&self, value: &str) -> String {
        salted_hash(&self.salt, value)
    }

    /// Record one event as a raw row appended to the local jsonl. Categorical
    /// labels must be safe tokens AND in their declared domain (invariant 2);
    /// otherwise the row is rejected and nothing is written.
    pub fn record(
        &self,
        event_kind: &str,
        fields: &[(&str, MetricValue)],
    ) -> Result<(), TelemetryError> {
        let mut cats = BTreeMap::new();
        let mut nums = BTreeMap::new();
        for (field, value) in fields {
            if !is_safe_token(field) {
                return Err(TelemetryError::UnsafeToken((*field).to_string()));
            }
            match value {
                MetricValue::Cat(label) => {
                    if !is_safe_token(label) {
                        return Err(TelemetryError::UnsafeToken(label.clone()));
                    }
                    if let Some(domain) = self.cfg.categorical_domains.get(*field) {
                        if !domain.contains(label) {
                            return Err(TelemetryError::UndeclaredCategorical {
                                field: (*field).to_string(),
                                value: label.clone(),
                            });
                        }
                    }
                    cats.insert((*field).to_string(), label.clone());
                }
                MetricValue::Count(n) => {
                    nums.insert((*field).to_string(), *n as f64);
                }
                MetricValue::Num(x) => {
                    nums.insert((*field).to_string(), *x);
                }
            }
        }
        let row = RawRow {
            event_kind: event_kind.to_string(),
            ts: now_secs(),
            cats,
            nums,
        };
        let line = serde_json::to_string(&row).map_err(|e| TelemetryError::Serialize(e.to_string()))?;
        if let Some(parent) = self.cfg.local_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TelemetryError::Io(e.to_string()))?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.cfg.local_file)
            .map_err(|e| TelemetryError::Io(e.to_string()))?;
        writeln!(f, "{line}").map_err(|e| TelemetryError::Io(e.to_string()))?;
        Ok(())
    }

    /// Record a standard event built by a [`standard`] constructor
    /// (`tel.record_event(standard::page("recording"))?`). Thin sugar over
    /// [`record`](Self::record) so producers use the pinned taxonomy instead of
    /// ad-hoc kind/field strings.
    pub fn record_event(
        &self,
        event: (&str, Vec<(&str, MetricValue)>),
    ) -> Result<(), TelemetryError> {
        let (kind, fields) = event;
        self.record(kind, &fields)
    }

    /// Aggregate the local rows in `window` into a validated rollup envelope.
    ///
    /// Per categorical field: a `by_<field>` count-per-label map (DP-noised when
    /// below the small-bucket threshold). Per numeric field: `mean`, `p50`,
    /// `p95`, and `count`. `consent.opted_in` defaults to `false` (not
    /// egress-eligible) unless `opted_in` is true.
    pub fn rollup(&self, window: Window, opted_in: bool) -> Result<Envelope, TelemetryError> {
        let rows = self.read_rows()?;
        let mut cat_counts: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
        let mut num_series: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for row in &rows {
            if row.ts < window.start || row.ts >= window.end {
                continue;
            }
            for (field, label) in &row.cats {
                *cat_counts
                    .entry(format!("by_{field}"))
                    .or_default()
                    .entry(label.clone())
                    .or_insert(0) += 1;
            }
            for (field, value) in &row.nums {
                num_series.entry(field.clone()).or_default().push(*value);
            }
        }

        let mut metrics = serde_json::Map::new();
        let mut rng = rand::rngs::OsRng;
        for (key, labels) in cat_counts {
            let mut obj = serde_json::Map::new();
            for (label, count) in labels {
                let reported = if count <= self.cfg.small_bucket_threshold {
                    apply_dp(count, self.cfg.dp_epsilon, &mut rng)
                } else {
                    count
                };
                obj.insert(label, serde_json::json!(reported));
            }
            metrics.insert(key, serde_json::Value::Object(obj));
        }
        for (field, mut series) in num_series {
            series.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = series.len();
            let mean = series.iter().sum::<f64>() / n as f64;
            let mut obj = serde_json::Map::new();
            obj.insert("count".into(), serde_json::json!(n));
            obj.insert("mean".into(), serde_json::json!(round2(mean)));
            obj.insert("p50".into(), serde_json::json!(round2(quantile(&series, 0.50))));
            obj.insert("p95".into(), serde_json::json!(round2(quantile(&series, 0.95))));
            metrics.insert(field, serde_json::Value::Object(obj));
        }

        let env = Envelope {
            kit_id: self.cfg.kit_id.clone(),
            kit_version: self.cfg.kit_version.clone(),
            install_id_hash: self.install_id_hash.clone(),
            schema_version: SCHEMA_VERSION,
            window,
            metrics: serde_json::Value::Object(metrics),
            consent: Consent {
                opted_in,
                dp_epsilon: self.cfg.dp_epsilon,
            },
        };
        // The library never emits an envelope that fails its own invariants.
        validate_envelope(&env)?;
        Ok(env)
    }

    fn read_rows(&self) -> Result<Vec<RawRow>, TelemetryError> {
        let content = match std::fs::read_to_string(&self.cfg.local_file) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(TelemetryError::Io(e.to_string())),
        };
        let mut rows = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(row) = serde_json::from_str::<RawRow>(line) {
                rows.push(row);
            }
        }
        Ok(rows)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
